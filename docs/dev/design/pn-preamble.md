---
project: openpulsehf
doc: docs/dev/design/pn-preamble.md
status: draft
last_updated: 2026-10-06
---

# Replace the BPSK preamble with a PN-63 sync word (#1062) — revision 2

Work plan decision 22 (2026-10-05): #1062 moves into Release 1 and gates M3. This is a wire-format
change, so it is reviewed as a design **before** any implementation (CLAUDE.md, adversarial review).
It builds on the #1062 thread's recorded state, not its body: revisions 2–4, the demod-parity column
(2026-08-04), the duration retraction (2026-09-07) and the f13 fade sweeps (2026-09-10).

Revision 2 folds in the mandatory review (`docs/dev/reviews/review-pn-preamble-design.md`): F1's pass
rule could not fail; the onset ranking reaches one rung, not four; the slow-rung blocker was
misquoted; distinct per-mode sequences had no consumer; receive cost was unmeasured; the AFC window
and QPSK were decided by assertion; and the receiver-site list had phantom and missing entries.

## Problem

Every BPSK frame starts with 32 symbols of NRZI-encoded alternating bits, so the transmitted
symbols run `--++` with period 4. Three open defects follow from that, plus a coverage gap:

1. **No interferer refusal.** The spectrum is lines at `fc ± baud/4` and odd harmonics. A steady
   tone on a line scores ρ ≈ 0.70 at any grid width (#1049 point 2), and sideband-symmetric shapes
   score 0.48–0.98. So a failed burst can never be shown to be *not ours*: #1460's remainder (a
   foreign CW/PSK31 over counts as ladder evidence) has no discriminator.
2. **No onset placement.** The template matches a copy of itself shifted 2 symbols at ρ = 1.000
   (`demod_parity` column D), so correlation cannot place a frame (#1049 point 3; the onset-snap
   was built and reverted). The onset ranking (#1504) needs a preamble-span separation to stop that
   alias filling its ranks.
3. **BPSK31/63 cannot publish a template, for two independent reasons.** (a) The **margin**: the
   2026-08-04 elimination failed its pre-registered rule on the noise ceiling (0.426) against the
   weakest decodable frame (0.569), correlator-independent, with the decode edge still falling with
   sample size; revision 3 showed both sides are sequence-invariant at the same bandwidth. (b) The
   **tone**: the grid must stay under `baud/4` (7.8 / 15.6 Hz) and a tone near a line corroborates
   anyway (`chain_veto_slow_rung::q1`, 2026-08-19). **PN removes (b) only.** It also doubles the
   duration, which lowers the noise ceiling ×0.68–0.73 for any sequence (2026-09-07), but coherent
   gain is bounded by the ~0.4 s coherence time on `moderate_f1` (2026-08-09, correction 2), BPSK31's
   template is already 1 s, and f13 shows the delivered-frame p10 falling 0.03–0.08 under masks. The
   honest prior is that BPSK31/63 **still cannot publish** after this change.
4. **Coverage.** The veto, the onset ranking and any evidence rule cover BPSK250 only today.

**Not a problem, and not claimed:** the noise floor. The in-band noise ceiling is set by template
*duration*, for any sequence including a tone (2026-09-07). PN buys refusal and placement.

## Decision

- **Sequence:** the length-63 m-sequence from x⁶ + x + 1 (`openpulse-dsp::preamble::Pn63`, already
  transmitted by the pilot plugin), as **symbols** (±1), replacing the 32-symbol run. N = 31 was
  vetted and failed (worst interferer 0.377, worst family tone 0.389, grid-searched sidelobe 0.409
  against the 0.40 reference); 63 is the floor. If F3 rejects this polynomial, the next of the six
  degree-6 primitives is vetted; the choice is one sequence for all rungs.
- **One sequence, not one per mode** (changed from revision 1). No two BPSK rungs share a chip rate
  (31.25 / 62.5 / 100 / 250), so the sequences never meet at the same rate and the mode is already
  told apart by baud and geometry. Revision 2 of the thread justified distinct sequences for two
  modes at the *same* chip rate and called the harm a nuisance. One sequence keeps `preamble_bits`
  a single source of truth and one fixture set.
- **Chip rate = the rung's own baud** (thread revision 2). A wider preamble dies in the
  payload-matched receive filter (a 4×-wide PN keeps 27 % through a 200 Hz mask), and occupied
  bandwidth is unchanged.
- **Specified in the symbol domain.** The NRZI pre-image is `b[k] = p[k] XOR p[k−1]`, `p[−1] = +1`
  (`nrzi_encode` starts at `phase_neg = false`, `modulate.rs:326`), which is exactly what
  `demod_parity`'s `symbols_to_bits` did, so the PN-31 parity evidence is for a symbol-domain
  m-sequence. `p[62]` is unconstrained: differential decode starts at `PREAMBLE_SYMS − 1` and NRZI
  runs on into the data, so the last preamble symbol's sign never enters a data bit, and the template
  already drops it (`modulate.rs:302`). `preamble_bits(len)` keeps its length argument
  (`demodulate.rs:904` passes `PREAMBLE_SYMS.min(n)`): `len ≤ 63` **truncates** the sequence, and
  `len > 63` is an error, never a cycle.
- **Scope: the four BPSK rungs, SL2–SL5, for uniformity** (thread revision 4) — one generator and
  one demod tuning across the BPSK ladder, at a uniform **1.49 %** airtime. Uniformity has a limit,
  stated now: **if F5's parity columns fail at a slow rung, that rung keeps `--++`** (two generators,
  accepted); a failed *template* column there changes nothing, since that rung cannot publish
  today either. The smaller alternative the review named — PN-63 on BPSK250 (and BPSK100 if its
  columns pass) only — delivers the same defect fixes, because only BPSK250 can carry the template
  path (see *Onset ranking*); it is the fallback if F5 fails.
- **QPSK250-D (SL6): decided by a number, not by assertion.** Its 16-symbol designed sequence has a
  lag-2 sidelobe of 0.500 (a placement defect) and its #1053 failure (decodable tail 0.276 against
  ceiling 0.291) is a 64 ms duration problem that a longer preamble plausibly fixes; SL6 is the
  `robust` profile's top rung. F0 measures its noise ceiling at 64 symbols. If that opens the margin,
  QPSK joins this break (one window); if not, it stays out and a later change is a second break,
  paid knowingly.
- **Out of scope:** MFSK16 (Costas), OFDM (Schmidl-Cox), the FSK4 ACK, and the non-`hpx_hf` modes
  (8PSK, 64QAM, pilot, SC-FDMA).
- **Flag day, no dual receive.** v0.17.0 is not cut and nothing is deployed (release-1.0-criteria
  decision 2). An old receiver reports "invalid magic" on a new frame, as at #1148.

## What changes

**Wire (plugins/bpsk):** `PREAMBLE_SYMS` 32 → 63; `preamble_bits` returns the NRZI pre-image of
`Pn63`; frame geometry (`preamble_samples`, `min_frame_samples`, `max_frame_samples`) follows. The
Hann, RRC and GPU modulators all derive from `preamble_bits`; the RRC demod path, which refuses a
non-shipped expectation today, accepts the new one.

**AFC window, decoupled.** `estimate_carrier_hz_wide` uses `4 × PREAMBLE_SYMS × n`
(`demodulate.rs:437`); at 63 that is 252 symbols, 1 s at BPSK250 and 8 s at BPSK31, on a fade. The
coarse stage squares, so the sequence is irrelevant there and the window is a separate parameter: it
becomes a named constant at **128 symbols** (today's value), not `4 × PREAMBLE_SYMS`. Inside the
engine's settle the plugin is handed preamble + 1 symbol anyway; the 4× applies to whole-burst
callers.

**Receiver sites.** Every `PREAMBLE_SYMS` use in `plugins/bpsk/src/demodulate.rs` follows the
constant: data and range start (`PREAMBLE_SYMS − 1`), the expected-symbol tables for timing and LMS
training, the P6 rescue span, `frame_geometry`. The engine's hard-coded 32/33 (`engine.rs:2818,
4717`) are the fallback for plugins **without** geometry; BPSK publishes geometry, so they are not
production sites and only their comments go stale (corrected in place). Revision 1 listed a
"drift-fit" and an "SNR lock over the preamble"; neither exists in the BPSK demod (the SNR estimate
runs over the data span from `range_start`, `demodulate.rs:263-276`; #1142 is success gating in the
engine).

**Tests whose premise is `--++`, rewritten rather than re-run:**
`preamble_correlation_settle::the_gate_is_not_fooled_by_a_steady_tone` (acceptance row 86) asserts
the template has ≥ 2 spectral lines and that an on-line tone fools the correlator; on PN its premise
guard fires, and its replacement asserts the tone is **refused**. The restricted-lock rescue
(`demodulate.rs:1029, 1447`, acceptance row 54, "at the −2-symbol alias") goes vacuous and is
re-scoped. The #1454 spectral-busy gather counts (16/16, acceptance row 92) were fitted on the
alternating preamble's envelope and are re-measured (F5).

**Templates and constants:** each mode publishes a template only after its own constants are
derived (`DERIVED_FOR`, #1053). BPSK250's threshold (0.40), grid (±20 Hz) and delivered-frame bound
(0.50) were derived on `--++` at 124 ms and are **void** at 252 ms; re-derived, not carried. The
`baud/4` grid bound belongs to the period-4 line structure; PN's own grid bound is derived (F2).

**Onset ranking (#1504): BPSK250 only, by construction.** A PN-63 template is 62 symbols:
1 984 samples at BPSK250 against `MAX_PREAMBLE_CORRELATION_SAMPLES = 2_048` (`engine.rs:387`), and
4 960 / 7 936 / 15 872 at BPSK100/63/31, which take the DDC arm, where `ranked_fallback_onsets`
returns `None` (`engine.rs:3035`). BPSK250 fits by 64 samples, so a gate pins BPSK250's template to
the passband arm (as `veto_membership_pin` pins membership); keeping the 63rd symbol or raising N
would silently drop the receive-cost fix. K and the separation are re-measured on PN-63. A DDC-arm
rank path is out of scope (`[modem] mode` defaults to BPSK250).

**#1460, after:** with a BPSK250 template, a failed burst counts as ladder evidence only if a
candidate rung's template corroborates it; rungs without one keep the 0.5 s floor. A separate PR.

## Validation, in order, with what each result kills

Pre-registered: each pass rule is fixed here, before its numbers exist. **Non-inferiority**, not
"within the CI": a column passes only if the **lower** bound of the paired 95 % CI of
(PN − `--++`) is ≥ −δ. δ and n are fixed now, n sized to resolve δ (the thread needed 600 paired
seeds to resolve 0.03–0.08, 2026-09-10).

| # | measurement | pass rule | kills, if it fails |
|---|---|---|---|
| F0 | QPSK250 noise ceiling at 64 vs 16 symbols: the f2 statistic (max peak ρ over 5 seeds × 15 s of band noise), white / SSB / 500 / 200 Hz; the 64-symbol template from `PreambleSpec(Pn63, 64, Qpsk)`. Positive control: the 16-symbol SSB cell reproduces #1059's 0.293 within 0.03 | QPSK joins only if the 64-symbol ceiling is ≤ 0.23 (= 0.276 / 1.2, the 16-symbol decodable tail with a 1.2 margin) in **both** SSB and 500 Hz. The decodable tail at 64 symbols is unmeasurable without a 64-symbol QPSK receiver, and f13 says it falls with length, so a pass is necessary, not sufficient | QPSK joining this break (it stays out) |
| F1 | **BPSK250 parity at the production entry**, PN-63 vs `--++`, through `accumulate_capture` → `ota_decode_burst` (frame location in play): decode rate on `moderate_f1` at the SL5 floor with `Rs`; decode rate at the AWGN cliff (fixed at **−5 dB** from the shipped arm's pilot, 100 trials per SNR: −6 dB 12/100, −4 dB 99/100, before the main run); decode rate across ±50 Hz offsets on `moderate_f1` (REQ-PHY-03); timing-lock rate. n = 600 paired per column | δ = 0.03 absolute on every rate | **the whole design**; run before any other mode is touched |
| F1c | **Receive cost**, `receive_cost_scaling` x86 and both Pis, `PROBE_READ=4096`, after the change | Pi SL2 decode + 0.52 s FSK4 ACK + 1 s margin fits the 9 s ACK window | the slow rungs taking PN (settle window, timing search, veto and phase-2 windows all scale with `preamble_samples`) |
| F2 | BPSK250's own constants, PN-63 template (`bpsk_preamble_template` on `BPSK250-PN`, 1 984 samples), engine window (template + 2 symbols) and grid (±20 Hz, step from the span), shipped template alongside as control. **Noise:** peak ρ, max over 5 seeds × 15 s, white / SSB / 500 / 200 Hz; control: the shipped template reproduces f2's BPSK250 column within 0.03. **Decode:** 400 seeds of `moderate_f1` at 5 dB and at 3 dB (F1's SNR convention), payload 24 B `Rs`; ρ at the true onset for every frame that decodes at its true onset; report min, p01, p05. **Interference:** ρ against a lone tone swept fc ± 200 Hz in 1 Hz steps, AM (carrier ± 31.25/62.5/125 Hz), DSB (the same pairs, no carrier) and a comb (tones every 31.25 Hz across ±250 Hz), grid centred at 0 | **pass** if the decodable p01 at 3 dB is ≥ 1.2 × max(SSB noise ceiling, worst interference ρ); the threshold is then the geometric mean of the two sides. The 500/200 Hz cells are reported, not gating: the #1157 runtime calibration owns narrow filters, as it does today | BPSK250's template (energy-only fallback: a regression; stop and redesign) |
| F3 | self-ambiguity of x⁶ + x + 1 at sample offsets ≥ 1 symbol, on a **fixed** set of 32 whitened random payloads | worst off-peak ≤ 0.5 of peak on every payload in the set | the polynomial (vet the next primitive) |
| F4 | goodput gate at N = 63: the gate's two PSK cases (`fast`, AWGN 20 dB, 200 B × 40, seed 5; `moderate_f1` 20 dB, 64 B × 60, seed 7) on `apparatus:fast-pn` (`fast`'s rungs through `from_rungs`, BPSK31–250 as `-PN`) against `apparatus:fast-copy` (the same rebuild, shipped preamble). Positive control: `fast-copy` reproduces `fast` within 2 % on both cases. The CLI benchmark replays HPX state-machine events with no modem, so the preamble cannot move it; it is run once as a check, not a measurement | the PN ladder clears the shipped floors (AWGN ≥ 250 bps; fade avg level ≥ 3.0, delivery > 0.9), and its AWGN goodput is ≥ control × (1 − 0.0149) × 0.95 (the airtime cost, then 5 % for one seed) | the chip count |
| F5 | for BPSK100, BPSK63, BPSK31: F1's parity columns (δ = 0.03, n = 600) and the #1454 gather count (BPSK31 at +8 dB, wide filter, 16 placements); then the template columns with the 2026-08-04 pre-registered margin rule and CI-calibrated margins, on the engine's shipped grid (2026-09-11 caveat) | parity: as F1; gather ≥ today's 16/16; template: the rule | parity fail → that rung keeps `--++`; template fail → no template (as today) |
| F6 | synthetic regression fixtures for the #1021 / #1045 / #1049 defect classes under the new format, **landed before the flag day** | each fails on a sabotaged build | the flag day (the capture pins go dark with the old corpus) |
| F7 | full gate + `scripts/slow-tests.sh` at one commit | green except the disclosed rows | the merge |
| F8 | on-air: re-record the corpus on 2 m; un-ignore the four replay rows (#1351) | the replay rows decode | the campaign (M3) |

F1 is the cheapest falsifier and the most likely to kill: the preamble is training data (LMS and
the timing metric consume known preamble symbols), and the only prior parity evidence is
`demod_parity`'s same-shape 32-symbol swap with the frame at buffer offset 0. There, PN-31 was at
parity (timing, AFC, decode at n = 96), and a random balanced sequence with 23 transitions failed
AFC at BPSK31 (unexplained; a hypothesis, 2026-08-04). x⁶ + x + 1 has 32 runs, so 31 transitions in
63 (density 0.49, against 0.47 shipped and 0.50 for PN-31), which passed. Supporting, not sufficient.

## Results so far (2026-10-05)

**F0 — QPSK stays out.** `f14_qpsk_preamble_length_and_the_noise_ceiling`, release, 5 seeds × 15 s:

| band | 16 symbols (shipped) | 64 symbols (PN-63 pairs) |
|---|---|---|
| white | 0.257 | 0.124 |
| SSB 300–2700 | 0.332 | 0.158 |
| 500 Hz | 0.614 | 0.334 |
| 200 Hz | 0.797 | 0.414 |

Rule: 64-symbol ≤ 0.23 in SSB **and** 500 Hz → SSB passes, 500 Hz fails (0.334): **FAIL, QPSK stays
out of this break.** The positive control **missed**: the 16-symbol SSB cell read 0.332 against
#1059's 0.293 (tolerance 0.03), most likely because this run's grid (±20 Hz, 11 hypotheses) is wider
than the withdrawn QPSK template's (not recorded); more hypotheses raise the noise maximum. The
verdict survives the miss: scaling every cell by the control's ×1.13 overestimate leaves the 500 Hz
cell at 0.295 > 0.23. The ratio (×0.48–0.54 for 4× the duration, every band) matches the 1/√T law.

**F1 — PASS on every column; PN-63 is better than `--++` on four of five.**
`pn_preamble_parity::f1_bpsk250_pn63_against_the_shipped_preamble`, release, n = 600 paired, δ = 0.03,
through `accumulate_capture` → `ota_decode_burst` in 4 096-sample reads, lead 8–9 reads of noise:

| column | shipped | PN-63 | PN − shipped, 95 % CI | |
|---|---|---|---|---|
| `moderate_f1` 5 dB (SL5 floor) | 507 | 527 | +0.033 [+0.010, +0.056] | PASS |
| `moderate_f1` 8 dB | 558 | 567 | +0.015 [−0.004, +0.034] | PASS |
| `moderate_f1` 8 dB, +50 Hz | 537 | 559 | +0.037 [+0.015, +0.058] | PASS |
| `moderate_f1` 8 dB, −50 Hz | 543 | 561 | +0.030 [+0.007, +0.053] | PASS |
| AWGN −5 dB (the cliff) | 534 | 550 | +0.027 [+0.005, +0.049] | PASS |

SNR here is frame power over full-band noise (8 kHz), not the ladder table's convention, so the
absolute rates do not compare with `mode-fec-ladder.md`; the paired difference is the measurement.
Positive control: `both_arms_deliver_a_clean_frame` (default run).

**F1c, x86 half** (`receive_cost_scaling`, release, `PROBE_READ=4096`, `PROBE_PN=1` for the
candidate), decode per frame: SL5 0.83 → 0.94 s (+14 %), SL4 0.92 → 1.27 s (+38 %), SL3 1.14 →
1.76 s (+54 %), SL2 1.79 → 3.08 s (+72 %). At the Pis' measured ×1.5 that projects SL2 to ≈ 4.7 s,
inside the rule (decode + 0.52 s FSK4 ACK + 1 s ≤ 9 s), but **the rule is on the Pis, pending**.
With the ≈ 5.9 s MFSK16 ACK SL2 would not fit (it barely fits today at ≈ 2.7 + 5.9 s); the MFSK16
ACK follows only an SL1 recommendation, which this rule did not cover — noted, not re-scoped.

**F2 — FAIL under the pre-registered rule (7b918cff); the sequence stops here for a decision.**
`f15_bpsk250_pn63_constants`, release, 400 seeds per decode cell, same harness for both templates:

| | shipped `--++` (992) | PN-63 (1 984) |
|---|---|---|
| noise white / SSB / 500 / 200 Hz | 0.159 / 0.205 / 0.436 / 0.627 | 0.118 / 0.152 / 0.327 / 0.433 |
| lone tone, fc ± 200 Hz | 0.699 (+83 Hz) | 0.248 (−60 Hz) |
| AM ±31.25 / ±62.5 / ±125 Hz | 0.114 / 0.569 / 0.054 | 0.182 / 0.271 / 0.197 |
| DSB ±31.25 / ±62.5 / ±125 Hz | 0.136 / **0.981** / 0.036 | **0.304** / 0.271 / 0.170 |
| comb every 31.25 Hz | 0.291 | 0.261 |
| decodable, `moderate_f1` 5 dB | 345/400, min 0.295, p01 0.388 | 355/400, min 0.325, p01 0.387 |
| decodable, `moderate_f1` 3 dB | 275/400, min 0.306, p01 0.324 | 298/400, min 0.318, p01 0.327 |
| rule: p01 @3 dB ≥ 1.2 × max(SSB, interference) | 0.324 vs 1.177 — FAIL | 0.327 vs 0.364 — **FAIL** |

Control: the shipped template reproduces f2's SSB / 500 / 200 Hz cells (0.196 / 0.441 / 0.624) within
0.01; its white cell (0.159 vs 0.080) differs for the documented reason — f2's table predates the
`band_noise` DC fix. Reading: PN-63 cuts the worst interferer from 0.981 to 0.304 and the lone tone
from 0.699 to 0.248, and lowers every noise ceiling ~25 %, but a threshold that refuses every
measured interferer (> 0.304) keeps the 3 dB decodable p01 (0.327) by a margin of 1.07, not the
1.2 the rule asked for. The binding interferer is a DSB pair at fc ± 31.25 Hz; the binding decode
tail is #1059's fade-null case (a preamble inside a fade), which no sequence removes.

**Decision 23 (maintainer, 2026-10-06): accepted.** BPSK250-PN publishes a template with threshold
**0.315** (the geometric mean of 0.304 and 0.327), on the ground that it beats the shipped template
on both sides: today's 0.40 passes the tone (0.699) and the DSB pair (0.981) and rejects more of the
fade's decodable frames (shipped p05 0.490). The rule's 1.2 margin is not met and is recorded as not
met; the margin is 1.07. The validation continues at F3.

**F3 — FAIL as pre-registered, for every degree-6 polynomial; decision needed.**
`f16_self_ambiguity_over_whitened_frames` (engine-built `Rs` frames, x⁶ + x + 1) and
`f16b_self_ambiguity_by_polynomial` (all six primitives, 32 seeded 255-byte blocks), 0 Hz:

| polynomial | whole frame (rule ≤ 0.5) | preamble span (≤ 63 symbols) | scan range (≤ 13 000 samples) |
|---|---|---|---|
| x⁶+x+1 | 0.572 (engine frames: 0.526) | 0.430 | 0.542 |
| x⁶+x⁵+1 | 0.583 | 0.375 | 0.467 |
| x⁶+x⁴+x³+x+1 | 0.653 | 0.441 | 0.484 |
| x⁶+x⁵+x³+x²+1 | 0.528 | 0.405 | 0.453 |
| x⁶+x⁵+x²+x+1 | 0.557 | 0.364 | 0.521 |
| x⁶+x⁵+x⁴+x+1 | 0.521 | 0.462 | 0.492 |
| shipped `--++` (control) | 0.972 at 1.97 symbols | — | — |

The whole-frame worst is chance correlation of a 62-symbol template with whitened payload
(σ ≈ 1/√62 ≈ 0.13 per offset, maximised over ~2 000 symbol offsets per frame), so it is set by
frame length, not by the polynomial, and no N = 63 sequence can meet the rule as written; the same
polynomial reads 0.526 and 0.572 on two payload sets, which is larger than most gaps between
polynomials. Inside the preamble span every candidate sits at 0.36–0.46 against the shipped 0.97.

**A finding F2 missed:** the same chance correlation applies to a *foreign* same-baud PSK signal
carrying random data. Over a long over it can exceed the 0.315 threshold, so neither the veto nor a
#1460 evidence rule can refuse same-baud data signals by ρ alone. This is inherent to a 62-symbol
template (the shipped one is worse) and bounds what #1460's rule can promise.

**Decision 24 (maintainer, 2026-10-06): keep x⁶ + x + 1.** F3 is recorded as failed with a misframed
rule; the polynomial stays because the measured differences are within payload noise and it is
already in production. The validation continues at F4.

### F4: goodput gate (rule pre-registered in a1619ea2)

| ladder | AWGN 20 dB | `moderate_f1` 20 dB delivery | avg level | final |
|---|---|---|---|---|
| `fast` | 331 bps | 0.98 | 9.8 | SL13 |
| `apparatus:fast-copy` (control) | 331 bps | 0.98 | 9.8 | SL13 |
| `apparatus:fast-pn` | 328 bps | 1.00 | 9.8 | SL11 |

**PASS.** The control reproduces `fast` exactly; the PN ladder clears both floors and its goodput
is above the bound 331 × 0.9851 × 0.95 = 310 (−0.9 %, one seed). The CLI benchmark replays state-machine events with no
modem: 10/10, mean transitions 5.1, as expected. The validation continues at F5.

### F5 parity: parameters fixed before the pilots

The F1 harness per rung (`F1_RUNG=31|63|100`), the same five columns, n = 600 paired, δ = 0.03,
one-rung `Rs` ladders at each rung's own level. Two changes from F1, both fixed now:

- **Payload 16–64 B** (`F1_PAYLOAD_MAX=64`), not 16–200 B: at BPSK31 a 200 B frame is 66 s of audio,
  which triples the run for no preamble information (the preamble is the same 63 symbols at any
  payload). Every rung uses the same cap, so the three are comparable with each other, not with F1.
- **Floor column** at each rung's `fast` floor: BPSK31 3 dB, BPSK63 4 dB, BPSK100 4.5 dB.
- **Cliff column:** a pilot of 100 trials per SNR on a 2 dB grid (BPSK31 −18…−10, BPSK63 −15…−7,
  BPSK100 −13…−5 dB); the cliff is the midpoint of the two grid points bracketing the shipped arm's
  50 % decode rate (F1's rule: −6 dB 12 %, −4 dB 99 % → −5 dB). If the grid does not bracket 50 %,
  it is extended by 4 dB on the open side and rerun, never interpolated.

**Pilots (2026-10-06, 100 trials per SNR, shipped / PN):** BPSK31 −16 dB 0/0, −14 dB 57/46,
−12 dB 100/94; BPSK63 −13 dB 0/0, −11 dB 96/95; BPSK100 −11 dB 0/0, −9 dB 89/98. **Cliffs fixed:
BPSK31 −15 dB, BPSK63 −12 dB, BPSK100 −10 dB.** The PN arm trailing at BPSK31's −14 and −12 dB is
a pilot reading, not a verdict; the main run decides it.

**First main run, BPSK31, floor column (the receiver as of c56fc245): FAIL.** Shipped 599/600,
PN 573/600, discordant 26/0, PN − shipped −0.043 [−0.060, −0.027]. `f5_diagnose_discordant_seeds`
put the loss in demodulation, not gathering: each lost frame was gathered whole, and failed again
when cut at its true onset. **Mechanism:** the timing search summed the preamble correlation
coherently over the whole preamble, 2.0 s at BPSK31-PN against 1.0 s shipped and 0.25 s at
BPSK250 (where PN won). A fade, or a carrier error inside the AFC's 2 Hz deadband, turns the phase
across that span and cancels the sum. **Fix (b4bcf90b):** coherent over at most 32 symbols (the
shipped length, so the shipped lock is bit-identical and no constant is fitted), segments added
in power; the GPU kernel declines past one segment. Seeds 1–80: 2 discordant before, 0 after;
`a_long_preamble_locks_through_a_deadband_carrier_error` fails with the span uncapped. The run was
stopped there; F5 restarts on the fixed receiver, **and F1 is rerun** because BPSK250-PN's lock
changed from one 63-symbol sum to two segments.

**Amendment (before any offset-column result): the ±50 Hz columns on the slow rungs run at
n = 200, not 600.** A 4-pair timing probe of BPSK31's +50 Hz column took 271 s (4/4 on both arms,
the only outcome seen): about 100 s of x86 CPU per 66 s frame, against ~8 s per pair at 0 Hz, the
same on both arms. At n = 600 each offset column costs ~8 h. At the measured decode rates (≈ 1.0)
200 pairs resolve δ = 0.03 unless discordance appears, and the column is then re-run at 600.
The other three columns keep n = 600. Run per column with `F1_COLUMN`. The receive cost itself is
a separate Release 1 finding (work plan), not a preamble question: it is the shipped receiver's.

**BPSK31 cliff column, as pre-registered (−15 dB): vacuous.** Shipped 2/600, PN 4/600, discordant
2/4, CI [−0.005, +0.011]: it passes, but both arms sit on the floor, so it says nothing. The
midpoint rule assumed a gentler curve than the pilot's (−16 dB 0 %, −14 dB 57 %). **Amendment,
before any −14 dB main-run result:** the column is re-run at −14 dB, the pilot grid point nearest
50 %, n = 600, and the same rule applies to BPSK63/100 if their cliff columns land under 10 % on
both arms.

**Receiver for the remaining F5 columns: d9031766** (the off-frequency scan fix, REQ-PHY-03,
found by this run). Both arms share it, so parity is unaffected, and it is the receiver the flag day
ships. It makes an offset frame cost what an on-frequency one does, so **the ±50 Hz columns return
to the pre-registered n = 600** (the n = 200 amendment was for cost alone, and no offset-column result
had been seen). Columns already run (BPSK31 floor/8 dB/−15 dB, BPSK63 floor/8 dB/−12 dB) ran on
b4bcf90b. BPSK63's −12 dB column also sat under 10 % on both arms (27 / 34 of 600, CI
[−0.013, +0.036]), so by the amendment above it is re-run at −11 dB.

**BPSK100 on d9031766 (n = 600):** floor 598 / 599, discordant 0/1, +0.002 [−0.002, +0.005] PASS;
8 dB 597 / 599, discordant 0/2, +0.003 [−0.001, +0.008] PASS. The −10 dB cliff column sat under
10 % on both arms (30 / 31, discordant 27/28, +0.002 [−0.023, +0.026]), so by the same amendment it
is re-run at −9 dB, the pilot grid point nearest 50 % (89 %; −11 dB read 0 %). Recorded before the
−9 dB result.
**BPSK100 at −9 dB (n = 600): PASS.** Shipped 564/600, PN 579/600, discordant 17/32, +0.025
[+0.002, +0.048].

**F5 on d9031766, complete.** Every column passes on BPSK100 and BPSK63 (PN ahead at both cliffs),
and on BPSK31 except one: **BPSK31's AWGN cliff at −14 dB fails** (−0.078), a carrier-detect
gathering deficit (below). Whether BPSK31 takes PN-63 is therefore open; the options are the
maintainer's (keep the shipped preamble on BPSK31 only, or make the carrier detect open as early
on the PN preamble, a DSP change that needs its own measurement and review).

**Decision 25 (maintainer, 2026-10-07): BPSK31 keeps the shipped `--++` preamble**, as F5's
pre-registered consequence says, and the carrier detect is not changed for it. The flag day puts
PN-63 on BPSK63, BPSK100 and (once its template is settled) BPSK250. The −14 dB gathering deficit is
parked with its measurements above, not ruled out.

**BPSK31 cliff column re-run at −14 dB (d9031766, n = 600): FAIL.** Shipped 345/600, PN 298/600,
discordant 136/89, PN − shipped −0.078 [−0.127, −0.030]. The pilot read the same way (57 / 46 at
−14 dB). This is the pre-registered column, so BPSK31 fails F5 as it stands. The mechanism is not
yet known; next is `f5_diagnose_discordant_seeds` on the 136 shipped-only seeds (gathering vs
demodulation, as for the floor-column failure), and no constant is changed before that reads.

**Where the BPSK31 cliff loss sits: the carrier detect, not the demodulator.**
`f5_diagnose_discordant_seeds` (`F1_COLUMN=4`, seeds 1–40): 15 discordant, 10 PN-only losses and
5 shipped-only; 14 of the 15 losing arms decode when the frame is cut at its true onset. The losses
are a burst never gathered (PN 5, shipped 4) or one that opens late or split, 418–2 107 samples
past the onset (PN 5, shipped 1). Cutting frames late shows a lost head is fatal to **both**
preambles (seeds 1–2: a 500-sample cut fails on either arm), so the asymmetry is not PN's
truncation tolerance. `f5_open_latency_and_truncation` with `F1_DIAG_GATHER_ONLY=1`, seeds
1–200, first gathered burst per arm:

| arm | no burst | burst covers the head | burst opens late |
|---|---|---|---|
| shipped | 62 | 132 | 6 |
| PN | 79 | 98 | 23 |

Paired: the head is gathered on shipped alone in 54 seeds and on PN alone in 20. The floor is warm
when every frame starts (8 reads of noise, 64 windows; warm at 16), so a cold floor learning the
head is ruled out for this harness. Which part of the carrier detect opens later on the PN preamble
at −14 dB (total power vs the spectral test, and why) is not yet measured. BPSK63 at −11 dB passed
with PN ahead (below), so the effect is at least rung- or SNR-dependent.

Two facts bound it. **The longer preamble buys nothing for gathering:** the head is kept only if
the detector opens within the pre-trigger lead (the previous read plus `S_LOOKBACK` windows, ~6 000
samples at 4 096-sample reads), and PN-63's extra 31 symbols lie past that. **The PN preamble's
band power is less steady:** `f5_preamble_band_power_by_phase` (clean frame, both of the spectral
test's window phases, best 4-bin band ÷ data median) reads the shipped preamble at 1.00 in every
window of both phases, the PN preamble at 0.70 / 1.00 / 1.28 (0.70 in 9 of 31 windows on one
phase, 8 of 31 on the other), and no window of either preamble or the data below 0.5. The spectral
test's two-phase design was built around the alternating preamble (#1454 round 7). Whether a
1.5 dB dip in a quarter of the windows accounts for the deficit is **not shown**: near the open
threshold a fluctuation can as well help as hurt a 3-of-4 count. Candidate, not mechanism.

**BPSK63 cliff column re-run at −11 dB (d9031766, n = 600): PASS.** Shipped 560/600, PN 579/600,
discordant 19/38, +0.032 [+0.007, +0.056].

**Offset columns (d9031766, n = 600), all PASS with no discordant pair:** BPSK100 +50 Hz and
−50 Hz 599 / 599; BPSK63 and BPSK31, +50 Hz and −50 Hz, 599 / 599. Spectral-busy gather counts
(`f5_pn_gather_counts`): every condition 16/16 decoded, whole and with its head, on both arms of
BPSK31 and BPSK63.

**A second gap, found preparing F6: `BPSK250-PN` publishes no template.** `preamble_template`
returns one only for the exact mode `BPSK250`, so F1's PN arm ran with no correlation veto and no
onset ranking, energy-only, and still matched or beat the shipped arm. The flag-day configuration
publishes a template with threshold 0.315 (decision 23) and grid ±20 Hz (F2), and a delivered-frame
bound that was never re-derived (the shipped 0.50 is void at 252 ms, *Templates and constants*).
**Derivation, fixed before the run:** f9 (`F9_MODE`), veto off on the receiver, `filter 1250-1750`
on `moderate_f1` at 5 / 10 / 20 dB, 120 seeds per cell, payload 200 B `Rs`, deterministic budget
`F9_POS=8000 F9_ITERS=64000` (the #1058 family's); the bound is the lowest of the three cells'
decoded-ρ p01, rounded down to 0.01. The shipped template runs the same cells as a control and is
reported beside its 0.50. Then F1 is rerun on BPSK250 with the full template.
**Amended before any output:** the deterministic budget (`F9_POS=8000 F9_ITERS=64000`) did not
finish the first cell in ~4 h (the probe's own comment records a smaller budget taking > 2 h), and
was stopped with nothing printed. The derivation uses the probe's default budget, the regime the
shipped 0.50 was derived in, on an otherwise idle machine, so the wall-clock budget is not
truncated by load.

**f9 result (2026-10-07, d9031766, default budget, veto off, `filter 1250-1750`, `moderate_f1`,
120 seeds per cell).** The first run overlapped other load, against the condition above, so both
arms were rerun on an idle machine (load average 1.00 throughout, the probe's one core): **every
figure reproduced exactly**, so the probe is deterministic here and the load did not bind its
budget.

| cell | BPSK250 decoded | ρ p01 / p05 / median | BPSK250-PN decoded | ρ p01 / p05 / median |
|---|---|---|---|---|
| 5 dB | 19 | 0.892 / 0.892 / 0.972 | 14 | 0.830 / 0.830 / 0.894 |
| 10 dB | 59 | 0.871 / 0.890 / 0.965 | 32 | 0.518 / 0.847 / 0.918 |
| 20 dB | 79 | 0.881 / 0.901 / 0.978 | 44 | 0.535 / 0.827 / 0.950 |

**Bound, by the rule fixed above: 0.51** (lowest PN p01, 0.518, rounded down). With 14–44 decodes
per cell each p01 is that cell's minimum, one frame each at 10 and 20 dB; p05 sits at 0.83–0.85.
The shipped control reads 0.871–0.892, above its 0.50.

**Unexplained, and now the open question for BPSK250:** behind this 500 Hz filter the PN arm
decodes about half as often as the shipped arm (14 / 32 / 44 vs 19 / 59 / 79), veto off on both. F1's
production-entry parity had no filter column, so it could not have seen this. f9 decodes through
`receive_with_fec_mode_timeout`, not the production entry, so this is not yet a parity verdict: the
F1 rerun with the full template adds a paired column behind `filter 1250-1750` (n = 600, δ = 0.03),
and that column decides it.

**F1 rerun with the full template (pre-registered 2026-10-07, before any output).** The candidate
plugin now publishes `BPSK250-PN`'s template: threshold 0.315 (decision 23), grid ±20 Hz (F2),
delivered-frame bound 0.51 (f9); the slow PN rungs still publish none. Receiver d9031766 plus that
template. Same harness, rule and columns as F1 (n = 600 paired, δ = 0.03, payload 16–200 B, cliff
fixed at −5 dB), plus a sixth column, **`moderate_f1` 8 dB behind a brick-wall 1250–1750 Hz filter**
over signal and noise (the SNR is set before the filter, as in every other column; the filter is
`common::filter::band_limit`, the same function f9 uses). A FAIL in the filter column fails F1.

**F1 rerun result (1e83be8d, n = 600 paired): FAIL, on the two offset columns.**

| column | shipped | PN | discordant | PN − shipped [95 % CI] | verdict |
|---|---|---|---|---|---|
| `moderate_f1` at the floor | 507 | 524 | 16/33 | +0.028 [+0.006, +0.051] | PASS |
| `moderate_f1` 8 dB | 558 | 572 | 7/21 | +0.023 [+0.006, +0.041] | PASS |
| `moderate_f1` 8 dB, +50 Hz | 537 | 508 | 60/31 | −0.048 [−0.079, −0.017] | **FAIL** |
| `moderate_f1` 8 dB, −50 Hz | 543 | 517 | 52/26 | −0.043 [−0.072, −0.015] | **FAIL** |
| AWGN −5 dB (the cliff) | 534 | 550 | 15/31 | +0.027 [+0.005, +0.049] | PASS |
| `moderate_f1` 8 dB, filter 1250–1750 | 525 | 532 | 10/17 | +0.012 [−0.005, +0.029] | PASS |

The cliff column reproduces the first F1 exactly (534 / 550), so the template changes nothing at
0 Hz in AWGN. The filter column passes, so f9's 2:1 decode gap behind the same mask does not appear
at the production entry. The offset loss is new: on the same receiver without the PN template, the
slow PN rungs read 599 / 599 at ±50 Hz (F5). **Next, before any mechanism is written down:** the
same two columns on 755fa457 (the same receiver, `BPSK250-PN` publishing no template), so a loss
that disappears there is the template path's (veto or onset ranking), and one that stays is not.

**A/B on 755fa457 (no PN template), n = 600:** +50 Hz shipped 537, PN **572**, discordant 4/39,
+0.058 [+0.037, +0.079] PASS; −50 Hz shipped 543, PN **570**, discordant 8/35, +0.045 [+0.024,
+0.066] PASS. Shipped is identical in both runs (its template did not change); PN without its
template reads the same at ±50 Hz as at 0 Hz (572). **The whole offset loss is the template path's:**
publishing `BPSK250-PN`'s template costs 64 / 53 of 600 PN frames at ±50 Hz. Which part (the veto
at 0.315, its ±20 Hz grid around the settled carrier, or the onset ranking) is not yet separated;
until it is, the flag-day template is not accepted, and decision 23's threshold stands only as
derived, not as validated off-frequency.

**Reproduced, and located in acquisition.** The +50 Hz column rerun on a build whose header reports
both vetoes active (`F1_PRINT_SEEDS=1`) gives the same 537 / 508, discordant 60/31, so the result
is deterministic; PN's 60 losses spread over the whole seed range. `f5_diagnose_discordant_seeds`
(`F1_COLUMN=2`, seeds 1–40): on every PN-only loss the gathered burst holds the whole frame (about
10 000 samples of lead) and `ota_decode_burst` returns no payload; cut at the true onset ± 1 000
samples, 3 of 5 then decode (seeds 28, 36, 38) and 2 still fail (7, 16). The loss is not gathering.
That a shorter lead rescues most of them points at the template path's choice among onsets (the
ranking, or the veto applied per candidate) more than at the threshold alone; **not yet shown**.

**Veto vs ranking (pre-registered 2026-10-07, before any output).** `set_preamble_veto_gate(false)`
(instruments only; `the_veto_gate_switch_accepts_what_the_veto_would_refuse`, sabotage-verified) makes
the veto compute ρ and count what it would refuse while rejecting nothing; the onset ranking, which
ranks and never thresholds, is unchanged. F1 at ±50 Hz with `F1_VETO_GATE=off` on the PN arm only,
n = 600. Reading: PN recovering to within the no-template run's CI (572 / 570) lays the loss on the
veto; PN staying at 508 / 517 lays it on the ranking; anything between, on both, in proportion.

**Result: the veto, all of it.** With the PN arm's gate off, +50 Hz reads shipped 537, PN **572**,
discordant 4/39, +0.058 [+0.037, +0.079]; −50 Hz shipped 543, PN **570**, discordant 8/35, +0.045
[+0.024, +0.066]: both identical to the no-template run, so the ranking contributes nothing. On the
PN-only losses of seeds 1–40 (gate on) PN's veto refused about twice as many onsets as the shipped
veto on the same frame (41 / 18, 39 / 24, 37 / 19, 56 / 26, 31 / 16), and the ranking produced no
decode on either arm. **Why** PN's ρ falls under 0.315 off-frequency (settle residual against the
template's narrower coherent bandwidth, or the threshold itself) is the next measurement: the true
onset's ρ at 0 and ±50 Hz on both templates. Until then decision 23's 0.315 is not validated for
`BPSK250-PN`, and the flag-day template stays unaccepted.

**Where the veto loses PN frames (2026-10-07).** `veto_probe` (instruments: the same settle and ρ
steps `acquire_at_onset` runs) at the TRUE onset, 8 dB fading, 60 seeds: the settle lands within
±1 Hz at ±50 Hz on both arms, and PN's ρ is healthy (p10 0.60 vs 0.315; 2 of 60 under at +50 Hz,
0 at the true frequency). So neither the settle residual nor the threshold at the right onset is the
cause. `f1_rho_by_onset_shift` (+50 Hz) moves the judged onset instead:

| shift (samples) | −128 | −96 | −64 | −32 | 0 | +32 | +64 | +96 | +128 |
|---|---|---|---|---|---|---|---|---|---|
| shipped ρ p50 (n < 0.40) | 0.835 (5) | 0.855 (7) | 0.868 (6) | 0.886 (5) | 0.887 (5) | 0.858 (5) | 0.860 (4) | 0.784 (6) | 0.793 (5) |
| PN ρ p50 (n < 0.315) | 0.189 (60) | 0.205 (60) | 0.836 (4) | 0.859 (3) | 0.857 (2) | 0.236 (55) | 0.195 (60) | 0.189 (60) | 0.196 (60) |

**Mechanism:** the veto's timing search spans about two symbols past the judged onset. The periodic
`--++` correlates at any alignment inside its preamble; PN-63 only within that span (−64…0 here).
Phase 2 settles on a grid of `PHASE2_STEP_MULTIPLIER` × 1 symbol = 128 samples, so on many frames no
grid onset lands inside PN's acceptance, every settle is refused, no correction is produced and the
frame is lost. At 0 Hz phase 1 decodes without the veto and the fine scan's one-symbol step always
lands inside, which is why only the offset columns fail.

**Confirmation run (pre-registered, before any output):** `set_phase2_veto_reach` (instruments)
lets phase 2's veto search one more coarse step past each grid onset, covering the grid's gaps at
an unchanged settle count. F1 ±50 Hz, n = 600, `F1_VETO_REACH=on` on the PN arm only. Reading: PN
at or within the no-template run's CI (572 / 570) confirms the mechanism and makes the reach the
fix candidate; PN still near 508 / 517 refutes it.

**Result: confirmed.** +50 Hz shipped 537, PN **563**, discordant 12/38, +0.043 [+0.020, +0.066]
PASS; −50 Hz shipped 543, PN **566**, discordant 10/33, +0.038 [+0.017, +0.060] PASS. The reach
recovers 55 of 64 and 49 of 53 lost frames; both CIs overlap the no-template run's. The remainder is
the veto's ordinary cost (frames whose ρ is low even at the true onset, 2 of 60 in the probe).

**Not yet a fix.** Two ways to make it one, each with a cost that must be measured first:
- **Reach** (as tested): phase 2's veto searches one more coarse step. Settle count unchanged, but
  the veto's timing search grows from about two symbols to six, which raises the noise ceiling the
  0.315 was derived against (F2, margin already 1.07): F2's noise column must be re-measured on the
  wider search before this ships.
- **Finer grid for PN:** phase 2 settles every two symbols instead of four, inside PN's acceptance.
  The veto's search is unchanged, so 0.315 and the 0.51 bound stand as derived, but phase 2's settles
  double, and receive cost at ±50 Hz (REQ-PHY-03, the 9 s ACK window on the Pi) must be re-measured.

**The reach shipped (44632e76)** after F2 was re-measured on the wider search: binding interferer
0.304 unchanged, SSB noise ceiling 0.152 → 0.157 (shipped `--++` 0.205 → 0.210). x86 receive cost,
BPSK250-PN at +50 Hz: 772 ms against 759 ms without. The reach applies to every template, so the
shipped arm moves too.

**F1 rerun on 44632e76 (n = 600 paired, same rule, columns and seeds as the 1e83be8d run): PASS on
every column.**

| column | shipped | PN | discordant | PN − shipped [95 % CI] | verdict |
|---|---|---|---|---|---|
| `moderate_f1` at the floor | 507 | 524 | 16/33 | +0.028 [+0.006, +0.051] | PASS |
| `moderate_f1` 8 dB | 559 | 572 | 7/20 | +0.022 [+0.005, +0.039] | PASS |
| `moderate_f1` 8 dB, +50 Hz | 541 | 563 | 14/36 | +0.037 [+0.014, +0.060] | PASS |
| `moderate_f1` 8 dB, −50 Hz | 544 | 566 | 9/31 | +0.037 [+0.016, +0.057] | PASS ¹ |
| AWGN −5 dB (the cliff) | 534 | 550 | 15/31 | +0.027 [+0.005, +0.049] | PASS |
| `moderate_f1` 8 dB, filter 1250–1750 | 526 | 532 | 10/16 | +0.010 [−0.007, +0.027] | PASS |

¹ The disk filled after this column printed its verdict line; its exit status and the log's tail
were lost. The verdict is the harness's printed line, not an rc.

Both arms printed `preamble veto active`. PN is better than `--++` on five columns; behind the filter
the two are level (the CI spans 0, its lower bound −0.007 clears δ = −0.03). The reach moved the
shipped arm by at most four frames per column (+50 Hz 537 → 541, −50 Hz 543 → 544).

**Caution recorded:** an A/B built in a second worktree with `CARGO_TARGET_DIR` shared with the main
checkout left the main test binary linked against the worktree's `bpsk-plugin` until that crate was
touched, and four diagnostic runs (none recorded here) silently measured the no-template receiver.
F1 now prints, per arm, whether the receive path has a veto (`preamble_veto_active`), so a run states
which receiver it measured.

**Lesson for any long preamble** (QPSK's parked longer preamble, pilots): length buys energy only
up to the channel's coherence time; past it, combine in power.

## Cost

- Airtime +1.49 % on the rungs that take it.
- One generator change, ~30 demod uses following one constant, the AFC-window decoupling, three
  rewritten tests, frame geometry, and the acceptance rows that name geometry.
- Every BPSK250 ρ constant re-derived; the veto, the calibration (#1060) and the onset ranking
  re-measured.
- Receive cost on the slow rungs rises with `preamble_samples` (F1c measures it).
- The replay corpus is dead until re-recorded (it already is: it predates #1148); F6 replaces its
  pins first.
- Schedule: past M2's 2026-10-28 target. M3 waits on it (decision 22).

## Consumer

`BpskPlugin` modulate/demodulate (every BPSK frame on SL2–SL5 and the station's `[modem] mode`
traffic), `BpskPlugin::preamble_template` → `ModemEngine::build_preamble_veto` →
`ranked_fallback_onsets` and the veto in `acquire_at_onset`, reached from `server.rs`'s
`ota_decode_burst`. Found by `grep -rn "preamble_bits\|preamble_template\|build_preamble_veto" plugins/bpsk/src crates/openpulse-modem/src`.

## Prior art

`openpulse-dsp::preamble` (`PreambleType::Pn63`, `pn_sequence(63, 0x45)`, x⁶ + x + 1), already
transmitted by the pilot plugin (`plugins/pilot/src/frame.rs:76`); MFSK16's Costas sync (aperiodic,
in production); QPSK's designed 16-symbol sequence; `demod_parity.rs` (the parity harness, whose
`symbols_to_bits` already does the symbol-domain pre-image) and
`preamble_rho_fade_and_filter_probe.rs` (f7–f13). Found by
`grep -rn "Pn63\|fn pn_sequence\|fn symbols_to_bits" crates plugins --include=*.rs` and the #1062
thread.

## Twins

The GPU modulator (`preamble_bits` is its source too, and default CI does not compile it) and the RRC
path. The 8PSK and 64QAM plugins share the `PREAMBLE_SYMS` shape but are off `hpx_hf` and unchanged;
QPSK is decided by F0. Found by `grep -rln "PREAMBLE_SYMS" plugins crates --include=*.rs`.
