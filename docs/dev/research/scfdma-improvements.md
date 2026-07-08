---
project: openpulsehf
doc: docs/dev/research/scfdma-improvements.md
status: living
last_updated: 2026-07-08
---

# SC-FDMA waveform improvements — research

A prioritized, code-grounded list of ways to further improve the SC-FDMA waveform. From a research
pass over `plugins/scfdma/`, the shared DSP crate, the FEC crates, and the SC-FDMA profile rungs.
No code was changed. Effort is S/M/L.

## Status (2026-07-08, second pass)

Acting on P2/P3/P4 uncovered a **bug**, not a tuning opportunity: the DFT channel estimator was wrong on
*every* frequency-selective channel. It is replaced (`channel::DelayCe`) and P3/P4 are folded into the
same change. See **Resolved** below. P1, P5, P6, P7, P8 remain open; P2's CPE tracking is still open
(its non-causal-smoothing half is superseded — the two-pass front end now exists).

## Resolved — the DFT-CE defect (2026-07-08)

`dft_ce_estimate` took the 13 pilot-comb LS observations, IDFT'd them, kept the first `l_max = 9` "CIR
taps", and re-evaluated with kernel `exp(-j2π(rel−offset)·l/total_sc)`. Two defects compounded:

1. Its delay grid is `N_FFT / (P × pilot_spacing) ≈ 3.94 samples`, because the comb spans only the 65
   occupied subcarriers, not all 256 FFT bins. A channel delay between grid points leaks across every
   tap; truncating the tap set then throws that leakage away. **`deramp_timing` runs first and re-centres
   the impulse response, so the post-deramp delays are essentially always off-grid.**
2. Taps `l > P/2` are *negative* delays; they were reconstructed as large positive ones.

Measured, on a known noiseless two-ray response `1 + a·z^-d`:

| | d=1 | d=2 | d=4 (on-grid) | d=8 |
|---|---|---|---|---|
| DFT-CE channel-estimate MSE | −16.5 dB | −14.3 dB | −41.5 dB | −36.5 dB |
| delay-basis MSE | −66 dB | −71 dB | −74 dB | −69 dB |

End-to-end, a **noiseless, static, inside-the-cyclic-prefix** two-ray channel (`d ≤ 8`, CP = 32) had a
hard BER floor: QPSK 0.20, 8PSK 0.26, 16QAM 0.36. Every SC-FDMA rung decoded 2–7 % of 60 Watterson
`good_f1` frames, flat across every SNR from 8 to 32 dB — a floor no SNR could move, which the tests
recorded as "correct and by design". It was not.

**Replacement.** `DelayCe` fits `L ≤ 13` complex taps at fixed sample delays spaced 5/3 samples,
symmetric about zero, evaluated at the true period `N_FFT`. Three things make it work:

- **f64 construction.** Over a 65-subcarrier aperture the steering vectors of adjacent delays are nearly
  collinear (`AᴴA` off-diagonals reach 0.98 of the diagonal); the normal equations lose every bit of an
  f32 mantissa. Only the intermediate needs the range — the product `B·pinv` is well conditioned.
- **A Wiener ridge with an exponential delay-power prior.** Plain least squares on that basis amplifies
  pilot noise: it cost 4–6 dB of AWGN frame success while uncoded BER and channel-estimate MSE both
  looked *fine* (the damage showed up only in soft-LLR confidence). `ridge_j = σ²_h·Σw/(w_j·P_ch)` with
  `w_j = exp(−|τ_j|/1.5)`. A **flat** prior costs ~6 dB of AWGN at reach ±10; the exponential prior
  removes that cost entirely, so reach and AWGN performance stop trading against each other. A mode with
  fewer pilots loses *reach*, not resolution: spreading 6 taps across the full ±10 samples makes the basis
  unresolvable for SCFDMA26's aperture and costs ~2 dB near its floor.
- **A σ² that no channel estimate can bias.** `pilot_comb_noise_var` reads σ² off the comb's
  out-of-delay-window IDFT taps (an orthogonal transform ⇒ those taps are noise-only), with a
  `NOISE_GUARD_TAPS` guard band; `pilot_diff_noise_var` reads it off CPE-removed adjacent-symbol pilot
  differences. The two fail in opposite directions (delay spread vs Doppler/CFO), so the front end takes
  the **minimum** of the frame-averaged pair. Measured: `σ̂²/σ²` is constant to 0.01 dB over 20 dB of SNR.

**Results** (`crates/openpulse-modem/tests/scfdma_ce_sweep.rs`, 60 frames/point, soft-concatenated FEC):

| | old DFT-CE | delay-basis Wiener CE |
|---|---|---|
| static two-ray BER (sum over the d×a grid, lower better) | 10.4 | **1.90** |
| AWGN frame success (sum over 6 rungs × 9 SNRs, max 54) | 39.00 | **41.32** |
| Watterson good_f1 frame success (sum over 6 rungs × 7 SNRs, max 42) | 1.58 | **9.19** |

AWGN 90 % floors move: SCFDMA52-8PSK 8→6 dB, SCFDMA52-16QAM 10→8 dB; SCFDMA52-32QAM, -64QAM and
-64QAM-P4 unchanged. Watterson `good_f1` frame success at 32 dB, per rung (old → new): SCFDMA26-32QAM
0.05→0.12, 52-8PSK 0.03→0.32, 52-16QAM 0.03→0.27, 52-32QAM 0.07→0.30, 52-64QAM-P4 0.03→0.32, 52-64QAM
0.03→0.28. The ladder's AWGN floors in `profile.rs` are therefore *conservative*, not optimistic — no
re-calibration is required to stay correct, though SL11/SL12 now have ~2 dB of headroom to reclaim.

**Folded in:** P3 (frame-mean σ² — a per-symbol σ² mis-weights whole symbols against each other in the
soft-Viterbi metric and in the majority-protected length prefix) and the rest of P4 (both GPU paths now
enter the shared `FrameFront::from_spectra`, so they can no longer skip `deramp_timing`; the GPU hard
path gained the `/alpha_avg` de-bias).

## Ground truth (corrections to earlier assumptions)

- **Pilots are already comb-type in frequency** (every 5th subcarrier in *every* symbol,
  `plugins/scfdma/src/channel.rs:13`). "Block pilots" only described the dropped `SCFDMA52-LP`
  demonstrator. The real pilot questions are the *time-direction* processing of the comb (currently a
  causal EMA only) — not comb-vs-block.
- **Demodulation is batch** (`demodulate.rs:134`) — the whole frame is in memory, so non-causal
  (two-pass / centered) processing is essentially free; the current one-sided EMA does not exploit it.
  *(A two-pass front end, `FrameFront`, now exists; the EMA is still one-sided.)*
- **A dedicated frequency-acquisition stage already exists and is unused**:
  `openpulse-dsp/src/freq_acquire.rs::acquire()` (qdetector-style joint timing+CFO+phase+gain), with no
  caller anywhere.
- **AGC is already engine-level** (`openpulse-modem/src/engine.rs`), so "no AGC" is addressed upstream.
- **Adaptive pilot density is dead plumbing** (`lib.rs:82`) — no engine caller, and pilot spacing is
  wire format, so it cannot activate without a negotiation protocol; it is a latent footgun.
- **Per-subcarrier bit-loading is inapplicable** — the DFT spread makes every data symbol see the
  *average* channel; there is no per-SC symbol to load. The codebase's answer is the narrowband
  SCFDMA26 family (already shipped).

## Prioritized improvements

| # | Improvement | Benefit | Effort | Risk | Building block |
|---|---|---|---|---|---|
| **P1** | **Wire in `freq_acquire::acquire()`** for one-shot joint timing+CFO, replacing/augmenting `find_sync_offset` | Single-shot acquisition over the full ±60 Hz window; removes the ±4 Hz own-sync fragility; frees phase+gain to seed CE | M | Low–Med | exists, unused (`freq_acquire.rs`) |
| **P2** | **Common-phase-error (CPE) removal per symbol + non-causal CE smoothing** — de-rotate `raw_h` by the pilot-mean phase; replace the causal EMA with a centered / forward-backward pass | ~1–2 dB on slow-fading dense modes; adds the intra-frame CFO/Doppler tracking the code currently lacks | S/M | Low | `pilot_tracker.rs`, `doppler_tracker.rs` |
| ~~P3~~ | ~~Smooth the noise-variance estimate across symbols~~ | **Done** — frame-mean σ² in `FrameFront`; the effect was far larger than "a few tenths dB" | S | Very low | — |
| ~~P4~~ | ~~Fix the hard-demod MMSE amplitude bias + GPU-path divergence~~ | **Done** — CPU hard path in PR #679, GPU paths folded into the CE change | S | None | — |
| **P5** | **Second-pass decision-directed CE** — re-spread sliced symbols as virtual pilots → 100% density for a 2nd CE/equalize pass | ~1–2 dB under frequency-selective fading; removes the CE-noise floor | M (L for full turbo-CE) | error propagation (gate on 1st-pass quality) | `IterativeDecoder` trait exists |
| **P6** | **LDPC on the dense rungs (SL8–SL11)** — swap `SoftConcatenated` for `LdpcHighRate`/`Ldpc`, keep the interleaver | ~1–3 dB vs RS soft-concat at the same rate; lowers the ladder floors; exploits the max-log LLRs already emitted | M (calibration, not DSP) | Low–Med | `LdpcCodec` + engine plumbing exist |
| **P7** | **Frequency-domain iterative block DFE (IBDFE)** after MMSE — cancel residual ISI at spectral notches over 1–2 iterations | 1–2 dB on frequency-selective (Watterson) channels for 16/64QAM | M/L | Med (convergence < ~10 dB) | ~100 lines; `dft`/`idft` plans in scope |
| **P8** | **TX windowing + CP tuning** — raised-cosine edge windowing (WOLA); optionally halve CP 32→16 samples | Windowing = adjacent-channel/regulatory hygiene; CP-16 = +5.9% throughput | S (S but wire-incompatible for CP) | Low (windowing); Med (CP) | — |

**Deprioritized:** π/2-BPSK pilot shaping (value is PAPR of TDM pilots; PAPR work was dropped) and
TDM DMRS pilot symbols (the per-symbol comb tracks Doppler strictly better; not worth a wire break).

## Weak / fragile spots in the current demod path

1. ~~Hard-path MMSE amplitude bias~~ — fixed (PR #679).
2. ~~GPU paths skip `deramp_timing` + CE smoothing~~ — fixed; both enter `FrameFront::from_spectra`.
3. **Causal EMA CE lags phase under residual CFO/Doppler** — a ~1 Hz residual never trips the jump-reset
   yet steadily rotates the estimate; **no intra-frame CFO/CPE tracking at all** (P2, still open).
4. ~~Watterson fading remains a weak regime~~ — **two sync bugs** (PRs #688, #689), not a waveform limit.
   (a) `find_sync_offset` took the matched-filter *argmax*, which on a two-ray channel sits on the delayed
   ray about half the time; a late FFT window start pulls the next symbol in, and the cyclic prefix only
   protects an **early** start. (b) That argmax was over the *unnormalised* score, so a faded preamble
   lost to a higher-energy data window. Together they took `good_f1` frame success from 9.19 to 31.40
   (of 42) with the AWGN sweep bit-for-bit unchanged. What is left is the exact-null case (equal-power
   rays erase a subcarrier outright, and the DFT de-spread smears the erasure over every symbol) plus
   deep-fade outage — *that* is P7 / Memory-ARQ territory.
5. **Sync fragility** — ±4 Hz own tolerance; `estimate_cfo_hz` aliases beyond ±13.9 Hz; the `rho=0.15`
   detection floor is untested against fading/impulse noise.
6. **Adaptive pilot density can't deploy** (no negotiation) — wire it or delete it (footgun).
7. **`SCFDMA52-LP` mis-decode risk** is documented but the mode stays user-selectable.
8. **`llr_noise_var` ignores channel-estimate error** — `mmse_llr_noise_var` models only the post-MMSE
   additive noise, while CE error adds 37–48 % to the true post-despread error variance. The LLRs are
   therefore ~1.5 dB over-confident, by a slightly SNR-dependent amount. Fixable from the solver:
   add `trace(R Rᴴ)·σ²_h / total_sc`.
9. ~~`combine_llrs_weighted` double-counts~~ — **fixed** (`combine_llrs_map`, PR #686). The engine's
   `1.0/mean_abs` weight proxy applied σ⁻² on top of the σ⁻² already inside a calibrated LLR. Worth
   0.75 dB of threshold on a graded 0/−4/−8 dB HARQ attempt set.
10. **Most plugins emit uncalibrated LLRs.** SC-FDMA and OFDM divide by their estimated σ²; 64QAM passes
   `noise_var = 1.0`, BPSK/QPSK emit raw correlations, and the trait default emits ±1.0. Their
   `mean(|LLR|)` is flat in SNR (measured: 1.00× across 8→24 dB), so a per-attempt reliability weight
   cannot be recovered from them at all. Calibrating them — `estimate_decision_noise_var` already exists
   — is worth ~1 dB of HARQ combining gain on graded attempt sets.

## Do next (top 3)

Order revised twice. The flat Watterson curve was **not** notch smearing (falsified: the selective-vs-flat
gap was 0.50 at 32 dB and 0.51 at 60 dB — a noise-enhancement mechanism cannot survive the removal of
noise). It was **two sync bugs**:

- **PR #688** — the matched filter's argmax lands on the *delayed* ray half the time (late FFT window).
- **PR #689** — the argmax is over the *unnormalised* score, which prefers a high-energy window. When the
  preamble itself is in a fade, that hands the frame to a data-region window that merely shares the pilot
  comb. This is what looked like "fade dynamics" and was slated as P2: **flat**-fade success at 60 dB (no
  noise, no selectivity) was 0.47 for SCFDMA52-16QAM at 0.5 Hz Doppler; it is now 0.93. P2's premise is
  therefore gone — the causal EMA's lag was measured to cost *nothing* (disabling `smooth_ce` entirely
  left the flat-fade numbers bit-identical).

1. **#8 — LLR calibration** (S): `mmse_llr_noise_var` models only the additive noise. It is missing the
   channel-estimate error (`ε²_k = σ²_h · Σ_j |recon[k·P+j]|²`, read straight off `CeSolver::recon`) and
   the residual-ISI term `var(α)` — on a selective channel at high SNR the LLRs are over-confident by up
   to ~20 dB, which is what lets a few smeared symbols poison the soft Viterbi. Also a prerequisite for
   P7's feedback-reliability estimate.
2. **P6 — LDPC on the dense rungs** (M): no new DSP — the codec + engine plumbing exist; 1–3 dB of
   ladder-floor improvement, on *every* channel including AWGN.
3. **P2 — CPE removal + non-causal CE smoothing** (S/M): demoted. Its motivating measurement was the sync
   bug. Re-measure before building: what remains of the Doppler dependence is intra-symbol Doppler (ICI),
   which no channel-estimate smoothing addresses.

**P7 (IBDFE) after those.** Its honest headroom, from a 20 000-draw Monte-Carlo of the MMSE-vs-matched-
filter bound (`SINR_mmse = N/Σ(1/(1+γ_k)) − 1` vs `SINR_mfb = (1/N)Σγ_k`, two-ray d=4 over the 52 data
SCs): median gain 2.7 dB at 8 dB SNR rising to 3.8 dB above 24 dB, of which a 2-iteration soft-feedback
implementation captures 60–80 %. At 32 dB on `good_f1` it buys ~nothing (MMSE outage there is 10⁻⁴); its
real domain is the 8–20 dB window and `moderate_f1`, where a 1 ms spread puts 2–3 notches in band and the
current MMSE has a noiseless BER floor of 0.31. Building it before P2 and #8 would tune it on top of
mis-calibrated LLRs and test as "barely helps".

## Method note

The bug was invisible to every metric that was being watched. Uncoded BER, channel-estimate MSE on a
*flat* channel, and all 58 unit tests were happy. Two experiments found it:

1. **Take the noise away.** A static two-ray FIR inside the cyclic prefix, no Doppler, 90 dB SNR. A
   receiver that cannot decode a noiseless in-CP channel has a bug, full stop — there is nothing to
   trade off. This turned "SC-FDMA is unsuitable for HF fading" into a five-line reproduction.
2. **Swap one component, hold the rest.** With the new estimator behind an env switch and everything
   else identical, the AWGN regression moved with the CE and nothing else — which is what said the
   Wiener ridge (not the delay basis) was the missing piece.

A metric that reads "fails at every SNR" is a bug signature, not a performance number.

> Any mode/FEC change alters `SessionProfile::fingerprint()` (an intentional interop version break);
> RX-only fixes (P2/P3/P4) and floor retunes do not. Re-run `snr_floor_calibration` after P6/P7.
