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
