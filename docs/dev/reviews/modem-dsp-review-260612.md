---
title: "Modulation / Demodulation Deep Review"
date: 2026-06-12
scope: "plugins/* (bpsk, qpsk, psk8, 64qam, fsk4, ofdm, scfdma), crates/openpulse-dsp, ModemEngine receive/AFC path"
reviewer: "Claude Opus 4.8 (requested by DC0SK)"
---

# Modulation / Demodulation Deep Review — 2026-06-12

Reviewed: all plugin modulate/demodulate paths (~9 k lines), `openpulse-dsp` primitives
(Gardner, Costas PLL, preamble, LMS), and the engine's frame-seeking + AFC machinery
(`receive_with_timeout`, `update_afc_estimate`, `receive_from_samples`).

Verdict up front: the per-mode signal processing is individually serviceable — every mode
that can pass clean/AWGN loopback does — but the codebase has **no shared acquisition
layer**, so the same conceptual bugs have been fixed serially per plugin (carrier-phase-
sensitive correlation was fixed three separate times: QPSK, OFDM #385, SCFDMA #386, and
**still exists in BPSK and in the shared `PreambleDetector`**). The timing-recovery loop
that is supposed to absorb sample-clock offset is provably a no-op. The engine's frame
seeker works but is a 400-line accretion of hardware-specific patches with mode-geometry
assumptions that are wrong for half the registered modes.

---

## 1. Conceptual challenges

### C1. The Gardner timing loop is decorative — no timing recovery actually happens

[timing.rs:55-73](crates/openpulse-dsp/src/timing.rs#L55-L73): the strobe interval is
`round(sps + mu)` and `mu` is clamped to ±0.49, so the strobe is **always exactly `sps`**
— the comment even says the clamp exists so the interval "never changes from sps". There
is no fractional interpolator, so the accumulated `mu` adjusts nothing. Every plugin's
"adaptive timing recovery via Gardner detector" comment
([bpsk demodulate.rs:540-545](plugins/bpsk/src/demodulate.rs#L540-L545),
[qpsk demodulate.rs:160-163](plugins/qpsk/src/demodulate.rs#L160-L163)) is false: the
demodulator samples at a fixed stride from the brute-force preamble offset.

Why this matters: the primary deployment is **two free-running CM108 sample clocks**. A
30 ppm clock offset at 8 kHz drifts one full sample every ~4.2 s. Short frames survive on
the initial preamble lock; long frames (full 255-byte RS blocks at low baud) will slip a
symbol mid-frame and there is no mechanism anywhere in the codebase to catch it. This is
also a plausible contributor to the 64QAM and wide-SCFDMA hardware failures attributed
purely to SNR — 64QAM at 500 baud needs sub-sample timing for the 8×8 grid.

Fix: a real interpolating timing loop (Farrow cubic interpolator, `mu` wraps with
base-index carry, complex Gardner error). This is the single highest-leverage DSP change
available.

### C2. Gardner is fed I-only in every plugin

[qpsk demodulate.rs:176-177](plugins/qpsk/src/demodulate.rs#L176-L177),
[64qam demodulate.rs:280](plugins/64qam/src/demodulate.rs#L280),
[psk8 demodulate.rs:395](plugins/psk8/src/demodulate.rs#L395): `det.update(s_i)` — the Q
channel never reaches the TED. For QPSK/8PSK/64QAM the correct Gardner error is the
complex form `Re{z_mid · (z_next − z_prev)*}`. An I-only error is data-dependent: symbol
runs whose transitions live mostly in Q contribute zero (or wrong-signed) timing error.
Today this is masked by C1 (the error is discarded anyway), but any fix for C1 must also
fix this or the loop will be noisy/unstable for the quadrature modes.

### C3. The shared preamble module is dead code that embodies the known-broken pattern

`openpulse_dsp::preamble` (Barker/PN/Zadoff-Chu, `PreambleDetector`) is used by **zero
production code** — only two test files reference it. Meanwhile each plugin hand-rolls
its own preamble and correlator. Worse:

- [preamble.rs:214-241](crates/openpulse-dsp/src/preamble.rs#L214-L241)
  `correlate_bpsk` is a bare real correlation `Σ r·ref` — exactly the carrier-phase-
  sensitive metric whose removal took three PRs (#385, #386, QPSK timing fix). If anyone
  ever wires this in, the bug returns.
- [preamble.rs:174-185](crates/openpulse-dsp/src/preamble.rs#L174-L185): `ZadoffChu64`
  keeps only `cos(·)` ("real projection"), which destroys the CAZAC property that is the
  entire reason to use Zadoff-Chu. As offered, it is a worse Barker.

Either make this module the real shared acquisition layer (phase-insensitive I/Q matched
filter, the one fixed pattern) and migrate the plugins onto it, or delete it. The current
state — a tempting, broken, unused API — is the worst option.

### C4. Five independent AFC estimators with wildly different acquisition ranges

| Mode | Estimator | Acquisition range |
|---|---|---|
| BPSK | 2-stage: Goertzel on squared signal + IQ-squaring fine | **±400 Hz** |
| QPSK | 4th-power Mth estimator only | ±baud/8 (QPSK125: ±15.6 Hz) |
| 8PSK | data-aided preamble conjugate | ±baud/2 |
| 64QAM | 4th-power Mth (m=4) | ±baud/8, high self-noise (see P7) |
| SCFDMA | pilot-spectra CFO | mode-specific |
| OFDM, FSK4 | none (trait default `None`) | — |

The engine applies them uniformly ([engine.rs:2823-2845](crates/openpulse-modem/src/engine.rs#L2823-L2845)).
Consequence: an adaptive session that rate-climbs from BPSK to QPSK silently loses 25× of
AFC acquisition range mid-session. The wide Goertzel coarse stage is mode-agnostic in
principle (it squares the signal and finds 2·fc) — it belongs in `openpulse-dsp` or the
engine, run once per acquisition, with the per-mode estimator only doing fine residual.

### C5. Batch re-scan instead of streaming acquisition

`receive_with_timeout` ([engine.rs:637-1033](crates/openpulse-modem/src/engine.rs#L637-L1033))
compensates for the absence of a streaming acquisition state machine by re-slicing and
re-decoding an ever-growing buffer with wall-clock retry ticks (T≥12 s, every 2 s),
full-buffer rescans, and per-position 6-pass AFC mini-settles. Each plugin then re-runs
its own brute-force preamble search per attempt. This works, but the cost is the function
itself: ~400 lines, four interacting trigger conditions, manual save/restore of mutable
AFC state in four places, and comments that mostly document previous regressions. A
`SignalAcquirer { Idle → EnergyDetected → AfcSettled → PreambleLocked }` state machine
consuming chunks incrementally would delete most of this and be unit-testable without an
audio backend. Acknowledged as a big-ticket refactor; the action list orders cheaper
robustness wins first.

---

## 2. Correctness and consistency findings

### Engine / frame seeking

- **E1 (correctness).** [engine.rs:657-672](crates/openpulse-modem/src/engine.rs#L657-L672)
  derives the scan step by parsing **trailing digits of the mode name** as baud:
  - `"OFDM52"` → 52 "baud" → step 154 samples (52 is the subcarrier count, not baud);
  - `"SCFDMA52-64QAM-P4"` → **4** → step 2000 samples — the scan strides 2 000 samples
    and `min_frame_samples = 66 000`, which can step entirely over a frame;
  - `"SCFDMA52-16QAM"`, `"FSK4-ACK"` → no trailing digit → silent 32 fallback.
  The mode-string heuristic is wrong for half the registry. The plugin must own its
  geometry (see A2).
- **E2 (correctness).** [engine.rs:675](crates/openpulse-modem/src/engine.rs#L675)
  `min_frame_samples = step * 33` and the settle/gate windows of `step * 32` assume
  `PREAMBLE_SYMS = 32` — true only for BPSK. QPSK/8PSK/64QAM use 16
  ([qpsk modulate.rs:10](plugins/qpsk/src/modulate.rs#L10)); OFDM/SCFDMA have a totally
  different frame anatomy (Schmidl-Cox symbol / 4 sync symbols).
- **E3 (consistency).** [engine.rs:2854-2858](crates/openpulse-modem/src/engine.rs#L2854-L2858)
  (`stage_demodulate_payload`) and [engine.rs:2824-2828](crates/openpulse-modem/src/engine.rs#L2824-L2828)
  (`update_afc_estimate`) build `ModulationConfig` **without setting `afc_correction_hz`**
  (defaults 0), while `receive_from_samples` ([engine.rs:1059-1063](crates/openpulse-modem/src/engine.rs#L1059-L1063))
  sets it. QPSK/8PSK gate their linear drift correction on `afc_correction_hz ≥ 0.5 Hz`
  ([qpsk demodulate.rs:572](plugins/qpsk/src/demodulate.rs#L572)) — so on the hard-decision
  path the drift corrector silently never engages even when AFC has settled.
- **E4 (robustness).** `ENERGY_GATE_THRESHOLD = 0.0001` is a fixed constant. The code's
  own comments record an on-air noise floor of ~0.0015 mean-square — above the gate, so
  on-air every scan position passes the gate and fires the expensive mini-settle. The
  gate should be relative to a rolling noise-floor estimate (`DcdState` already tracks
  energy; this logic is duplicated rather than shared).
- **E5 (fragility).** AFC state (`afc_correction_hz`, `afc_step`) is temporarily mutated
  and manually restored in four code paths inside `receive_with_timeout`. The comment at
  [engine.rs:998-1001](crates/openpulse-modem/src/engine.rs#L998-L1001) documents the
  bug class this already caused (>1000 Hz accumulated drift). A scoped guard (RAII) or
  passing AFC as an explicit argument to the demod call would eliminate the class.
- **E6 (consistency).** `receive_from_samples` calls `demodulate_soft` and treats *any*
  error as "plugin doesn't support soft" → silently retries hard
  ([engine.rs:1067-1086](crates/openpulse-modem/src/engine.rs#L1067-L1086)). A genuine
  soft-path failure (short signal) is thus demodulated twice per attempt; and
  `supports_soft_demod()` exists but is never consulted here.
- **E7 (hygiene).** Wall-clock magic numbers (retry at T≥12 s, 2 s tick) are calibrated
  to one specific rig (FT-991A PipeWire timing) and live as inline literals. Indentation
  around [engine.rs:744-746](crates/openpulse-modem/src/engine.rs#L744-L746) and the
  stray block at line 788 are evidence of patch-on-patch; the function needs extraction
  before the next on-air campaign adds another trigger condition.

### BPSK

- **B1 (robustness — the un-ported phase fix).** BPSK timing search is **I-channel-only**
  in both paths, and the RRC path doesn't even take `abs()`:
  [demodulate.rs:572-590](plugins/bpsk/src/demodulate.rs#L572-L590)
  (`find_timing_offset_bb`, signed I-only score) and
  [demodulate.rs:594-628](plugins/bpsk/src/demodulate.rs#L594-L628) (`|Σ I·e|`).
  At a ~90° carrier phase, the preamble energy lives in Q and both metrics collapse —
  the exact failure mode fixed in QPSK/8PSK/SCFDMA/OFDM with the I/Q-magnitude metric.
  BPSK passes hardware loopback because differential decoding forgives a wrong-but-
  consistent timing pick more often, and because the engine retries many offsets — but
  it's the same latent bug, and likely a contributor to the BPSK100 flakiness.
- **B2 (performance).** `find_timing_offset` calls `demodulate_iq` on the **entire
  slice** per candidate offset (up to n=64 offsets × full multi-second buffer)
  ([demodulate.rs:594-628](plugins/bpsk/src/demodulate.rs#L594-L628)); only the preamble
  span is needed. Same pattern in QPSK
  ([demodulate.rs:214-250](plugins/qpsk/src/demodulate.rs#L214-L250)) and 64QAM. On a
  Pi this is the dominant per-attempt cost the engine comments measure at ~90 ms.
- **B3 (doc rot).** `estimate_carrier_hz_wide` doc says "2×fc ± 600 Hz (= fc ± 300 Hz)"
  and "Acquisition range: ±300 Hz"; the code uses `range_hz = 800` (±400 Hz baseband)
  ([demodulate.rs:151-170](plugins/bpsk/src/demodulate.rs#L151-L170)). Also
  `afc_estimate_hz` stage-1 comment says ±300. Stale after the #380-era widening.

### QPSK / 8PSK

- **Q1 (consistency).** The non-RRC path applies `carrier_phase_correct` (preamble LS
  fit) **then** a Costas PLL; the RRC path applies **neither** in QPSK (relies on LMS
  + PLL inside `gardner_pll_sample_rrc`) but **does** apply `carrier_phase_correct`
  after the PLL in 8PSK ([psk8 demodulate.rs:125-135](plugins/psk8/src/demodulate.rs#L125-L135)).
  Three different recovery stacks for the same problem across two near-identical
  plugins. Pick one canonical ordering (acquire phase from preamble → PLL for drift →
  LMS for ISI) and apply it uniformly.
- **Q2 (concept).** The 0.5 Hz `afc_correction_hz` drift gate
  ([qpsk demodulate.rs:572](plugins/qpsk/src/demodulate.rs#L572)) means that in the
  standard hardware loopback configuration (`--no-afc`) the linear drift corrector is
  permanently off and the PLL carries everything. That's defensible — but then the gate
  isn't doing what its comment claims on the path where AFC *is* enabled, because of E3.
- **Q3 (duplication).** `estimate_frequency_offset_mth` is duplicated **verbatim** in
  [qpsk/demodulate.rs:17-45](plugins/qpsk/src/demodulate.rs#L17-L45) and
  [64qam/demodulate.rs:20-48](plugins/64qam/src/demodulate.rs#L20-L48).

### 64QAM

- **P7 (concept).** 4th-power CFO estimation on a **non-constant-modulus** constellation
  has high data-dependent self-noise; it is the weakest possible choice for the mode
  that most needs accurate carrier recovery. The 64QAM preamble symbols are known —
  the data-aided conjugate estimator 8PSK already uses
  ([psk8 demodulate.rs:23-53](plugins/psk8/src/demodulate.rs#L23-L53)) would be strictly
  better here, for free.
- **P8 — WITHDRAWN (2026-06-12).** The rectangular window is deliberate and the
  divergence IS documented — in the modulator ([64qam modulate.rs:181-186](plugins/64qam/src/modulate.rs#L181-L186)):
  the non-RRC TX path emits rectangular-windowed symbols because a Hann
  crossfade would blur 8 amplitude levels per axis across symbol boundaries, so
  rectangular RX integration is the matched filter. Fixed by cross-referencing
  the rationale from the demodulator instead.
- **P9 (minor).** `dd_carrier_track` uses `β = bw²·0.25`
  ([64qam demodulate.rs:133-137](plugins/64qam/src/demodulate.rs#L133-L137)) while
  `CarrierPll` uses `β = bw²` — two second-order loop conventions in one codebase, with
  no comment explaining why the QAM loop is 4× less stiff.

### OFDM / SCFDMA

- **O1 (inconsistency — detection gate).** OFDM acquisition rejects noise via the
  Schmidl-Cox metric floor `best_m < 0.5`
  ([ofdm demodulate.rs:92](plugins/ofdm/src/demodulate.rs#L92)). SCFDMA's
  `find_sync_offset` has **no threshold at all**
  ([scfdma demodulate.rs:308-339](plugins/scfdma/src/demodulate.rs#L308-L339)) — on
  noise it returns the best-scoring arbitrary offset and the demodulator decodes
  garbage, including interpreting random bits as the length prefix. Add a normalized
  correlation floor (the OFDM stage-2 ρ is already normalized; SCFDMA's score is not).
- **O2 (doc/code mismatch).** [ofdm demodulate.rs:141-142](plugins/ofdm/src/demodulate.rs#L141-L142):
  comment says leading-path threshold is "half the peak"; code uses `best_rho * 0.20`.
  One of them is wrong — and 0.20 was tuned on hardware, so fix the comment.
- **O3 (robustness).** Both OFDM and SCFDMA carry a bare 2-byte LE length prefix with no
  integrity check ([ofdm demodulate.rs:227-233](plugins/ofdm/src/demodulate.rs#L227-L233),
  [scfdma demodulate.rs:137-144](plugins/scfdma/src/demodulate.rs#L137-L144)); the soft
  path even hard-decides the prefix from the first 16 LLRs and then trusts it to trim
  the LLR stream. A single bit error in those 16 bits silently truncates or inflates
  the frame *before* FEC sees it. Protect the prefix (repeat it 3× and majority-vote,
  or CRC-4 in a third byte) or derive length from the outer `Frame` envelope.
- **O4 (consistency).** `ofdm_demodulate`/`scfdma_demodulate` return `Vec<u8>` (empty on
  failure) instead of `Result<_, ModemError>` like every PSK plugin — acquisition
  failure, short-buffer, and "decoded zero bytes" are indistinguishable to the engine,
  which then can't log *why* a window failed.
- **O5 (duplication).** `quadrature()` (FFT Hilbert) is duplicated verbatim in
  [ofdm/demodulate.rs:156-179](plugins/ofdm/src/demodulate.rs#L156-L179) and
  [scfdma/demodulate.rs:343-366](plugins/scfdma/src/demodulate.rs#L343-L366); it belongs
  in `openpulse-dsp` next to the phase-insensitive matched filter (A1).
- **O6 (surprise).** `ScFdmaPlugin::estimate_afc_hz` **mutates adaptive state** (updates
  the coherence-bandwidth tracker through a mutex) as a side effect of what every other
  plugin treats as a pure estimate ([scfdma lib.rs:221-231](plugins/scfdma/src/lib.rs#L221-L231)).
  The engine calls this in settle loops 6× per candidate position — the adaptation state
  gets polluted by noise windows that are subsequently rejected.
- **O7 (limitation).** SCFDMA hard-rejects any `center_frequency ≠ 1500 Hz`
  ([scfdma lib.rs:200-205](plugins/scfdma/src/lib.rs#L200-L205)), which silently makes it
  incompatible with engine AFC (which works by shifting `center_frequency`). If AFC ever
  settles nonzero while an SCFDMA mode is active, every demodulate call errors out.

### DSP crate

- **D1.** `CarrierPll` discriminant doc/code mismatch: doc says `e = I·sign(Q) −
  Q·sign(I)`, code computes `q·sign(i) − i·sign(q)`
  ([pll.rs:11 vs 88](crates/openpulse-dsp/src/pll.rs#L88)). The loop converges (tests
  prove polarity is consistent end-to-end), but the doc will mislead the next person
  who ports it.
- **D2.** `psk_order` parameter accepts 1/2/3 meaning *bits per symbol*, named "order",
  and silently returns 0 error for any other value ([pll.rs:95](crates/openpulse-dsp/src/pll.rs#L95))
  — a no-op PLL rather than a loud failure if someone passes 4.
- **D3.** `PreambleSpec::iq_symbols` QPSK mapping indexes `chips[(2k) % len]` — with the
  odd-length Barker bases the I/Q chip pairing drifts parity each wrap, producing a
  sequence with no designed correlation property. Dead code today (C3), but broken as
  specified.

---

## 3. AFC + frame seeking: how to make them more robust

In priority order, cheapest-first:

1. **Make the plugin describe its own frame geometry** (new trait method, e.g.
   `fn frame_geometry(&self, mode, sample_rate) -> FrameGeometry { symbol_period,
   preamble_samples, min_frame_samples, max_frame_samples }`). Kills E1/E2 — the engine
   stops parsing baud out of mode-name suffixes and stops assuming a 32-symbol preamble.
   Every magic `step * 32` / `step * 33` / `step * 2280` becomes a derived value.
2. **One phase-insensitive matched-filter primitive in `openpulse-dsp`**
   (`fn correlate_iq(samples, template, template_q, bound) -> (offset, rho_normalized)`),
   used by BPSK (fixes B1), QPSK, 8PSK, 64QAM, SCFDMA, OFDM stage-2, and the revived
   `PreambleDetector`. One implementation, one test suite, one place where the
   carrier-90° regression test lives. Includes the shared `quadrature()` (O5).
3. **Hoist the coarse Goertzel carrier scan out of the BPSK plugin** into the engine
   settle phase (it operates on the squared passband signal; only the modulation-removal
   exponent differs per family). Every mode then gets ±400 Hz acquisition; per-mode
   estimators only refine. Fixes the C4 range cliff.
4. **Add a detection threshold to SCFDMA sync and normalize its score** (O1) — without
   it, the engine's retry loop burns CPU decoding noise at full frame cost.
5. **Set `afc_correction_hz` in every `ModulationConfig` the engine builds** (E3) — a
   3-line fix that turns on the drift corrector for the hard-decode path.
6. **Adaptive energy gate**: replace the fixed `1e-4` with `max(1e-4, k × noise_floor)`
   where the noise floor is the trailing median of gate-window energies (or reuse
   `DcdState`). Fixes E4 and makes the on-air scan cost collapse back to near-loopback
   levels.
7. **RAII guard for AFC state** (`struct AfcScope<'a>` restoring on drop) (E5).
8. **Real timing recovery** (C1+C2): Farrow interpolator + complex Gardner error +
   mu-wrap symbol-slip handling in `openpulse-dsp`, wired into the RRC paths first
   (they already have baseband I/Q). Acceptance: a synthetic ±50 ppm resampled loopback
   test decodes a full 255-byte frame — a test that **cannot pass today**.
9. **Streaming acquisition state machine** (C5): long-term replacement for
   `receive_with_timeout`'s rescan heuristics. Do after 1–8; its design falls out of the
   `FrameGeometry` + shared correlator work.

---

## 4. Action list

| # | Action | Fixes | Effort | Priority |
|---|---|---|---|---|
| A1 | Shared phase-insensitive I/Q matched filter + `quadrature()` in `openpulse-dsp`; migrate SCFDMA/OFDM/BPSK/QPSK/8PSK/64QAM searches onto it | B1, O1, O5, C3 | M | **P1** |
| A2 | `ModulationPlugin::frame_geometry()`; engine derives step/min/max/settle windows from it | E1, E2 | M | **P1** |
| A3 | Set `afc_correction_hz` in `stage_demodulate_payload` + `update_afc_estimate` configs | E3 | XS | **P1** |
| A4 | SCFDMA sync: normalized score + detection floor (mirror OFDM `best_m < 0.5` gate) | O1 | S | **P1** |
| A5 | Fix BPSK timing metrics to I/Q magnitude (port the QPSK fix; add carrier-90° regression tests for BPSK like qpsk's) | B1 | S | **P1** |
| A6 | Bound all brute-force timing searches to the preamble span instead of whole-slice demod per offset | B2 | S | **P2** |
| A7 | Adaptive energy gate from rolling noise floor (share with/reuse `DcdState`) | E4 | S | **P2** |
| A8 | Hoist coarse Goertzel carrier scan to engine/dsp; document per-plugin fine-AFC range in the trait | C4 | M | **P2** |
| A9 | Protect OFDM/SCFDMA length prefix (majority-vote triplication or CRC) and/or derive from Frame envelope | O3 | S | **P2** |
| A10 | 64QAM: replace 4th-power AFC with data-aided preamble estimator (copy 8PSK's); switch `demodulate_iq` to the matched half-Hann window | P7, P8 | S | **P2** |
| A11 | Deduplicate `estimate_frequency_offset_mth` (qpsk/64qam) into `openpulse-dsp` | Q3 | XS | **P2** |
| A12 | RAII `AfcScope` guard for save/restore of `afc_correction_hz`/`afc_step` | E5 | S | **P2** |
| A13 | Farrow interpolating timing loop with complex Gardner error; ±50 ppm SRO loopback acceptance test | C1, C2 | L | **P2** |
| A14 | OFDM/SCFDMA demodulate return `Result<_, ModemError>` with typed acquisition/short-buffer errors | O4 | S | **P3** |
| A15 | Engine: consult `supports_soft_demod()` before attempting soft path; don't double-demodulate on soft errors | E6 | XS | **P3** |
| A16 | Unify QPSK/8PSK carrier-recovery stack ordering (one canonical preamble-fit → PLL → LMS pipeline) | Q1 | M | **P3** |
| A17 | `ScFdmaPlugin::estimate_afc_hz`: split estimation from adaptive-state update (engine calls it on rejected noise windows) | O6 | S | **P3** |
| A18 | Doc fixes: BPSK Goertzel range (±400 not ±300/±600), OFDM lead threshold (0.20 not "half"), `CarrierPll` discriminant formula, remove "adaptive timing" claims until A13 lands | B3, O2, D1 | XS | **P3** |
| A19 | Either fix or delete dead `openpulse_dsp::preamble` (phase-sensitive `correlate_bpsk`, broken ZC real projection, broken QPSK chip mapping) — decide after A1 | C3, D3 | S | **P3** |
| A20 | Extract `receive_with_timeout` scan/retry/settle into a testable `SignalAcquirer`; replace wall-clock magic numbers with derived values | C5, E7 | L | **P3** |
| A21 | Define an LLR normalization convention across plugins (matters for `ArqSession` soft combining across mode switches; per-plugin LLR scales currently differ arbitrarily) | — | M | **P3** |

**Suggested sequencing:** A3/A5/A4 are independent quick wins shippable as one PR each
with hardware loopback validation. A1+A2 are the foundation PRs that the rest build on.
A13 is the one item with the potential to change the hardware story for 64QAM/SCFDMA52
beyond "buy better sound cards" — worth scheduling deliberately.

---

## 5. What is in good shape

For balance, things this review found solid:

- The phase-insensitive sync fixes (#385/#386) are correct and well-commented; OFDM's
  two-stage Schmidl-Cox + leading-path matched filter is a textbook-quality design.
- BPSK's two-stage AFC (Goertzel + squaring residual) is the right architecture — it
  just needs to be shared (A8).
- 8PSK's data-aided preamble CFO estimator is the best per-mode estimator in the tree.
- LLR sign convention (positive = bit 0) is consistently documented and respected
  across all soft demodulators and the engine fold.
- `LmsEqualizer` train-then-DD with per-mode profiles, plus the QPSK characterization
  sweep harness, is disciplined, evidence-based tuning with regression guards.
- OFDM soft path's |H|²-weighted LLRs correctly model post-ZF noise enhancement.
- FSK4's explicit hard-decision-only contract (`supports_soft_demod() = false` with a
  documented rationale) is exactly how a plugin should declare a limitation.

---

## 6. Implementation status (2026-06-12, branch `fix/modem-dsp-review-actions`)

All 21 actions executed in the same session as the review. Deviations from the
plan, found empirically:

| Action | Status | Notes |
|---|---|---|
| A1–A7, A9, A11, A12, A14, A15, A17–A21 | **Done** | As specified |
| A8 | **Done, approach changed for QPSK** | An m=4 Goertzel coarse stage does NOT work on Hann-shaped QPSK (envelope k·baud lines alias over the 4·fc line). QPSK instead gained a coarse data-aided preamble scan (±min(400, 3·baud) Hz). BPSK migrated onto the shared `goertzel_carrier_scan` (m=2, unchanged behaviour). |
| A10 | **AFC part done; window part withdrawn** | 64QAM switched to data-aided preamble CFO estimation (±baud/2). The window finding (P8) was wrong — see above. A coarse stage was also tried for 64QAM and reverted: the 4th power of a non-constant-modulus constellation is too self-noisy even for coarse acquisition. |
| A13 | **Done, deliberately scoped** | `FarrowTimingLoop` (cubic interpolator + complex Gardner + PI + fade-coast) wired into **BPSK-RRC** with the ±150 ppm acceptance test passing. QPSK/8PSK/64QAM RRC keep documented fixed-stride: at 1000 baud the Watterson delay spread spans 1–2 symbols, biases the TED toward the echo centroid, and regressed the fading coverage guards even at minimal loop bandwidth — while SRO over their short frames is negligible. The end-to-end SRO test exposed and fixed two further latent defects: BPSK-RRC had no carrier tracking at all (resampling shifts fc; LMS trains on real targets and cannot follow rotation → added a Costas PLL), and `LmsEqualizer` DD adaptation drifts to a wrong equilibrium over ~1000 clean symbols (added tap-energy guard + `with_frozen_dd()`; BPSK trains-then-freezes). |
| A16 | **Done (RRC ordering)** | QPSK RRC paths now apply the preamble phase fit after the PLL, matching 8PSK. The non-RRC paths were already identical. |
| A21 | **Done as doc + conformance test** | Full cross-plugin magnitude normalisation deliberately NOT performed: `snr_from_llrs` calibrates rate adaptation on the existing per-plugin scales. The contract (sign, ordering, per-plugin scale, per-frame weighting for cross-mode combining) is pinned in the trait doc and enforced by `llr_convention_conformance`. |

Hardware notes: rpi52's CM108 was replaced mid-session with the same C-Media
"USB Audio Device" model as rpi51 (loopback script updated to `cset`
raw-control names and the new 0–35 capture range; QPSK250 verified PASS on the
new card). The matched-clock pair weakens the "two free-running clocks" SRO
threat on THIS test rig, but the Farrow loop targets the general deployment.
