# Virtual audio loopback — the default loopback transport

OpenPulseHF validates the modem signal path through three transports, each gated
on the previous one passing:

| Rung | Transport | Script | When |
|---|---|---|---|
| 1 | **Virtual** (snd-aloop, single clock, no analog) | `scripts/run-loopback-virtual.sh` | **default — every run** |
| 2 | **Hardware** (two Pis, two soundcards, cable + ground-loop isolator) | `scripts/run-loopback-rpi51-rpi52.sh` | on request |
| 3 | **On-air** (real rigs / RF) | `scripts/run-onair-*.sh` | after rungs 1 and 2 pass |

The three differ by exactly which real-world effects they add:

- **In-process channel sim** (`openpulse-testmatrix`) — no audio device at all; pure DSP through a simulated channel.
- **Virtual loopback** — adds the real cpal + ALSA + 8 kHz↔48 kHz resampler device path, but with **one shared clock** and **no analog cable/isolator**.
- **Hardware loopback** — adds **two independent soundcard clocks** (sample-rate offset/drift) and the **analog cable + ground-loop isolator**.
- **On-air** — adds RF, real noise, multipath, and (still) two independent station clocks.

A failure that appears only when you move *up* a rung tells you which layer is responsible. This is how the SCFDMA52-\*/64QAM hardware failures were diagnosed (see below).

## Why virtual is the default

The virtual rung catches DSP, acquisition, framing, resampler, and config regressions on the real audio path without needing two Raspberry Pis and a cable. It is deterministic, fast, and runnable in CI (given the `snd-aloop` module). Hardware and on-air then only need to be run to validate the effects they uniquely add (dual-clock SRO, analog response, RF).

## Setup

```bash
scripts/setup-virtual-loopback.sh        # loads snd-aloop, writes aloop_tx/aloop_rx PCMs to ~/.asoundrc
cargo build --release -p openpulse-cli   # cpal CLI (default features include cpal)
scripts/run-loopback-virtual.sh          # runs every registered mode through the virtual loopback
```

`snd-aloop` cross-links PCM device 0 ↔ device 1 (audio played on `(dev0, subN)` is captured on `(dev1, subN)`). ALSA namehints only expose `DEV=0` and cpal matches `--device` by exact enumerated name, so the setup script publishes two named `plug` PCMs with `hint` blocks: `aloop_tx` → `hw:Loopback,0,0`, `aloop_rx` → `hw:Loopback,1,0`.

`run-loopback-virtual.sh` enumerates the full mode set from `openpulse modes` (no curated exclusions). Modes that are physically impossible at 8 kHz audio (the 9600-baud modes need ≥4 samples/symbol → Fs ≥ 38.4 kHz) are reported as **SKIP with reason**, not silently dropped. Override the mode set with `MODES="BPSK250 SCFDMA52"` for targeted runs.

### Known rig limitation

The cpal output callback emits silence on buffer underflow, so very slow (BPSK31/63) and bursty wideband (OFDM52) modes can intermittently **underrun** and produce a corrupted transmit, causing a flaky fail. The script retries each mode up to `RETRIES` (default 3) to absorb this. A proper fix is to pre-buffer/pace the cpal transmit side; tracked as follow-up.

## Diagnosing analog-path effects: the chirp probe

`scripts/measure-loopback-response.sh` plays a linear chirp through a real loopback path and `scripts/analyze-loopback-response.py` computes the magnitude response (a linear chirp has a flat source spectrum, so the received PSD shape *is* |H(f)|²). Running it at the card's native 48 kHz (no resample) vs at 8 kHz (through the resampler) separates the analog path from the ALSA resampler.

## Diagnostic finding (2026-06-13): SCFDMA52-\* / 64QAM

These modes fail 0/8 on the **hardware** rig but pass on the **virtual** rig, and the chirp probe shows the analog path is flat ±0.2 dB from 250 Hz to 3.75 kHz (the only magnitude rolloff is the resampler above ~3.2 kHz, well above SCFDMA52's 2.5 kHz top subcarrier). Conclusion: the hardware failure is the **two independent soundcard clocks (sample-rate offset)** and/or **analog group-delay/phase**, **not** bandwidth, SNR, or a code bug — the DSP decodes correctly through the identical software+resampler path when the clock is shared. Wideband multicarrier and dense QAM are sample-rate-offset-intolerant; narrowband/single-carrier modes are not. The fix is sample-rate-offset tracking in the wideband demodulators (or disciplined/shared clocks on hardware); on-air has the same two-clock condition, so it matters for deployment.
