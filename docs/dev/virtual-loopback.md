# Virtual audio loopback — the default loopback transport

> **Evidence currency (2026-07-18):** the results recorded below predate the fade-aware ladder
> arc. No loopback run in the tree is newer than 2026-06-25, and several shipped modes
> (`QPSK250-D`, `QPSK500-D`, `MFSK16`, JS8) have never been run on real audio at all. See
> [loopback-revalidation-plan.md](loopback-revalidation-plan.md).


OpenPulseHF validates the modem signal path through three transports, each gated
on the previous one passing:

| Rung | Transport | Script | When |
|---|---|---|---|
| 1 | **Virtual** (snd-aloop, single clock, no analog) | `scripts/run-loopback-virtual.sh` | **default — every run** |
| 2a | **Dual-card** (two USB soundcards on one host, cable) | `scripts/run-loopback-dualcard.sh` | real analog path, no second machine. **Not a dual-clock check** — measured +0.10 ppm, see [dualcard-loopback.md](dualcard-loopback.md) |
| 2b | **Two Pis** (two soundcards, cable + ground-loop isolator) | `scripts/run-loopback-rpi51-rpi52.sh` | on request |
| 3 | **On-air** (real rigs / RF) | `scripts/run-onair-*.sh` | after rungs 1 and 2 pass |

The three differ by exactly which real-world effects they add:

- **In-process channel sim** (`openpulse-testmatrix`) — no audio device at all; pure DSP through a simulated channel.
- **Virtual loopback** — adds the real cpal + ALSA + 8 kHz↔48 kHz resampler device path, but with **one shared clock** and **no analog cable/isolator**.
- **Hardware loopback** — adds the **analog cable + ground-loop isolator**. It was assumed to add **two independent soundcard clocks** too; on the dual-card rig that is false (measured +0.10 ppm — USB adapters slave to the host frame clock). Genuine sample-rate offset needs two hosts (rung 2b).
- **On-air** — adds RF, real noise, multipath, and (still) two independent station clocks.

A failure that appears only when you move *up* a rung tells you which layer is responsible. This is how the SCFDMA52-\*/64QAM hardware failures were diagnosed (see below).

## Why virtual is the default

The virtual rung catches DSP, acquisition, framing, resampler, and config regressions on the real audio path without needing two Raspberry Pis and a cable. It is deterministic, fast, and runnable in CI (given the `snd-aloop` module). Hardware and on-air then only need to be run to validate the effects they uniquely add (analog response, RF — and true SRO only on a genuinely two-host rig).

### In CI

The `virtual-loopback-smoke` job in `.github/workflows/ci.yml` (manual `workflow_dispatch`) runs a representative subset (`BPSK250 QPSK500 OFDM52`) through the virtual rung on the GitHub runner. It is **non-blocking** (`continue-on-error`) and **gracefully skips** when `snd-aloop` is unavailable on the runner image, because audio timing under CI scheduling is best-effort — the authoritative virtual gate is the local run before a hardware/on-air session. Once the job is observed stable on the runner image it can be promoted to a blocking gate.

## Setup

```bash
scripts/setup-virtual-loopback.sh        # loads snd-aloop, writes aloop_tx/aloop_rx PCMs to ~/.asoundrc
cargo build --release -p openpulse-cli   # cpal CLI (default features include cpal)
scripts/run-loopback-virtual.sh          # runs every registered mode through the virtual loopback
```

`snd-aloop` cross-links PCM device 0 ↔ device 1 (audio played on `(dev0, subN)` is captured on `(dev1, subN)`). ALSA namehints only expose `DEV=0` and cpal matches `--device` by exact enumerated name, so the setup script publishes two named `plug` PCMs with `hint` blocks: `aloop_tx` → `hw:Loopback,0,0`, `aloop_rx` → `hw:Loopback,1,0`.

`run-loopback-virtual.sh` enumerates the full mode set from `openpulse modes` (no curated exclusions). Modes that are physically impossible at 8 kHz audio (the 9600-baud modes need ≥4 samples/symbol → Fs ≥ 38.4 kHz) are reported as **SKIP with reason**, not silently dropped. Override the mode set with `MODES="BPSK250 SCFDMA52"` for targeted runs.

### cpal transmit pacing

The cpal output stream previously called `play()` immediately on open — before any
samples were buffered — so the output callback fired against an empty queue and
**underran at the frame start**, corrupting the transmit. This was flaky for slow
(BPSK31/63) and bursty wideband (OFDM52) modes. The fix (`crates/openpulse-audio/src/cpal_backend.rs`):

- **Defer `play()` to the first `write()`**, once the frame is buffered, so the
  callback never starts against an empty queue.
- **Append a short trailing-silence pad in `flush()`** so the unavoidable
  pull-based end-of-stream underrun (ALSA logs one `snd_pcm_recover` line when the
  stream is dropped) lands in silence and never clips the final data symbols.

After the fix, OFDM52 decodes reliably on the virtual rig instead of intermittently.
A single close-time underrun line is benign; the loopback script only treats **≥2**
underruns as a TX-pacing failure (one is the stream-close artifact). `RETRIES`
(default 3) remains as a safety net for the hardware rig's dual-clock jitter.

## Diagnosing analog-path effects: the chirp probe

`scripts/measure-loopback-response.sh` plays a linear chirp through a real loopback path and `scripts/analyze-loopback-response.py` computes the magnitude response (a linear chirp has a flat source spectrum, so the received PSD shape *is* |H(f)|²). Running it at the card's native 48 kHz (no resample) vs at 8 kHz (through the resampler) separates the analog path from the ALSA resampler.

## Diagnostic finding (2026-06-13): SCFDMA52-\* / 64QAM

These modes fail 0/8 on the **hardware** rig but pass on the **virtual** rig, and the chirp probe shows the analog path is flat ±0.2 dB from 250 Hz to 3.75 kHz (the only magnitude rolloff is the resampler above ~3.2 kHz, well above SCFDMA52's 2.5 kHz top subcarrier). Conclusion at the time: the hardware failure is the **two independent soundcard clocks (sample-rate offset)** and/or **analog group-delay/phase**, **not** bandwidth, SNR, or a code bug.

> **CORRECTION (2026-07-20).** The first half of that disjunction is **eliminated by measurement**: the
> dual-card rig runs at **+0.10 ppm** (`--sro-check`), because both USB adapters slave to the host's USB
> frame clock rather than free-running. The clocks were never independent, so SRO cannot be what broke
> these modes here. Of the two candidates the original diagnosis offered, **only analog
> group-delay/phase survives** — and it has not been tested. Do not schedule "sample-rate-offset
> tracking in the wideband demodulators" on the strength of this rig's evidence; on a genuine two-host
> rig SRO may still matter for deployment, but that has to be measured there, not assumed from topology.

## Full coded sweep at HEAD (2026-07-20) — 63/73, and what it settles

Task B of [loopback-revalidation-plan.md](loopback-revalidation-plan.md), run `FEC=rs` to match the
dual-card sweep so the two rungs are comparable. Report:
`docs/dev/test-reports/loopback-virtual-2026-07-20T090857Z.json`.

**63 pass, 6 fail, 4 skip (of 73).**

### It could not run at all before this date

`aloop_tx` was unreachable: cpal's ALSA enumeration is stateful, and holding `cpal::Device` values
alive while iterating **silently truncates the list** (39 devices when each is named and dropped, 18
when retained, 4 when collected first). `select_cpal_device` retained them, so the resolver saw a
truncated list and returned `device not found` for a device that `openpulse devices` had just listed.
`hwloop_tx` happened to fall inside the surviving prefix, which is why the hardware rung worked and
this one did not. Fixed by enumerating twice — names only in pass 1, retaining just the match in
pass 2. Gate: `crates/openpulse-audio/tests/device_enumeration.rs`.

### Virtual × hardware: the comparison that settles the attribution

The virtual rung shares the entire software path with the dual-card rig but has **no analog cable**.
With sample-rate offset eliminated by measurement (+0.10 ppm — see
[dualcard-loopback.md](dualcard-loopback.md)), a mode that passes here and fails there has exactly one
remaining variable: **the analog path**.

| Modes | virtual | dual-card | Verdict |
|---|---|---|---|
| `64QAM{500,1000,2000-RRC}`, `SCFDMA52-{16QAM,32QAM,64QAM,64QAM-P4}`, `PILOT-QPSK500` | pass | fail | **Analog path** — confirms the surviving half of the 2026-06-13 disjunction |
| `8PSK2000`, `BPSK250-RRC`, `SCFDMA52-LP` | fail | fail | **Software** — fails with no analog path at all. Narrowed further below. |
| `QPSK125` | fail | pass | Virtual-only, and **consistent** (3/3) — see below |

This closes the question the 2026-06-13 note left open. That note offered "two independent soundcard
clocks (sample-rate offset) **and/or** analog group-delay/phase". The clock half was eliminated by
measurement; this sweep confirms the other half by construction. **`64QAM` and `SCFDMA52-*` are limited
by the analog path, not by a code defect and not by SRO.**

Note `BPSK31`'s dual-card FAIL in that run is stale — it was the 60 s flush clamp, fixed the same day,
and it now passes on both rungs.

### Narrowed by a third rung (2026-07-20, after the sweep)

"Fails on both audio rungs" is not the same as "the DSP is wrong". Re-running the three through the
**in-process** `ChannelSimHarness` — no cpal, no ALSA, no resampler — on a clean channel splits them
again:

| Mode | in-process | virtual | dual-card | Layer |
|---|---|---|---|---|
| `8PSK2000` | **fail** | fail | fail | **DSP core** |
| `BPSK250-RRC` | pass | fail | fail | **audio I/O path** (cpal/ALSA/resampler) |
| `SCFDMA52-LP` | pass | fail | fail | **audio I/O path** |
| `QPSK125` | pass | fail | pass | **audio I/O path**, virtual only |

So three rungs isolate three different variables, and only `8PSK2000` is a modem-DSP defect. The other
three pass every DSP test and fail once real audio I/O is in the path, which is a materially different
place to look than "the waveform is broken".

**`8PSK2000` — diagnosed and fixed.** It fails in-process on a *clean, noiseless* channel, the repo's
signature for a bug rather than a limitation. Cause: `samples_per_symbol` enforced a floor of 4, so
`8PSK2000` at 8 kHz (exactly 4 samples/symbol) was accepted, modulated and transmitted — and nothing
could decode it. The plain pulse's residual ISI grows as `n` shrinks and at 4 sps exceeds 8PSK's ±22.5°
margin. Measured: `8PSK500` (16 sps) and `8PSK1000` (8 sps) round-trip; `8PSK2000` (4 sps) does not;
`8PSK2000-RRC` at the same 4 sps does; and plain `QPSK2000` at 4 sps does. **It is the phase margin
that runs out, not the sample rate.** The floor is **5**: the pre-existing `psk8_9600_loopback_48k`
test (5 sps, plain pulse) refuted a first attempt that used 8 — generalised past the boundary that
made the measurement true, the same mistake as the `RsStrong is free` entry in CLAUDE.md. The plain pulse now requires ≥5 samples/symbol and refuses the
combination with a message naming the `-RRC` variant, on both the transmit and receive paths. The mode
is still advertised because at 48 kHz it is 24 sps and perfectly usable — only the pairing with 8 kHz
is refused. Gate: `plugins/psk8/tests/plain_pulse_sps_floor.rs`.

### Open items this sweep produced

- **`8PSK2000`** — **FIXED** (see above): a plain-pulse samples/symbol floor, now enforced.
- **`BPSK250-RRC`, `SCFDMA52-LP`** — moved out of the "dual-clock" group, and then narrowed again: both
  pass in-process, so the defect is in the **audio I/O path**, not the DSP. Not diagnosed further.
- **`QPSK125`** — fails here 3/3 while passing on hardware, which is the inverse of the usual
  direction. Both runners use identical flags and the wire length is identical under `rs` (both pad to
  one 255-byte block), so payload size is not the difference. The RX log shows the demodulator
  recovering ~82 of the 255 bytes it needs (`FEC data length 82 is shorter than one 255-byte block`),
  i.e. it is seeing about a third of the frame. Not diagnosed further.

### Runner parity

`FSK4-ACK` and `MFSK16-ACK` were recorded as failures here while the dual-card runner **skips them by
rule** ("ACK-channel waveform, exercised by the ARQ tests not a data sweep"). Two runners disagreeing
about what is even in scope makes their results incomparable, so `skip_reason_for` is now shared in
shape between them. This runner also gained `FEC=` support: it previously never passed `--fec` at all,
which silently made the revalidation plan's own `FEC=rs MODES="QPSK250-D ..."` command inert — and a
no-FEC differential run decodes 0.00 **by design**, so it would have manufactured a regression.
