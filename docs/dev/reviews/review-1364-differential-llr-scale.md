---
project: openpulsehf
doc: docs/dev/reviews/review-1364-differential-llr-scale.md
status: resolved
last_updated: 2026-09-21
---

# Design review — #1364, `differential_llr_scale`'s low-SNR floor

## Prompt

Fable was asked to **falsify** a diagnosis and a candidate fix before implementation, not confirm
them. Seven numbered attack points went out with the apparatus: the measured floor (a noise-only
attempt emitting LLRs of std 1.459 at every σ from 0.1 to 2.0, taken through the shipped function),
my candidate `2Â²/⟨cross²⟩` with `Â² = √(⟨dot²⟩ − ⟨cross²⟩)`, and its measured shortfall. The hardest
question was put first and flagged as unverified: *"is the identity `E[dot²] − E[cross²] = A⁴` correct
for this model, given dot and cross are formed from adjacent symbols and share `z_{k−1}`? If it is
wrong the candidate is unsound regardless of the numbers."*

## Verdict

**Build with changes.** The candidate is sound; my *framing* of the defect was wrong in two ways that
would have gone into the commit message and the docs.

1. **The headline was backwards.** At A = 0 the shipped estimator returns **exactly the `1/σ²` its
   contract promises** — the measured std of √2 ≈ 1.414 is that contract honoured, not violated. The
   defect is the **contract**: `1/σ²` is the high-SNR *limit* of the true DBPSK LLR slope, and the
   true slope vanishes with the signal. Regressed against the exact pairwise LLR, the old formula is
   **8× over-confident at −12 dB** (slope 0.94 against 0.11). My candidate turns out to be the
   Gaussian-approximation LLR `2μ_x/s²` with an unbiased fourth-moment `Â²` — a principled
   derivation rather than the patch I presented it as.
2. **My 0.04 target was a category error.** `0.0398 = σ₁²/σ₂²` is a **scale** ratio; #1364's 0.21 and
   the gate are **magnitude** ratios, and under exact `1/σ²` with the true σ the magnitude ratio in
   that regime is **0.20**. So the shipped 0.21 was its contract, not a miss. The candidate reaches
   the correct Gaussian target: 0.040 measured against shipped 0.149.
3. **Q1, the identity:** correct for iid circular noise (agreement < 1 % at five (A,σ) points); the
   shared `z_{k−1}` widens the sample spread, not the expectation. **But it breaks under lag-1 noise
   correlation**, which `cancel_crossfade_isi` induces at ρ = −1/3 — returning `2ρ²v² = 0.889v²` at
   A = 0 and restoring the defect. That couples this fix to #1361's open question, and is now pinned
   by `differential_llr_scale_assumes_iid_noise` rather than left implicit.
4. **Q2, the residual floor is sampling noise:** confirmed, `∝ N^(−1/4)`, with `P(scale = 0) ≈ 0.5`.
   My "still flat across σ" observation was a tautology — any scale-invariant estimator is
   σ-independent at A = 0, and a fixed seed makes the five values identical by construction.
5. **No blind estimator does better.** At A = 0 the score for `A²` equals the score for `v`, so the
   Fisher information is singular — the known blind-SNR degeneracy. Side information buys a constant,
   not a rate. So the floor is fundamental, not a shortcoming of the candidate.
6. **Q5, why it was never caught:** confirmed, plus a reason I had not seen. The old fixture
   synthesised `dots`/`crosses` from the asymptotic model (omitting `n·conj(n)`) and swept only
   10/20 dB — but its `expected = 1/sigma2` was *also* the wrong target, so even a correct fixture
   would have measured against the wrong reference. The "safe direction (under-confident)" doc claim
   is backwards relative to the true LLR.
7. **Q6, regression risk:** none on AWGN (good attempts' scales move by a common factor 1.09, so
   thresholds do not shift). Two caveats raised: a Jensen effect (`√E[A⁴] ≥ E[A²]`, ×1.41 on a
   Rayleigh envelope) meaning a within-frame-faded attempt votes louder, **measured and not
   observed** — `moderate_f1` at 6/8/10 dB is 19/23/27 new against 20/22/27 old; and `−0.0` LLRs from
   a zero scale, closed with `SCALE_FLOOR`.
8. **Q7, blast radius wider than I stated**, and the property is a **class**: every blind
   scale-invariant calibrator votes at a σ-independent magnitude on noise, with 8PSK500 at 0.44 of a
   good attempt against BPSK250's 0.009. Filed as #1425.

## Consumer

`plugins/bpsk/src/demodulate.rs:427`, the BPSK soft path — live in production, since the OTA arm
admits `FecMode::Rs` (`engine.rs:3040`) and `hpx_hf` SL2–SL5 are BPSK. Also
`apps/openpulse-testbench/src/signal_path.rs:91` (display only) and the BPSK `-RRC` modes through the
same call, whose LMS-equalised symbols carry coloured noise — no profile uses BPSK-RRC, but that is
the ρ-floor case from point 3 and is why the premise pin is a test rather than a comment.

## Prior art

`grep -rn "differential_llr_scale"` → one production caller, one unit test, one testbench display.
The existing test `differential_llr_scale_recovers_inverse_noise_var_at_any_amplitude` claimed exactly
this property and passed; reading it is what found the synthesised fixture. #687 is the original
calibration work whose contract this corrects; #832 is the decision whose gate
(`a_deeply_faded_extra_attempt_does_not_hurt`) this improves without changing.

## Twins

`psk_symbol_noise_var` is the coherent sibling and is **not** changed here: it estimates noise from
the component orthogonal to the hard decision, so its amplitude reference is a different quantity and
needs its own derivation — that is #1425, filed rather than folded in. Within this change the two
directions are twins and both are sabotage-verified: the rebuilt tests fail on the old estimator
(`signal-free vote 1.439 exceeds derived bound 0.464`) and pass on the new one, with the old formula
retained inside the test as a control asserting it votes ≈ √2.
