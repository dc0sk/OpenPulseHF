---
project: openpulsehf
doc: docs/dev/design/pn-preamble.md
status: draft
last_updated: 2026-10-05
---

# Replace the BPSK preamble with a PN-63 sync word (#1062)

Work plan decision 22 (2026-10-05): #1062 moves into Release 1 and gates M3. This is a wire-format
change, so it is reviewed as a design **before** any implementation (CLAUDE.md, adversarial review).
It builds on the #1062 thread's recorded state, not its body: revisions 2–4, the demod-parity column
(2026-08-04), the duration retraction (2026-09-07) and the f13 fade sweeps (2026-09-10).

## Problem

Every BPSK frame starts with 32 symbols of NRZI-encoded alternating bits, so the transmitted
symbols run `--++` with period 4. That one property causes four open defects:

1. **No interferer refusal.** The spectrum is lines at `fc ± baud/4` and odd harmonics. A steady
   tone on a line scores ρ ≈ 0.70 at any grid width (#1049 point 2), and sideband-symmetric shapes
   score 0.48–0.98. So a failed burst can never be shown to be *not ours*: #1460's remainder (a
   foreign CW/PSK31 over counts as ladder evidence) has no discriminator.
2. **No onset placement.** The template matches a copy of itself shifted 2 symbols at ρ = 1.000
   (`demod_parity` column D), so correlation cannot place a frame (#1049 point 3, #1052 reverted).
   The onset ranking (#1504) needs a preamble-span separation to stop that alias filling its ranks.
3. **No slow-rung templates.** BPSK31/63 cannot publish a template, because the frequency grid must
   stay under `baud/4` (7.8 / 15.6 Hz) and a tone near a line corroborates anyway
   (`chain_veto_slow_rung::q1`: BPSK31/63 corroborate a steady tone; BPSK250 refuses it). So the
   veto, the onset ranking and any evidence rule cover BPSK250 only.
4. **Per-mode coverage gaps compound.** QPSK publishes no template (#1053, #1059 withdrawn). The
   ladder's acquisition protection exists on one rung of five single-carrier rungs.

**Not a problem, and not claimed:** the noise floor. The thread established (2026-09-07, f7/f13)
that the in-band noise ceiling is set by template *duration*, for any sequence including a tone.
PN at the same length is tied with `--++` there. PN buys refusal and placement, not noise margin.

## Decision

- **Sequence:** a length-63 maximal-length sequence (m-sequence, degree 6), as **symbols** (±1),
  replacing the 32-symbol run. N = 31 was vetted and failed (worst interferer 0.377, worst family
  tone 0.389, grid-searched sidelobe 0.409 against the 0.40 reference); 63 is the floor.
- **Chip rate = the rung's own baud** (revision 2). A wider preamble dies in the payload-matched
  receive filter (a 4×-wide PN keeps 27 % through a 200 Hz mask), and it keeps the occupied
  bandwidth unchanged.
- **Specified in the symbol domain.** BPSK data is NRZI-differential and the first data symbol is
  referenced to the last preamble symbol (`range_start = PREAMBLE_SYMS - 1`). The wire definition is
  the symbol sequence `p[0..63]`; `preamble_bits` becomes the NRZI pre-image of it, so the modulator,
  GPU modulator and demodulator's expected table keep deriving from one function (phase 0.5's single
  source of truth).
- **Distinct sequence per mode** (revision 2's proposal). Degree 6 has six primitive polynomials, so
  BPSK31/63/100/250 each take one, chosen by the vetting gate below, not by name ("PN" guarantees
  nothing: 26 % of random 31-chip sequences fail the tone test). Distinct sequences make the
  correlation mode-selective at no cost and identify the mode from the sync word (REQ-RX-01).
  Cross-correlation between the chosen four is a vetting column.
- **Scope: the BPSK rungs, SL2–SL5, all four** at the flag day (revision 4: uniformity). One wire
  format across the BPSK ladder, so the rate adapter never crosses formats mid-session, at a uniform
  airtime cost of **1.49 %** (31 extra symbols at every baud).
- **Out of scope:** QPSK250-D (SL6) keeps its designed 16-symbol aperiodic sequence (R₁-minimal,
  drift-fit neutral, LMS-diverse; its defects are duration and fade overlap, which PN does not fix).
  MFSK16 (Costas), OFDM (Schmidl-Cox), FSK4 ACK, and the non-`hpx_hf` modes (8PSK, 64QAM, pilot,
  SC-FDMA) are untouched. A later QPSK change would be a second wire break; this is accepted
  knowingly, since QPSK is not where the four defects above live.
- **Flag day, no dual receive.** v0.17.0 is not cut and nothing is deployed (release-1.0-criteria
  decision 2: wire format may change freely until 1.0). An old receiver reports "invalid magic" on a
  new frame, as at #1148.

## What changes

**Wire (plugins/bpsk):** `PREAMBLE_SYMS` 32 → 63; `preamble_bits` per mode; frame geometry
(`preamble_samples`, `min_frame_samples`, `max_frame_samples`) follows.

**Receiver sites that consume the preamble length or pattern** (from
`grep -n PREAMBLE_SYMS plugins/bpsk/src/demodulate.rs`, ~30 production sites): data start and range
start (`PREAMBLE_SYMS - 1`), the AFC window (`4 × PREAMBLE_SYMS`), the expected-symbol tables for
timing, drift-fit and LMS training, the SNR lock over the first preamble symbols (#1142), the P6
rescue span, and the engine's hard-coded geometry comments and constants (`engine.rs` ~4715: "33 =
PREAMBLE_SYMS(32) + 1"; ~4800: "± PREAMBLE_SYMS (1024 samples)"; `ScanPlanner`'s 33-symbol minimum).
Every one is re-pointed at the plugin's geometry, not re-typed: a literal `32` or `33` left behind is
the one-sided-rebuild failure ("invalid magic") the phase 0.5 refactor exists to prevent. A gate
greps for them.

**Templates and constants:** each mode publishes a template only after its own constants are
derived (the `DERIVED_FOR` rule, #1053). BPSK250's `PREAMBLE_RHO_THRESHOLD` (0.40), grid (±20 Hz)
and `DELIVERED_FRAME_RHO_BOUND` (0.50) were derived on `--++` at 124 ms and are **void** for PN-63
at 252 ms; they are re-derived, not carried. The `baud/4` grid bound is a property of the period-4
line structure ("would be wrong for a PN successor", `plugins/bpsk/src/lib.rs`), so PN needs its own
grid bound, derived (the prediction that it is looser is to be tested, not assumed). BPSK31/63/100
publish if and only if their own columns pass; revision 3's argument that no same-bandwidth sequence
gives them a threshold was about the *noise* column, while their blocker today is the tone column,
which PN changes.

**Onset ranking (#1504):** `FALLBACK_RANKED_ONSETS` and the preamble-span separation are re-measured
on PN-63 (the alias the separation exists for should be gone); `fallback_onset_rank_measurement` is
reused unchanged.

**#1460, after:** with templates on the BPSK rungs, a failed burst counts as ladder evidence only if
a candidate rung's template corroborates it. Rungs without a template keep today's 0.5 s floor. A
separate PR on top of this one.

## Validation, in order, with what each result kills

Pre-registered: each row's pass rule is fixed here, before its numbers exist.

| # | measurement | pass rule | kills, if it fails |
|---|---|---|---|
| F1 | **BPSK250 parity at the production entry**: PN-63 vs `--++`, through `accumulate_capture` → `ota_decode_burst` (frame location in play, unlike `demod_parity` column E): decode rate on `moderate_f1` at the SL5 floor with `RsStrong`, AWGN cliff, acquisition across ±50 Hz (REQ-PHY-03), AFC lock rate, timing lock | PN within the paired 95 % CI of `--++` on every column, n ≥ 200 fade trials | **the whole design**; run before any other mode is touched |
| F2 | BPSK250's own constants: noise column (white, SSB, 500, 200 Hz), decode column (`moderate_f1`), interference column (tone swept over ±200 Hz, AM/DSB/comb at the mode's band) | a threshold exists with the decode tail above the noise and interference ceilings | BPSK250's template (it would fall back to energy-only, a regression; stop and redesign) |
| F3 | self-ambiguity and cross-correlation of the four chosen sequences at sample offsets | worst off-peak ≤ 0.5 of peak on any payload tried; cross-mode ≤ the mode's derived threshold | the sequence choice (pick another polynomial) |
| F4 | goodput gate at N = 63 (`goodput_gate`, benchmark) | passes as today | the chip count |
| F5 | F1 and F2 for BPSK100, BPSK63, BPSK31 | per mode | that mode's template only (it still changes on the wire, for uniformity) |
| F6 | synthetic regression fixtures for the #1021 / #1045 / #1049 defect classes under the new format, **landed before the flag day** | each fails on a sabotaged build | the flag day (the capture-based pins go dark with the old corpus) |
| F7 | full gate + `scripts/slow-tests.sh` at one commit | green except the disclosed rows | the merge |
| F8 | on-air: re-record the corpus on 2 m; un-ignore the four replay rows (#1351) | the replay rows decode | the campaign (M3) |

F1 is the cheapest falsifier and the most likely to kill: the preamble is training data (LMS, the
drift-fit, the timing metric and the fine AFC all consume known preamble symbols), and the only
prior parity evidence is `demod_parity`'s same-shape 32-symbol swap with the frame at buffer offset 0.
`demod_parity` measured PN-31 at parity there (timing, AFC, decode at n = 96) and the transition
density of a random balanced sequence (23 transitions) failed AFC at BPSK31; an m-sequence has
N/2 ± 1 transitions, like PN-31 (16), which passed. That is supporting, not sufficient.

## Cost

- Airtime +1.49 % on SL2–SL5.
- ~30 receiver sites, 3 modulator paths (Hann, RRC, GPU), frame geometry, the acceptance-table
  targets that name geometry (14 of 56 at the last count, a floor).
- Every BPSK ρ constant re-derived; the veto, the calibration (#1060) and the onset ranking
  re-measured.
- The replay corpus is dead until re-recorded (it already is: it predates #1148); F6 replaces its
  pins first.
- Schedule: past M2's 2026-10-28 target. M3 waits on it (decision 22).

## Open questions for the review

1. Is symbol-domain specification with an NRZI pre-image the right layering, or should the PN be
   specified in the bit domain (simpler generator, but the transmitted symbols are then the NRZI
   integral of an m-sequence, which is not an m-sequence and loses the autocorrelation property)?
   The design assumes symbol domain for that reason.
2. Does the last preamble symbol's role as the differential reference constrain the sequence
   (e.g. fix `p[62]`), and does a fixed final symbol cost anything measurable?
3. Is the AFC window (`4 × PREAMBLE_SYMS` = 252 symbols at 63) still right, or should it stay at 128
   symbols? The coarse AFC squares the signal, so the sequence is irrelevant at first order; the
   window length is a separate parameter that the doubling would change silently.
4. Is uniformity a sufficient reason to change BPSK31/63 on the wire if F5 fails for them?

## Consumer

`BpskPlugin` modulate/demodulate (every BPSK frame on SL2–SL5 and the station's `[modem] mode`
traffic), `BpskPlugin::preamble_template` → `ModemEngine::build_preamble_veto` →
`ranked_fallback_onsets` and the veto in `acquire_at_onset`, reached from `server.rs`'s
`ota_decode_burst`. Found by `grep -rn "preamble_bits\|preamble_template\|build_preamble_veto" plugins/bpsk/src crates/openpulse-modem/src`.

## Prior art

`openpulse-dsp::preamble` (`PreambleType::Pn63`, `pn_sequence(63, 0x45)`, x⁶ + x + 1), already
transmitted by the pilot plugin (`plugins/pilot/src/frame.rs:76`); MFSK16's Costas sync (aperiodic,
in production); QPSK's designed 16-symbol sequence; `demod_parity.rs` (the parity harness) and
`preamble_rho_fade_and_filter_probe.rs` (f7–f13, the duration/noise measurements). Found by
`grep -rn "Pn63\|fn pn_sequence" crates plugins --include=*.rs` and the #1062 thread.

## Twins

The GPU modulator (`preamble_bits` is its source too, and default CI does not compile it) and the RRC
path (which refuses a non-shipped expectation today and must accept the new one). The 8PSK and
64QAM plugins share the `PREAMBLE_SYMS` shape but are off `hpx_hf` and unchanged; the QPSK plugin
likewise. Found by `grep -rln "PREAMBLE_SYMS" plugins crates --include=*.rs`.
