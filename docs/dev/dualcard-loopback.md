# Dual-card hardware loopback — the two-soundcard rung on one host

> **Evidence currency (2026-07-18):** the results recorded below predate the fade-aware ladder
> arc. No loopback run in the tree is newer than 2026-06-25, and several shipped modes
> (`QPSK250-D`, `QPSK500-D`, `MFSK16`, JS8) have never been run on real audio at all. See
> [loopback-revalidation-plan.md](loopback-revalidation-plan.md).


This rig runs the modem TX→RX through **two USB soundcards plugged into the same
PC**, joined by an analog cable. It is the **hardware / dual-clock rung** of the
loopback ladder (see [virtual-loopback.md](virtual-loopback.md)) made runnable on
a single machine — no SSH, no two Raspberry Pis.

| Rung | Transport | Script | Adds over the rung below |
|---|---|---|---|
| 1 | Virtual (snd-aloop, one clock) | `scripts/run-loopback-virtual.sh` | real cpal+ALSA+resampler path |
| 2a | **Dual-card (two USB cards, one host)** | `scripts/run-loopback-dualcard.sh` | **two independent clocks (SRO) + analog cable** |
| 2b | Two Pis (two hosts) | `scripts/run-loopback-rpi51-rpi52.sh` | physically separate machines |
| 3 | On-air (real rigs / RF) | `scripts/run-onair-*.sh` | RF, noise, multipath |

Rung 2a delivers exactly what the single-clock virtual rig **cannot**: two
independent sample clocks (sample-rate offset / drift) and a real analog cable.
That is the condition that broke the wideband multicarrier (SCFDMA52-\*) and dense
QAM (64QAM) modes on the two-Pi rig — see the
`project-loopback-mode-matrix` memory and the SRO diagnosis in
[virtual-loopback.md](virtual-loopback.md). Rung 2a reproduces it without needing
two machines, so SRO-tracking work can be iterated locally.

## Hardware

Two C-Media USB audio adapters on this host:

- `pci-0000:07:00.3` → ALSA card `Device`  (TX: its output drives the cable)
- `pci-0000:07:00.4` → ALSA card `Device_1` (RX: its mic input receives the cable)

Cards are pinned by **USB path** (stable across replug) — the ALSA card name
(`Device` / `Device_1`) and index are assigned in enumeration order and can swap.

```
USB card 3 (Device) speaker/line OUT  --analog cable-->  USB card 4 (Device_1) mic IN
```

## Setup

```bash
scripts/setup-dualcard-loopback.sh        # resolves both cards by USB path,
                                          # writes hwloop_tx / hwloop_rx PCMs to
                                          # ~/.asoundrc, normalises mixers
cargo build --release -p openpulse-cli    # cpal build (default features include cpal)
```

`setup-dualcard-loopback.sh` publishes two named `plug` PCMs with `hint` blocks so
the cpal CLI enumerates them by exact name (cpal matches `--device` on the
enumerated name; bare `plughw:CARD=...` is not enumerated — same constraint the
virtual rig hit). They resample 8 kHz ↔ the cards' native 48 kHz.

### Capture gain — the one real gotcha

These adapters expose a **mic** input with a +23 dB preamp. A line-level output
drives it into hard clipping at full capture gain, and a clipped BPSK signal will
not decode (this is why a max-gain default looks like "nothing works"). The setup
script sets a moderate `CAPTURE_GAIN=16` (measured: modem TX peaks ~0.79 FS,
unclipped). Override with `CAPTURE_GAIN=10 scripts/setup-dualcard-loopback.sh` if
your levels differ.

Confirm the cable and level before a run:

```bash
scripts/run-loopback-dualcard.sh --level-check
#   captured rms=0.4967  peak=0.7940  clipped_samples=0
#   OK -- signal present and unclipped; safe to run the matrix.
```

`--level-check` measures the actual captured RMS/peak with `arecord`, so it
distinguishes a missing/reversed cable (no signal) from an over-hot input
(clipping) and tells you which knob to turn.

## Running

```bash
scripts/run-loopback-dualcard.sh --quick                 # one case per mode family
scripts/run-loopback-dualcard.sh --full                  # broader baud/payload sweep
scripts/run-loopback-dualcard.sh --single-case "BPSK250|64"
FEC=soft-concatenated scripts/run-loopback-dualcard.sh --single-case "SCFDMA26-16QAM|64"
```

Both ends run locally as two cpal CLI processes. The RX receiver starts first and
the TX waits `IRS_STARTUP_WAIT` (10 s) so the frame lands after the ~6.4 s AFC
settling window. Per case the mixers are re-normalised (the C-Media hardware AGC
drifts capture gain down after strong frames) and each case is attempted up to
`RETRIES` (3) times — the wideband modes (OFDM52, SCFDMA52) intermittently miss
acquisition in long back-to-back sweeps even though they pass in isolation, so a
single attempt understates them. Results are written as JSON to
`docs/dev/test-reports/loopback-dualcard-<tier>-<ts>.json` (each case records its
`attempts`).

Useful env overrides: `TX_DEVICE`/`RX_DEVICE`, `TX_CARD`/`RX_CARD`,
`CAPTURE_GAIN`, `FEC`, `RETRIES`, `IRS_LISTEN_MS`, `IRS_STARTUP_WAIT`, `OUTPUT_DIR`.

## Status (2026-07-19) — MFSK16 validated, QPSK250-D blocked

First run after the registry-driven `--full` change (loopback-revalidation-plan task A). Rig: cards
`Device`/`Device_1` (USB `07:00.4-2` / `07:00.3-2`), `CAPTURE_GAIN=16`, TX playback raised 14 → 30
(the default left the captured level at rms 0.033; at 30 it is rms 0.222 / peak 0.353, unclipped).
Binary built with `--features cpal-backend` — without it the CLI silently falls back to the loopback
backend and would report a "hardware" pass that never touched a sound card.

| Mode | FEC | Result |
|---|---|---|
| **MFSK16** | `rs` | **PASS** — first validation of `hpx_hf` SL1 on real audio |
| QPSK250 | none | PASS (attempt 2) |
| QPSK250 | `rs` | FAIL — `FEC data length 128 is not a non-zero multiple of 255` |
| **QPSK250-D** | `rs` | **FAIL** — same framing error (len 123/124) |
| **QPSK250-D** | `ldpc` | **FAIL** — `differential QPSK has no soft-LLR path` |

### QPSK250-D (SL6) cannot currently complete over a real audio path

This is the rung the differential work exists for (#923), and it is boxed in by three facts that only
collide on hardware:

1. **It requires FEC.** Uncoded differential decodes 0.00 by design — a per-slip dibit error has to be
   corrected.
2. **`rs` needs a frame-exact demodulator output.** `FecCodec` emits multiples of 255 bytes and the
   decoder rejects anything else. In-process the demod returns the exact transmitted byte count; over
   a real audio link it returns whatever the onset/timing search sliced (123, 124, 128 …), so RS
   never gets a valid block. **This is not `-D`-specific** — plain `QPSK250 + rs` fails identically,
   which is what the ablation established. Uncoded QPSK250 passes on the same cable minutes earlier,
   so the analog path and the rig are fine.
3. **The length-tolerant FECs need soft LLRs**, and `-D` deliberately has none: `qpsk_demodulate_soft`
   errors rather than emit miscalibrated coherent LLRs. So `ldpc` and `soft-concatenated` are not
   available to it.

So the ladder's designated fade rung has no working FEC over real audio today. Closing it means
either a length-tolerant hard-decision FEC, or making the QPSK demodulator emit a frame-exact byte
count on a real audio path. **Not attempted here** — recorded so the next attempt starts from the
measurement rather than re-deriving it.

Note the scope: this says nothing about whether `QPSK250-D` survives a *fade*. Every fading claim for
it remains Watterson-simulator.

## Status (2026-06-19)

Validated on this host (cards `Device`/`Device_1`, USB `07:00.3`/`07:00.4`,
`CAPTURE_GAIN=16`) against current `main`.

**No FEC — full tier** (`--full`, 14 cases): **14/14 PASS** — BPSK31/63/100,
QPSK125/250/500/1000, 8PSK500/1000, OFDM16/52, SCFDMA16/52. (OFDM16/52 and
8PSK500 occasionally take an extra retry in a long sweep but pass reliably.)
- **BPSK31** was fixed by the forward-onset micro-sweep in the receive loop (its
  settled onset lands ~1-2 symbols early on the analog turn-on, outside the
  demod's one-symbol timing search — the slowest rung sits right at the boundary).
- **SCFDMA52** decodes on a real *dual-clock* path, confirming the #392
  per-symbol pilot SFO-deramp holds on hardware. The full-buffer retry it relies
  on is preserved for short-frame modes (the BPSK31 fix only skips that retry for
  the slow, long-frame BPSK rungs that would otherwise starve the read loop).

**No FEC — pilot family** (`--single-case`)
- PASS: PILOT-QPSK500, PILOT-8PSK500, PILOT-16QAM500.
- FAIL: PILOT-32APSK500 — densest pilot constellation, SNR-bound raw; needs FEC.

**Soft-concatenated FEC (RS + K=7 soft Viterbi)**
- PASS: SCFDMA26-8PSK/16QAM/32QAM (narrow + soft FEC, matches the two-Pi rig).
- PASS: SCFDMA52-8PSK, **SCFDMA52-16QAM** (16QAM was only flaky on the two-Pi rig).
- FAIL: 64QAM500 — single-carrier dense QAM stays SNR-bound on the analog cable
  (consistent with prior two-Pi findings; not an SRO/sync issue).
- FAIL (hardware only): the PILOT dense rungs (PILOT-8PSK500/16QAM500) — fail
  **deterministically** (3/3, identical garbage length prefix) on the dual-card
  cable with "LLR slice too short / length prefix exceeds available bits".

  This is **not** a geometry incompatibility (an earlier revision of this doc
  claimed it was — that was wrong). The pilot soft-concatenated path round-trips
  cleanly in sim across *every* condition tested — clean loopback, AWGN to 18 dB,
  pure SRO to 200 ppm, and combined SRO+AWGN (200 ppm / 15 dB) — for all three
  dense rungs incl. PILOT-32APSK500. Regression: `pilot_hom_soft_concatenated_*`
  in `crates/openpulse-modem/tests/fec_timeout_receive.rs`. The hardware-only,
  deterministic failure is an unmodeled dual-clock effect (the two independent
  ALSA 8k↔48k resamplers slip a sample, and the convolutional inner code loses
  resync catastrophically — its length prefix decodes to garbage). **Use LDPC
  for the pilot dense rungs on hardware** (below); it tolerates the slip because
  it extracts a fixed-size block rather than reading a prefix off a continuous
  bitstream.

**LDPC FEC (the recommended pilot soft path on hardware)**
- PASS: PILOT-8PSK500, PILOT-16QAM500, **PILOT-32APSK500** — LDPC drives the
  entire pilot ladder, including the densest rung that fails raw. This is the
  FEC the in-process `plugins/pilot/tests/soft_fec_loopback.rs` validates
  (rate-1/2 LDPC + rate-8/9 high-rate PEG LDPC), now confirmed over real audio.

Net: the dual-card rig reproduces the two-Pi dual-clock results locally and, for
SCFDMA52-16QAM, slightly exceeds them. Remaining hardware failures are 64QAM500
(SNR-bound on the cable) and the pilot dense rungs under soft-concatenated
(dual-clock resampler slip — use LDPC); neither is a static code/geometry bug.
