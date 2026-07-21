# Dual-card hardware loopback — the two-soundcard rung on one host

> **Evidence currency (2026-07-20):** a full 67-mode coded sweep ran at HEAD — **55/67 pass, and every
> rung of the `hpx_hf` ladder decodes on real audio**. It found three defects (a 60 s flush clamp that
> made `BPSK31` untransmittable, fixed-size harness windows, a silently-swallowed SKIP report) and
> falsified this rig's dual-clock premise by measurement. See
> [Full coded sweep](#full-coded-sweep-2026-07-20--5567-and-the-whole-hpx_hf-ladder-passes).
> `QPSK500-D` has since passed both rungs. **JS8 is still unvalidated on real audio and cannot be
> reached by these sweeps at all** — it is not registered in the CLI's plugin registry, so `modes`
> never lists it. Its validation is FF-15 Phase H (on-air).


This rig runs the modem TX→RX through **two USB soundcards plugged into the same
PC**, joined by an analog cable. It is the **hardware (real analog path) rung** of
the loopback ladder (see [virtual-loopback.md](virtual-loopback.md)) made runnable
on a single machine — no SSH, no two Raspberry Pis. It was long described as the
*dual-clock* rung; measurement says otherwise (see the note below the table).

| Rung | Transport | Script | Adds over the rung below |
|---|---|---|---|
| 1 | Virtual (snd-aloop, one clock) | `scripts/run-loopback-virtual.sh` | real cpal+ALSA+resampler path |
| 2a | **Dual-card (two USB cards, one host)** | `scripts/run-loopback-dualcard.sh` | **a real analog cable** (NOT a second clock — measured +0.10 ppm) |
| 2b | Two Pis (two hosts) | `scripts/run-loopback-rpi51-rpi52.sh` | physically separate machines |
| 3 | On-air (real rigs / RF) | `scripts/run-onair-*.sh` | RF, noise, multipath |

Rung 2a adds a real analog cable over the virtual rung.

> **It does NOT add a second clock — measured 2026-07-20 at +0.10 ppm** (`--sro-check`; see
> [Measured: this rig has no meaningful SRO](#measured-2026-07-20--this-rig-has-no-meaningful-sro)).
> This section previously claimed rung 2a "delivers exactly what the single-clock virtual rig cannot:
> two independent sample clocks (sample-rate offset / drift)". That is false: these USB adapters slave
> to the host's USB frame clock, so plugging two into the same host gives you two cards sharing one
> clock. **Any SRO explanation for a failure on this rig must cite a measurement, not the topology.**
> Genuine dual-clock testing needs two hosts (rung 2b) or a deliberately offset clock.

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

## Status (2026-07-20) — MFSK16 and QPSK250-D both validated on real audio

First run after the registry-driven `--full` change (loopback-revalidation-plan task A). Rig: cards
`Device`/`Device_1` (USB `07:00.4-2` / `07:00.3-2`), `CAPTURE_GAIN=16`, TX playback raised 14 → 30
(the default left the captured level at rms 0.033; at 30 it is rms 0.222 / peak 0.353, unclipped).
Binary built with `--features cpal-backend` — without it the CLI silently falls back to the loopback
backend and would report a "hardware" pass that never touched a sound card.

The **pre-fix** run, kept because it is the evidence that located the scanning-receive defect. For
current results see [RESOLVED (2026-07-20)](#resolved-2026-07-20--qpsk250-d-passes-on-real-audio-there-was-never-a-second-defect)
below — every FAIL here except the `ldpc` one is now a PASS.

| Mode | FEC | Result (pre-fix) |
|---|---|---|
| **MFSK16** | `rs` | **PASS** — first validation of `hpx_hf` SL1 on real audio |
| QPSK250 | none | PASS (attempt 2) |
| QPSK250 | `rs` | FAIL — `FEC data length 128 is not a non-zero multiple of 255` |
| **QPSK250-D** | `rs` | **FAIL** — same framing error (len 123/124) |
| **QPSK250-D** | `ldpc` | **FAIL** — `differential QPSK has no soft-LLR path` (still open) |

### Pin the cards by NAME, not index (2026-07-19)

ALSA assigns card indices in enumeration order, and they **shift**. Mid-session on 2026-07-19 the
internal `acp` device moved from index 3 to 4, so a `~/.asoundrc` pinning `hwloop_tx` to `hw:4,0`
silently started pointing at the **laptop's internal audio** instead of the USB adapter. The
`--level-check` caught it (`device not found`), but a run that happened to find *some* device there
would have produced meaningless results while looking healthy.

Use the stable name form in `~/.asoundrc`:

```
pcm.hwloop_tx { type plug; slave.pcm "hw:CARD=Device,DEV=0"   }
pcm.hwloop_rx { type plug; slave.pcm "hw:CARD=Device_1,DEV=0" }
```

`amixer` still needs a numeric index, so `run-loopback-dualcard.sh`'s `_slave_card` now resolves a
`CARD=<name>` slave through `/proc/asound/<name>` to its current index, and still accepts the old
`hw:N,0` form.

**Always run `--level-check` before believing a sweep.** It is the only step that proves the cable,
the levels and the card mapping at once. After a re-index the TX mixer setting also lands on the
wrong card — the level check reported rms 0.033 instead of 0.395, which is what exposed it.

### The FEC scanning receive cannot find a frame inside a long capture (2026-07-19)

**Reproduction, everything else held constant:**

| Capture window | `QPSK250 + rs`, 64 B payload |
|---|---|
| `IRS_LISTEN_MS=7000` (buffer ≈ frame) | **PASS** |
| `IRS_LISTEN_MS=45000` (the default) | **FAIL** |

Same mode, same FEC, same rig, same level, same payload. The only variable is how much audio the
receiver captured around the frame. This is a software defect in
`receive_with_fec_mode_timeout`, not a channel or waveform limitation, and it is why the in-process
suite never sees it: `ChannelSimHarness` hands the receiver a buffer that *is* the frame.

**What was ruled out first**, each by measurement rather than argument:

- **Frame length / airtime.** Uncoded `QPSK250` with a 250 B payload — 260 B wire, **4.16 s**, the same
  airtime as the failing coded frame — decodes **perfectly**. Uncoded means any single bit error fails
  the CRC, so the physical path delivers a 4.2 s frame error-free.
- **Sample-rate offset.** `sro_confirmation::does_sro_alone_break_a_long_coded_qpsk_frame`: the coded
  and uncoded frames tolerate the *same* 500 ppm and fail at the same 1000 ppm. The long frame is not
  more SRO-sensitive. Arithmetic agrees — at 100 ppm the drift across the whole frame is 0.10 symbol
  periods.
- **Signal level.** Raising TX to rms 0.3955 / peak 0.6302 (the documented working point) changed
  nothing.
- **RS correction capacity.** `rs-strong` (t=32, double) fails identically.
- **TX buffer starvation.** The single `snd_pcm_recover: underrun` line appears in the **passing** runs
  too — it is the documented benign end-of-stream one that `flush()` pads for.
- **Physical corruption.** RX audio captured to WAV during both a passing and a failing run: 4.20 s
  continuous burst, stable envelope (rms 0.409, min 0.325, max 0.437), **zero interior dropouts**.
- **Sub-symbol scan granularity.** Quartering the scan step (`symbol_period_samples / 4`) did not fix
  it.

**The mechanism.** Every decode attempt slices a *fixed-length* window —
`end = (start + max_frame_samples).min(accumulated.len())` — so the demodulated byte count is a
function of the **window**, not of the frame. `FecCodec::decode` demands an exact multiple of 255, so
once the capture outlasted the frame, the length gate rejected attempt after attempt before Reed–Solomon
ever ran. The tight window passed only because the slice happened to land near the frame's own length.

**The fix (2026-07-20).** `FecCodec::decode_prefix` tries successively longer block prefixes
(1..=N blocks) and returns the first that decodes. This is safe because `decode` already validates its
own 4-byte length prefix against the decoded size, so a wrong block count cannot silently succeed. Wired
into the `Rs` and `RsStrong` arms of `receive_from_samples_with_fec`; the single-shot
`receive_with_fec_mode` and `decode_combined_llrs` stay strict, and `RsInterleaved` is untouched (it
deinterleaves first and genuinely needs the exact length).

Gate: `crates/openpulse-modem/tests/fec_scan_long_capture.rs`, which embeds a frame in a capture several
times its length via the new `ChannelSimHarness::route_embedded`. Sabotage-verified — reverting the two
arms reproduces the hardware error verbatim (`FEC data length 270 is not a non-zero multiple of 255`).

### RESOLVED (2026-07-20) — QPSK250-D passes on real audio; there was never a second defect

> **CORRECTION #1 (2026-07-19).** The first version of this section said the blocker was "FEC framing" —
> that the demodulator never produced a valid 255-byte block. **Wrong.** The scanning receive *does*
> reach length 255, RS runs there, and fails with `TooManyErrors`. The framing message dominates the log
> only because every *other* scan position produces an invalid length; 255 is absent from it precisely
> because it passes the length check and fails later. I inferred a mechanism from the absence of a log line.
>
> **CORRECTION #2 (2026-07-20).** The second version concluded SL6 had a **separate, differential-specific
> defect** behind the window bug, since `-D` failed even at the tight window where coherent passed, and
> named **sample-rate offset** the best-supported hypothesis for the long-frame failures. **Both were
> wrong.** With the window bug fixed, `QPSK250-D + rs` passes on this rig at the *default* 45 s window,
> 4/4 including a 200 B multi-block frame. There was one defect, not two: the tight window was not a
> control, it was a coin flip, and coherent won it. SRO was independently falsified in
> `sro_confirmation` — coded and uncoded tolerate the same 500 ppm.

Measured on this rig after the fix, TX at rms 0.3963 / peak 0.6304:

| Mode | FEC | Window | Result |
|---|---|---|---|
| **QPSK250-D** | `rs` | 45 s (default) | **PASS** ×3 — first validation of `hpx_hf` SL6 on real audio |
| **QPSK250-D** | `rs`, 200 B payload | 45 s | **PASS** — multi-block |
| QPSK250 | `rs` | 45 s (default) | **PASS** |
| **MFSK16** | `rs` | — | **PASS** — `hpx_hf` SL1 |

Both load-bearing fade rungs of `hpx_hf` — SL1 (`MFSK16`) and SL6 (`QPSK250-D`) — are now validated on
real audio.

`QPSK250-D` + `ldpc` still fails with `differential QPSK has no soft-LLR path` — but that is now
**correct and correctly surfaced**. The refusal is by design (#923); what was wrong was that
`supports_soft_demod()` returned `true` for the whole QPSK plugin, so the mode advertised a capability
it refused at call time. The capability is now per-mode (`supports_soft_demod(&self, mode: &str)`), QPSK
returns `!is_differential(mode)`, and the engine no longer routes `-D` down the soft path. Pair `-D`
with a hard-decision FEC (`rs`); it has no soft path to pair with a soft-input decoder.

**Method note.** Eight hypotheses were falsified before the real one landed, and the two that survived
longest were the two I had reasoned my way to rather than measured: a "second differential defect"
inferred from a single tight-window comparison, and SRO inferred from frame length. Both died to a
direct test. The tight-vs-wide window comparison is what actually located it — *vary one thing about
the receiver, not about the signal.*

`short-rs` is not an escape route: `receive_with_fec_mode_timeout` explicitly rejects it because it is
byte-exact with no length prefix, so a scanning receive cannot guarantee its frame length. It was
briefly exposed on the CLI during this investigation and reverted — a `--fec` value the receive path
refuses is a footgun.

So `QPSK250-D` is boxed in: it **requires** FEC (uncoded differential is 0.00 by design), the padded RS
frame is too long to survive the clock offset, and the length-tolerant FECs need soft LLRs it
deliberately does not provide.

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

## Full coded sweep (2026-07-20) — 55/67, and the whole `hpx_hf` ladder passes

First `--full` sweep at HEAD after the scanning-receive fix (#995), run with `FEC=rs` so every case
exercises the coded path the fix lives on. Report:
`docs/dev/test-reports/loopback-dualcard-full-2026-07-20T061734Z.json`.

**The `hpx_hf` ladder is fully validated on real audio.** All 12 distinct waveforms decode:

| Rung | Mode | Result |
|---|---|---|
| SL1 | `MFSK16` | PASS |
| SL2 | `BPSK31` | PASS *(after the flush fix below; FAIL in the sweep itself)* |
| SL3–SL5 | `BPSK63`, `BPSK100`, `BPSK250` | PASS |
| SL6 | `QPSK250-D` | PASS |
| SL7–SL10 | `OFDM52`, `OFDM52-{8PSK,16QAM,32QAM,64QAM}` | PASS |
| SL11–SL14 | the same waveforms at LDPC r≈8/9 | covered — same waveform as their SC pair |

### Three defects the sweep exposed

**1. A 60 s flush clamp made `BPSK31` untransmittable.** `cpal_backend`'s drain deadline was
`(queued_seconds + 3.0).clamp(5.0, 60.0)`, under a comment reading "Timeout adapts to queued audio
length so slow modes can fully drain". The adaptation was correct; **the upper clamp made it inert for
exactly those slow modes.** RS pads any payload to a full 255-byte block, so a `BPSK31` frame is 65.3 s
of audio: it asked for 68.3 s, got 60 s, and failed every time with `output buffer did not drain within
60.0 s`. The mode could not transmit at all, and it is `hpx_hf` SL2. Extracted to
`openpulse_audio::flush::flush_timeout_seconds` (an **ungated** module, so it is testable under
`--no-default-features`) with the missing invariant as its gate: *the deadline must always exceed the
audio it is waiting on.*

**2. The harness sized every case with one fixed window.** `TX_TIMEOUT=60` / `IRS_LISTEN_MS=45000` are
both shorter than a `BPSK31` frame, so even with the flush fix the sweep would still have reported a
false failure. Windows are now derived per mode from `openpulse modes --airtime`, which reads
`frame_geometry` from the plugin registry — the same registry-driven principle that replaced the frozen
`FULL_CASES` list. An explicit `TX_TIMEOUT=` / `IRS_LISTEN_MS=` still wins.

**3. The SKIP report was a syntax error, so skips were silently dropped.** `${#SKIPPED[@]:-0}` is
invalid bash — `${#arr[@]}` cannot take `:-`. The line errored and the 6 skipped modes vanished from
the summary, directly contradicting the code's own comment ("Reported as SKIP, never silently
dropped"). The array was also declared inside the `full` branch while being read unconditionally.

### Measured (2026-07-20) — this rig has no meaningful SRO

`scripts/run-loopback-dualcard.sh --sro-check` plays a 60 s 1 kHz tone across the cable and measures the
received frequency: **+0.10 ppm**. Repeat runs read +0.01 ppm.

This **falsifies the standing premise for this rig**, which both this document and
[virtual-loopback.md](virtual-loopback.md) asserted from the topology: two USB cards were assumed to
mean two independent clocks. They do not — these adapters slave to the host's USB frame clock, so both
cards on one host share it. The 2026-06-13 conclusion that the `SCFDMA52-*` / `64QAM` hardware failures
are "the two independent soundcard clocks (sample-rate offset)" **cannot be right on this rig**, and
those failures are now unexplained.

The estimator has a self-test (`python3 scripts/lib/sro_estimator.py`) and `--sro-check` refuses to
report a reading if it fails. That is not decoration: the **first** version of the estimator wrapped
phase above ~5 ppm and reported an injected 200 ppm as −6.9 ppm, i.e. it would have called a badly
offset rig "clean". A measurement device that cannot detect a known offset cannot certify an unknown one.

### The 12 sweep failures

| Group | Modes | Status |
|---|---|---|
| Slow-rung harness/flush defect | `BPSK31` | **Fixed** — now PASSes |
| Software defect | `8PSK2000` | **Real bug** — fails at **0 ppm in-process**, on a clean channel. Not in any shipped profile (manual-select only). Its `-RRC` sibling passes. |
| Hardware-only, unexplained | `BPSK250-RRC`, `PILOT-QPSK500` | Pass in-process through 400–800 ppm of injected SRO, and the rig has 0.1 ppm — so **SRO does not explain them**. |
| Previously attributed to dual-clock SRO | `64QAM{500,1000,2000-RRC}`, `SCFDMA52-{16QAM,32QAM,64QAM,64QAM-P4,LP}` | **Attribution withdrawn** — see the SRO measurement above. Also measured at `rs` (hard-decision), while these modes are designed around soft FEC (~+6 dB), so read them as *not disproven* rather than *failed*. |

Note the pulse-variant split, which is suggestive but has one data point per mode and no ablation behind
it — recorded as an observation, **not** a mechanism: `8PSK2000` FAIL / `8PSK2000-RRC` PASS,
`PILOT-QPSK500` FAIL / `PILOT-QPSK500-RRC` PASS, but `BPSK250` PASS / `BPSK250-RRC` FAIL, which inverts.

## Analog-path characterisation (2026-07-20) — four mechanisms eliminated, none explains the failures

The virtual×hardware comparison localised `64QAM{500,1000,2000-RRC}`,
`SCFDMA52-{16QAM,32QAM,64QAM,64QAM-P4}` and `PILOT-QPSK500` to the **analog path** (they pass the
virtual rung, which shares all software and has no cable). This section measures that path. Every
standard analog impairment came back clean:

| Property | Measured | Verdict |
|---|---|---|
| Magnitude response, 306–3388 Hz | flat within **±0.21 dB** | not the mechanism |
| Group delay, 250–3400 Hz | **~1.04 ms** spread, no systematic slope — mostly measurement jitter | inside SC-FDMA's cyclic prefix; not the mechanism |
| SNR (1 kHz tone vs idle floor) | **71.1 dB** | 64QAM needs ~25–30 dB; not the mechanism |
| PAPR / clipping at the working level | PAPR 3.4–6.1 dB, peak ≤ 0.78 FS, **0 clipped samples** on all of `BPSK250`, `OFDM52`, `SCFDMA52-64QAM`, `64QAM1000` | not the mechanism |

**So the attribution is a localisation, not an explanation.** "Analog path" is where the failure lives;
*what* about the analog path is still unknown, and the four obvious candidates are now ruled out by
measurement rather than argument. Do not "fix" filtering, levels, or sample-rate offset on the strength
of this — all three are measured clean.

### Update (2026-07-20, later) — AGC and nonlinearity eliminated too; the AUDIO is what is bad

Continued after the section below was written. Three further results, and one invalid measurement:

- **The AGC hypothesis is dead.** `amixer -c <rx> sget 'Auto Gain Control'` reports the control
  **already off**. No re-run needed.
- **Nonlinearity is eliminated.** A two-tone test (1200 + 1700 Hz at the modem's working peak, ~0.63 FS)
  measures **IMD3 at −60…−62 dBc** and IMD5 at −80 dBc. A pure-tone SNR cannot see intermodulation;
  this can, and there is none worth the name.
- **Short-term timing wander is eliminated.** The +0.10 ppm SRO figure is a 58 s average and would hide
  wander. Removing the constant slope leaves rms **0.115** modem samples, peak 0.48, and at most
  **0.72 samples of drift within a 4 s frame** — far too little to matter.
- **The failures still reproduce at HEAD**, after #997–#1001: `SCFDMA52-16QAM`, `SCFDMA52-64QAM`,
  `64QAM500`, `64QAM1000` and `PILOT-QPSK500` all still FAIL. This was worth checking — the original
  measurements predated five fixes.

**The decisive split: it is the audio content, not the live streaming path.** Capturing a frame to a
WAV at 8 kHz and decoding it **offline**, through the same engine, reproduces the failure:

| mode | captured audio decoded offline |
|---|---|
| `BPSK250` | **decodes** — proves the capture/replay method is sound |
| `64QAM1000` | fails (`RS correction failed at block 0`) |
| `SCFDMA52-16QAM` | fails |

So the signal coming off the cable is genuinely damaged, and cpal/ALSA streaming, buffer scheduling and
capture timing are all ruled out. That contradicts every channel-level measurement above, which means
the impairment is **signal-dependent in a way none of the probe signals (tone, two-tone, chirp)
reproduce** — the remaining suspects are wideband/high-PAPR-specific effects, not the flat-channel
properties measured so far.

**An invalid measurement, recorded so it is not repeated.** An attempt to quantify this as an
end-to-end waveform SNR against the ideal transmitted samples produced **5.3 dB for `BPSK250` — a
signal that decodes perfectly**. The metric was a time-domain cross-correlation with neither
fractional-sample nor carrier-phase alignment, so it measured its own alignment error. A valid version
must compare **recovered symbols** (post-demod, post-carrier-recovery EVM), not raw passband samples.

**The earlier leading candidate was the capture-side AGC** — these adapters have one, and this document
records it drifting capture gain after strong frames, which would be near-harmless to the phase-only
modes that pass and destructive to the amplitude-carrying modes that fail. It fit the failure set
better than anything else. **It is now eliminated: the control is already off.**

## RESOLVED (2026-07-20) — most of it was the FEC operating point; 64QAM is untracked slow wander

A second-opinion review (Fable) attacked the eliminations above and found two of them unsound. Three
follow-up experiments then collapsed the failure set from eight modes to three.

### The sweep measured modes at an FEC they are not designed for

Re-run on the rig with the FEC each mode actually uses:

| Mode | `rs` (the sweep) | `soft-concatenated` |
|---|---|---|
| `SCFDMA52-16QAM` | FAIL | **PASS** |
| `SCFDMA52-32QAM` | FAIL | **PASS** |
| `64QAM500` | FAIL | **PASS** (attempt 3) |
| `64QAM1000` | FAIL | **PASS** (attempt 2) |
| `64QAM2000-RRC` | FAIL | **PASS** (attempt 2) |
| `SCFDMA52-64QAM` | FAIL | FAIL |
| `SCFDMA52-64QAM-P4` | FAIL | FAIL |
| `PILOT-QPSK500` | FAIL | FAIL |

`ldpc` gives the same answer as `soft-concatenated` for the SCFDMA pair. **Five of the eight "analog-path
limited" modes were never analog-path limited** — they were measured at a hard-decision operating point
roughly 6 dB below what they are built for. This document already warned about that in the sweep table
("read them as *not disproven* rather than *failed*"), and the June 2026 record already had
`SCFDMA52-16QAM` passing this rig. The warning was written and then not acted on.

### The 64QAM mechanism: untracked slow clock wander

The elimination of timing wander above was **wrong, and wrong in an instructive way**. 0.72 samples was
judged against a *symbol period* and dismissed. At a 1500 Hz carrier, 0.48 samples is **32° of carrier
phase**, and the wander is concentrated at **0.1–2 Hz** — precisely where 64QAM's decision-directed loop
(natural frequency ≈0.4 Hz at `loop_bw = 0.01`) cannot follow it.

`plugins/64qam` is the **only receiver in the fleet with no mid-frame reference update**: a single scalar
AGC from the 16-symbol preamble, absolute PAM-8 thresholds, preamble-only phase whose drift fit is gated
on `afc_correction_hz >= 0.5` (never fires on a 0.1 ppm rig — a guard that cannot fire), and fixed-stride
sampling with no timing loop. Every mode that passes tracks its reference: PILOT re-estimates complex
gain every 16th symbol, OFDM/SCFDMA re-estimate per symbol from pilots.

Reproduced in-process, noiselessly (byte errors; RS corrects ≤16; drift = A·sin(2π·0.3t) samples):

| drift A | 0.05 | 0.1 | 0.2 | 0.35 | 0.48 |
|---|---|---|---|---|---|
| `64QAM500` | 0 | 9 | 49 | 125 | 180 |
| `64QAM1000` | 73 | 97 | 122 | 151 | 183 |
| every OFDM / SCFDMA / PILOT mode | 0 | 0 | 0 | 0 | 0 |

The rig measures rms 0.115 / peak 0.48 — straddling the breaking point. Attribution is clean: with pure
sinc interpolation (drift, no resampler comb) `64QAM500` still takes 99 errors, so **the wander itself is
the cause**, not the resampler.

So the earlier "amplitude-carrying modes fail, phase-only pass" framing was the right observation on the
wrong axis. The axis is **frame-static reference vs tracked reference**.

**The fix is not one constant.** Sweeping the DD loop bandwidth: `64QAM500` improves 125 → 15 errors at
`loop_bw = 0.06` (under the RS threshold) and degrades again by 0.12, but `64QAM1000` shows **61 errors
in the static case alone**, so it needs timing interpolation as well as faster carrier tracking. Not
shipped: these modes pass with their intended FEC, and a speculative change to a shipped demodulator
needs its own evidence.

### A correction: the virtual rung does not exercise the resampler

Verified on this host: `hw:Loopback` reports `RATE: [8000 768000]`, so the virtual rung's `plug` is a
**pass-through at 8 kHz**, while the C-Media cards report `RATE: [44100 48000]` and therefore always
resample 8k↔48k in both directions. The rung table in
[virtual-loopback.md](virtual-loopback.md) claims the virtual rung "adds the real cpal+ALSA+resampler
path"; the resampler half of that is false. Consequently "analog path" as used above really means
*analog cable + double linear resample + inter-card wander*.

## `PILOT-QPSK500` RESOLVED (2026-07-21) — the retry starves on COST, not frame length

The odd one out of its family: `PILOT-8PSK500`, `PILOT-16QAM500` and `PILOT-32APSK500` all pass, and
only the **least dense** mode failed — an inversion that says software, not channel. It was.

Evidence, in order:

| test | result |
|---|---|
| in-process, clean and embedded, every FEC | **passes** |
| hardware audio captured to WAV, decoded **offline** | **decodes** |
| live on the rig | **fails 3/3**, while `PILOT-QPSK500-RRC` passes 3/3 |

So the audio is fine and the DSP is fine; the failure is in the live path. The live RX log shows the
demodulator returning **0 bytes** at each of 1028 scan positions, having reached only ~10.9 s of buffer.

**The arithmetic closes it.** The full-buffer retry is O(buffer) and `PILOT-QPSK500` costs ~640 ms per
decode attempt — 1028 positions is **~11 minutes of CPU for a 45 s listen**. The scan can never reach
the frame before the process is killed. `PILOT-QPSK500-RRC` costs *more* per attempt (~1150 ms) but
acquires early and stops, so it never grinds.

**Frame length is the wrong variable.** The retry was gated by `long_frame`, a geometry proxy.
`PILOT-QPSK500` is 55 200 coded samples (classified "short") and starves; `QPSK250` is 112 800 —
**twice as long** — and passes comfortably. Confirmed by ablation: forcing the retry off makes
`PILOT-QPSK500` pass while `SCFDMA52`/`OFDM52` (which depend on the retry for acquisition) still pass.

**Fix: budget the pass by the audio it covers, enforced from inside.** A scan that cannot walk its own
buffer in less than real time can never catch up, because the buffer keeps growing. The first attempt
measured the pass *after* it completed and was **inert** — the pathological pass never completes at all;
it has to be abandoned while running. Verified on the rig: `PILOT-QPSK500` PASS, and
`PILOT-QPSK500-RRC`, `SCFDMA52`, `OFDM52`, `SCFDMA52-8PSK`, `BPSK250`, `QPSK250-D`, `MFSK16`, `BPSK31`
all still PASS. Gate: `crates/openpulse-modem/tests/retry_budget.rs`.

### Still genuinely unexplained (2 modes, down from 8)

`SCFDMA52-64QAM` and `SCFDMA52-64QAM-P4` fail with every FEC tried. `SCFDMA52-16QAM`
was additionally decoded from a hardware capture at **plugin level** (bypassing the engine entirely,
with the probe validated against a coded control frame that decodes at the same offset) and still
failed — so an engine-path/AFC explanation is ruled out for that group too.

### Where this stands

Eight mechanisms measured, all clean: magnitude, group delay, SNR, clipping/PAPR, AGC, IMD3/IMD5,
timing wander, and the live-streaming path. The failure is real, reproduces at HEAD, and lives in the
captured audio. **No mechanism has been identified.** The next step is a *valid* EVM measurement on
recovered symbols — not on raw passband samples — since that is the metric these modes actually fail
on and the only one that will show a signal-dependent impairment the probe tones cannot.

### Method note

Two of the measurements in this section were wrong before they were right, in the same way both times:

- The **first SRO estimator** wrapped phase above ~5 ppm and reported an injected 200 ppm as −6.9 ppm.
  It would have certified a badly offset rig as clean.
- The **first PAPR capture** returned identical rms, peak and PAPR to four decimal places for four
  completely different waveforms — because a stray `aplay` from the SNR test was still running and every
  capture was recording that 1 kHz tone. An occupied-bandwidth check (0 Hz wide, exactly 1 kHz)
  confirmed it instantly.

Both were caught by the result looking *too clean or too uniform*, not by the tooling. When measuring a
physical path, check the instrument against a known input and check the capture is of the thing you
think it is — a spectrum is cheap and unambiguous.
