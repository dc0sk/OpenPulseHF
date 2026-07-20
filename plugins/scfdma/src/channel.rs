//! Pilot layout, LS channel estimation, ZF/MMSE equalization, and CFO estimation for SC-FDMA.
//!
//! Pilot layout is identical to OFDM: every 5th SC starting at first_sc+4.

use num_complex::{Complex32, Complex64};
use rustfft::FftPlanner;

use crate::params::{
    ScFdmaParams, CP, FFT_SIZE, PILOT_AMPLITUDE, SAMPLE_RATE, SC_SPACING_HZ, SYM_LEN,
};

/// Return the absolute SC indices of all pilot subcarriers for `p`.
pub fn pilot_positions(p: &ScFdmaParams) -> Vec<usize> {
    if p.localized {
        // Localized (low-PAPR-demonstrator) layout: pilots are a contiguous block at the high edge,
        // so the `n_data` data SCs form one contiguous block [first_sc .. first_pilot). The
        // contiguity adds only ~0.5 dB of PAPR reduction (the bulk of SCFDMA52-LP's win is the
        // smaller pilot COUNT, not the mapping — see the `papr_ablation` test / `SCFDMA52_LP` docs).
        let first_pilot = p.last_sc + 1 - p.n_pilots;
        return (first_pilot..=p.last_sc).collect();
    }
    if p.pilot_spacing == 0 {
        return vec![];
    }
    let mut pilots = Vec::with_capacity(p.n_pilots);
    let mut sc = p.first_sc + p.pilot_spacing - 1;
    while sc <= p.last_sc {
        pilots.push(sc);
        sc += p.pilot_spacing;
    }
    pilots
}

/// Remove the residual linear phase ramp across subcarriers (sampling-frequency /
/// timing offset) using the known real-BPSK pilots — the SC-FDMA counterpart of
/// the OFDM `deramp_timing`.
///
/// A sample-rate offset between the TX and RX clocks rotates each subcarrier's
/// phase by an amount proportional to subcarrier index (and growing with symbol
/// index). On the direct per-SC OFDM path that is benign, but SC-FDMA's DFT
/// de-spread coherently combines all subcarriers, so an uncorrected ramp smears
/// across every recovered data symbol. The pilots are real +1 and evenly spaced,
/// so the vector sum of adjacent-pilot conjugate products gives the average
/// per-pilot-step rotation; de-rotating the whole spectrum removes the ramp
/// before channel estimation. On a clean (offset-free) channel the slope is ~0
/// and this is a near-identity.
/// The localized (block-pilot) layout is handled by the same fit: its pilots are **contiguous**, which
/// is even spacing of 1, and `pilot_spacing` is already 1 for it. This used to return early on
/// `p.localized` with the reasoning "no evenly-spaced pilots to fit a ramp" — that premise is wrong,
/// and skipping the fit cost `SCFDMA52-LP` any tolerance to where the frame sits: measured 2026-07-20,
/// it decoded **only** with the frame at sample 0 of the buffer and failed at a **one-sample** offset
/// (1/12 embedded positions, versus 12/12 for `SCFDMA52`). A real receiver never has the frame at
/// offset 0, so the mode could not work on any real capture. With the fit enabled it is 12/12.
pub fn deramp_timing(p: &ScFdmaParams, freq: &mut [Complex32]) {
    if let Some(slope) = timing_ramp_slope(p, std::slice::from_ref(&&freq[..])) {
        apply_timing_deramp(p, freq, slope);
    }
}

/// Accumulated adjacent-pilot conjugate product over `spectra`, or `None` if it is degenerate.
///
/// **Estimated over the whole frame, not per symbol, on purpose.** The timing offset is constant
/// across a frame, so summing every symbol's contribution cuts the estimator's noise by
/// `sqrt(n_symbols)`. That is what makes the fit usable for the localized (block-pilot) layout, which
/// carries only 4 pilots — 3 adjacent products — per symbol: a per-symbol estimate there is so noisy
/// that de-rotating 65 subcarriers by it is worse than not correcting at all. Measured 2026-07-20, a
/// per-symbol fit broke `SCFDMA52-LP` on AWGN at 20 dB (CRC mismatch) while fixing its frame-position
/// fragility; the frame-wide fit does both.
pub fn timing_ramp_slope(p: &ScFdmaParams, spectra: &[&[Complex32]]) -> Option<f32> {
    let pilots = pilot_positions(p);
    if pilots.len() < 2 {
        return None;
    }
    let mut acc = Complex32::new(0.0, 0.0);
    for freq in spectra {
        // De-phase the known pilot symbols first (identity for non-PN modes), leaving channel phase
        // only, so the adjacent-pilot conjugate products measure the timing ramp, not the pilot phases.
        let dephased: Vec<Complex32> = pilots
            .iter()
            .enumerate()
            .map(|(k, &sc)| freq[sc] * pilot_value(p, k).conj())
            .collect();
        for w in dephased.windows(2) {
            acc += w[1] * w[0].conj();
        }
    }
    if acc.norm_sqr() < 1e-12 {
        return None;
    }
    Some(acc.arg() / p.pilot_spacing as f32) // rad per subcarrier
}

/// De-rotate `freq` by a previously estimated `slope` (rad per subcarrier).
pub fn apply_timing_deramp(p: &ScFdmaParams, freq: &mut [Complex32], slope: f32) {
    let pilots = pilot_positions(p);
    let Some(&k_ref) = pilots.first() else {
        return;
    };
    let k_ref = k_ref as f32;
    for (k, c) in freq.iter_mut().enumerate() {
        let (sin_p, cos_p) = (-slope * (k as f32 - k_ref)).sin_cos();
        *c *= Complex32::new(cos_p, sin_p);
    }
}

/// `true` when absolute SC index `sc` is a pilot for this mode.
pub fn is_pilot(p: &ScFdmaParams, sc: usize) -> bool {
    if sc < p.first_sc || sc > p.last_sc {
        return false;
    }
    if p.localized {
        // Contiguous pilot block at the high edge.
        return sc >= p.last_sc + 1 - p.n_pilots;
    }
    if p.pilot_spacing == 0 {
        return false;
    }
    let offset = sc - p.first_sc;
    offset % p.pilot_spacing == (p.pilot_spacing - 1)
}

/// Deterministic constant-modulus pilot phase (radians) for pilot ordinal `k`, shared TX/RX.
///
/// A Zadoff–Chu quadratic phase `π·k·(k+1)/13` (root 1): its ideal autocorrelation spreads the pilot
/// comb's energy uniformly in time instead of letting the 13 equal-phase cosines peak together, which
/// is the pilot PAPR driver. Constant modulus keeps the channel-estimate division well-conditioned.
fn pilot_pn_phase(k: usize) -> f32 {
    std::f32::consts::PI * (k * (k + 1)) as f32 / 13.0
}

/// The known complex pilot symbol for pilot ordinal `k` under `p`: constant-modulus
/// [`PILOT_AMPLITUDE`], real +1 for the default modes, PN-phased when `p.pn_pilots`.
pub fn pilot_value(p: &ScFdmaParams, k: usize) -> Complex32 {
    if p.pn_pilots {
        let (s, c) = pilot_pn_phase(k).sin_cos();
        Complex32::new(PILOT_AMPLITUDE * c, PILOT_AMPLITUDE * s)
    } else {
        Complex32::new(PILOT_AMPLITUDE, 0.0)
    }
}

/// Least-squares channel estimate at each pilot SC, linearly interpolated
/// across all data SCs.
///
/// Returns estimates indexed by `sc - first_sc` (length = `p.total_sc()`).
pub fn ls_estimate(p: &ScFdmaParams, freq: &[Complex32]) -> Vec<Complex32> {
    let total = p.total_sc();
    let pilots = pilot_positions(p);

    let known: Vec<(usize, Complex32)> = pilots
        .iter()
        .enumerate()
        .map(|(k, &sc)| {
            let h = freq[sc] / pilot_value(p, k);
            (sc - p.first_sc, h)
        })
        .collect();

    if known.is_empty() {
        return vec![Complex32::new(1.0, 0.0); total];
    }

    let mut h_est = vec![Complex32::new(1.0, 0.0); total];

    let (first_pilot_rel, first_h) = known[0];
    let (last_pilot_rel, last_h) = *known.last().unwrap();

    for h in h_est[..first_pilot_rel].iter_mut() {
        *h = first_h;
    }
    for h in h_est[(last_pilot_rel + 1)..].iter_mut() {
        *h = last_h;
    }

    for window in known.windows(2) {
        let (rel0, h0) = window[0];
        let (rel1, h1) = window[1];
        h_est[rel0] = h0;
        h_est[rel1] = h1;
        if rel1 > rel0 + 1 {
            let steps = (rel1 - rel0) as f32;
            for (i, h) in h_est[(rel0 + 1)..rel1].iter_mut().enumerate() {
                let t = (i + 1) as f32 / steps;
                *h = h0 * (1.0 - t) + h1 * t;
            }
        }
    }
    for (rel, h) in &known {
        h_est[*rel] = *h;
    }

    h_est
}

/// Single-tap (flat-channel) estimate for the localized low-PAPR layout.
///
/// The block-pilot layout has no interpolation grid, so estimate one complex channel gain by
/// averaging over the contiguous pilot block and apply it to every SC.  Exact on a flat channel
/// (AWGN / mild fading); the localized mode does not attempt frequency-selective equalization.
/// Returns estimates indexed by `sc - first_sc` (length = `p.total_sc()`).
pub fn flat_channel_estimate(p: &ScFdmaParams, freq: &[Complex32]) -> Vec<Complex32> {
    let total = p.total_sc();
    let pilots = pilot_positions(p);
    if pilots.is_empty() {
        return vec![Complex32::new(1.0, 0.0); total];
    }
    let mut acc = Complex32::new(0.0, 0.0);
    for (k, &sc) in pilots.iter().enumerate() {
        acc += freq[sc] / pilot_value(p, k);
    }
    let h = acc / pilots.len() as f32;
    vec![h; total]
}

/// Spacing of the CE basis's delay taps, in samples (0.21 ms at 8 kHz).
const CE_TAP_STEP_SAMPLES: f64 = 5.0 / 3.0;

/// Most delay taps the basis ever uses. A mode with fewer pilots uses fewer taps at the same spacing
/// — it loses *reach*, not resolution. Spreading a small tap set across the full reach instead makes
/// the basis unresolvably coarse for its aperture and costs the 6-pilot SCFDMA26 modes ~2 dB near
/// their floor (measured: 0.17 → 0.62 frame success at 4 dB on SCFDMA26-32QAM).
const CE_MAX_TAPS: usize = 13;

/// Longest channel delay the full basis models, in samples: 10 = 1.25 ms at 8 kHz. Because
/// [`deramp_timing`] re-centres the impulse response on its energy centroid, the basis is two-sided
/// and covers a ~2.5 ms delay spread — past the CCIR "poor" HF profile, inside the 32-sample cyclic
/// prefix. The reach is free of AWGN cost only because of [`CE_PRIOR_TAU_RMS`]; do not widen one
/// without re-measuring the other (`cargo test -p openpulse-modem --test scfdma_ce_sweep -- --ignored`).
const MAX_CE_DELAY_SAMPLES: f64 = CE_TAP_STEP_SAMPLES * (CE_MAX_TAPS as f64 - 1.0) / 2.0;

/// Extra delay margin, in units of the pilot comb's own tap spacing, before a comb tap is treated as
/// noise. Without it the noise window abuts the channel window, and a channel tap sitting between two
/// comb taps Dirichlet-leaks straight into the "noise" set: a single ray at the window edge puts >50 %
/// of its comb energy there, so σ² saturates at an apparent ~8 dB SNR on any selective channel — which
/// over-ridges the estimator, over-regularises MMSE, and de-rates every LLR.
const NOISE_GUARD_TAPS: f32 = 1.5;

/// RMS delay of the estimator's exponential power-delay prior, in samples (≈ 0.19 ms at 8 kHz).
///
/// The prior is what lets the basis reach out to [`MAX_CE_DELAY_SAMPLES`] without paying for it in
/// AWGN: a flat prior ridges every tap equally, so widening the reach simply gives pilot noise more
/// places to hide (measured: ~6 dB of AWGN frame-success lost going from ±4 to ±10 samples). Weighting
/// each tap by `exp(-|τ|/τ_rms)` suppresses the far taps unless the data insists on them — at high SNR
/// the likelihood overrides the prior, so a two-ray channel at ±8 samples is still recovered exactly.
///
/// It is a regulariser, not a claim about the channel: the value was swept, not derived.
const CE_PRIOR_TAU_RMS: f64 = 1.5;

/// Signed delay taps, in samples, spanned by the CE basis.
///
/// Symmetric about zero: [`deramp_timing`] removes the channel's mean group delay before estimation,
/// re-centring the impulse response, so pre-cursor taps are as necessary as post-cursor ones.
///
/// A pilot comb of `P` observations supports at most `P` taps, so a mode with few pilots gets a
/// shorter basis at the same [`CE_TAP_STEP_SAMPLES`] resolution.
fn delay_taps(n_pilots: usize) -> Vec<f64> {
    let l = n_pilots.clamp(2, CE_MAX_TAPS);
    let reach = CE_TAP_STEP_SAMPLES * (l - 1) as f64 / 2.0;
    (0..l)
        .map(|j| -reach + j as f64 * CE_TAP_STEP_SAMPLES)
        .collect()
}

/// Delay, in samples, of comb tap `l` of a `n`-point pilot-comb IDFT (signed; taps past `n/2` are
/// pre-cursor). The comb samples `H` every `pilot_spacing` subcarriers, so it resolves delays over one
/// period of `span = n × pilot_spacing` subcarriers with a grid step of `N_FFT / span` samples.
fn comb_tap_delay(p: &ScFdmaParams, n: usize, l: usize) -> f32 {
    let signed = if l > n / 2 {
        l as i32 - n as i32
    } else {
        l as i32
    };
    signed as f32 * FFT_SIZE as f32 / (n * p.pilot_spacing) as f32
}

/// Estimate the per-FFT-bin noise variance σ² directly from the pilot comb of one symbol.
///
/// The `P`-point IDFT of the pilot least-squares observations is an *orthogonal* transform, so it
/// splits them into a channel part (taps whose delay is physically reachable) and a noise-only part.
/// Averaging |h′[l]|² over the unreachable taps gives σ² without reference to any channel estimate —
/// unlike a fit residual, it cannot be biased by how well (or badly) the estimator fits.
///
/// Over-reports σ² on channels with delay spread beyond the guard band (leakage), so callers should
/// take the smaller of this and [`pilot_diff_noise_var`], which fails the other way.
///
/// Returns `None` when no tap falls outside the guarded delay window (too few pilots).
pub fn pilot_comb_noise_var(p: &ScFdmaParams, freq: &[Complex32]) -> Option<f32> {
    let pilots = pilot_positions(p);
    let n = pilots.len();
    if n < 4 {
        return None;
    }
    let mut h: Vec<Complex32> = pilots
        .iter()
        .enumerate()
        .map(|(k, &sc)| freq[sc] / pilot_value(p, k))
        .collect();
    FftPlanner::<f32>::new().plan_fft_inverse(n).process(&mut h);
    let inv_n = 1.0 / n as f32;
    let grid_step = FFT_SIZE as f32 / (n * p.pilot_spacing) as f32;
    let cutoff = MAX_CE_DELAY_SAMPLES as f32 + NOISE_GUARD_TAPS * grid_step;
    let (mut acc, mut cnt) = (0.0f32, 0usize);
    for (l, tap) in h.iter().enumerate() {
        if comb_tap_delay(p, n, l).abs() > cutoff {
            acc += (tap * inv_n).norm_sqr();
            cnt += 1;
        }
    }
    if cnt == 0 {
        return None;
    }
    // var(h′[l]) = σ²_h / P, and σ²_bin = σ²_h · |pilot|².
    let sigma2_h = acc / cnt as f32 * n as f32;
    Some((sigma2_h * PILOT_AMPLITUDE * PILOT_AMPLITUDE).max(1e-9))
}

/// Estimate σ² from the pilot-observation difference between two adjacent symbols.
///
/// At HF Doppler the channel is essentially static across one 36 ms symbol, so `h_k(s+1) − h_k(s)` is
/// pure noise of variance `2σ²_h`. Unlike [`pilot_comb_noise_var`] this is immune to delay spread — it
/// never has to say which part of the channel is "real". It fails the other way (fast fading and any
/// residual carrier offset inflate it), so callers take the smaller of the two.
///
/// The bulk phase rotation between the symbols is removed first: a residual CFO of even 1 Hz turns
/// into 0.23 rad of common phase per symbol, which would otherwise swamp σ² at high SNR.
pub fn pilot_diff_noise_var(
    p: &ScFdmaParams,
    prev: &[Complex32],
    cur: &[Complex32],
) -> Option<f32> {
    let pilots = pilot_positions(p);
    let n = pilots.len();
    if n < 2 {
        return None;
    }
    let (h0, h1): (Vec<Complex32>, Vec<Complex32>) = pilots
        .iter()
        .enumerate()
        .map(|(k, &sc)| {
            let pv = pilot_value(p, k);
            (prev[sc] / pv, cur[sc] / pv)
        })
        .unzip();
    let rot = h1
        .iter()
        .zip(h0.iter())
        .fold(Complex32::new(0.0, 0.0), |acc, (&b, &a)| acc + b * a.conj());
    let derot = if rot.norm_sqr() > 1e-20 {
        (rot / rot.norm()).conj()
    } else {
        Complex32::new(1.0, 0.0)
    };
    let sum: f32 = h1
        .iter()
        .zip(h0.iter())
        .map(|(&b, &a)| (b * derot - a).norm_sqr())
        .sum();
    // E|h₁ − h₀|² = 2σ²_h; one phase parameter was fitted out, hence n−1 degrees of freedom.
    let dof = (n - 1).max(1) as f32;
    let sigma2_h = sum / (2.0 * dof);
    Some((sigma2_h * PILOT_AMPLITUDE * PILOT_AMPLITUDE).max(1e-9))
}

/// Pilot-to-subcarrier channel interpolator on a physical delay basis.
///
/// Fits the `P` least-squares pilot observations with `L` complex taps at fixed *sample* delays — the
/// physically meaningful basis — and evaluates the fit at every occupied subcarrier.
///
/// This replaced a DFT-CE (IDFT of the pilot comb → keep the first `l_max` taps → re-evaluate).
/// That estimator's delay grid is `N_FFT / (P × pilot_spacing) ≈ 3.94` samples wide, because the pilot
/// comb spans only the 65 occupied subcarriers rather than all 256 FFT bins. Any channel whose delays
/// fall between grid points leaks across every tap, and truncating the tap set then discards that
/// leakage: measured channel-estimate MSE on a noiseless two-ray channel was **−10 to −17 dB** for
/// 1–2-sample delays (the post-`deramp_timing` regime) against **−60 dB** here. The dense QAM rungs
/// could not decode a static, in-cyclic-prefix, *noiseless* two-ray channel at all.
///
/// The price of the physical basis is conditioning: over a 65-subcarrier aperture, steering vectors
/// for adjacent integer delays are nearly collinear (`AᴴA` off-diagonals reach 0.98 of the diagonal),
/// so an unregularised least-squares fit amplifies pilot noise by several dB. [`DelayCe::solver`]
/// therefore builds a **Wiener** fit — a ridge scaled by the measured noise-to-channel-power ratio,
/// which is the MMSE estimator under a flat delay-power prior. The ridge is what keeps the AWGN
/// channel-estimate MSE at (or below) the old DFT-CE's while the basis fixes the selective channels.
pub struct DelayCe {
    total: usize,
    pilots: Vec<usize>,
    pilot_values: Vec<Complex32>,
    taus: Vec<f64>,
    /// `P × L`, row-major.
    a: Vec<Complex64>,
    /// `AᴴA`, `L × L` row-major.
    ata: Vec<Complex64>,
    /// Ridge floor: `1e-6 · tr(AᴴA)/L`, keeping the near-collinear normal equations invertible even
    /// when a short frame under-reports σ². Without it the fit can reach a pilot-noise gain of ~18 dB.
    lambda_floor: f64,
    /// `1 / w_j` for the exponential power-delay prior `w_j ∝ exp(-|τ_j|/τ_rms)`, `Σ w_j = 1`.
    prior_inv: Vec<f64>,
    /// `total × L`, row-major.
    b: Vec<Complex64>,
}

/// A [`DelayCe`] specialised to one frame's noise-to-signal ratio: the reconstruction collapses to a
/// single `total_sc × P` complex matrix, so per-symbol estimation is one matrix-vector product.
pub struct CeSolver {
    total: usize,
    pilots: Vec<usize>,
    pilot_values: Vec<Complex32>,
    /// `total × P`, row-major: `h_est = recon · h_pilot_ls`.
    recon: Vec<Complex32>,
    /// `Σ_k |recon[rel][k]|²` per subcarrier — the estimator's noise gain, and hence its error variance
    /// once multiplied by the pilot-observation noise variance.
    recon_row_energy: Vec<f32>,
    residual_debias: f32,
}

impl DelayCe {
    /// Precompute the mode-constant matrices. Independent of the received signal.
    pub fn new(p: &ScFdmaParams) -> Self {
        let total = p.total_sc();
        let pilots = pilot_positions(p);
        let n_pilots = pilots.len();
        let pilot_values = (0..n_pilots).map(|k| pilot_value(p, k)).collect();

        let taus = delay_taps(n_pilots);
        let l = taus.len();

        // A[k][j] = exp(-j2π · sc_k · τ_j / N_FFT): the response of a unit tap at delay τ_j, sampled
        // at pilot subcarrier sc_k. N_FFT — not the occupied span — is the true period.
        let a: Vec<Complex64> = pilots
            .iter()
            .flat_map(|&sc| taus.iter().map(move |&t| steer(sc as f64, t)))
            .collect();
        let b: Vec<Complex64> = (0..total)
            .flat_map(|rel| {
                let sc = (p.first_sc + rel) as f64;
                taus.iter().map(move |&t| steer(sc, t))
            })
            .collect();

        let mut ata = vec![Complex64::new(0.0, 0.0); l * l];
        for i in 0..l {
            for j in 0..l {
                let mut acc = Complex64::new(0.0, 0.0);
                for k in 0..n_pilots {
                    acc += a[k * l + i].conj() * a[k * l + j];
                }
                ata[i * l + j] = acc;
            }
        }
        let trace: f64 = (0..l).map(|i| ata[i * l + i].re).sum();
        let lambda_floor = 1e-6 * trace / l as f64;

        let w: Vec<f64> = taus
            .iter()
            .map(|&t| (-t.abs() / CE_PRIOR_TAU_RMS).exp())
            .collect();
        let w_sum: f64 = w.iter().sum();
        let prior_inv = w.iter().map(|x| w_sum / x).collect();

        Self {
            total,
            pilots,
            pilot_values,
            taus,
            a,
            ata,
            lambda_floor,
            prior_inv,
            b,
        }
    }

    /// Number of complex taps the basis fits.
    pub fn taps(&self) -> usize {
        self.taus.len()
    }

    /// Least-squares pilot observations `h_k = Y[sc_k] / pilot_k`.
    fn pilot_ls(&self, freq: &[Complex32]) -> Vec<Complex32> {
        self.pilots
            .iter()
            .zip(self.pilot_values.iter())
            .map(|(&sc, &pv)| freq[sc] / pv)
            .collect()
    }

    /// Mean pilot power `E|h_k|²` — this still includes the noise; [`DelayCe::solver`] subtracts it.
    pub fn channel_power(&self, freq: &[Complex32]) -> f32 {
        let h = self.pilot_ls(freq);
        (h.iter().map(|c| c.norm_sqr()).sum::<f32>() / h.len() as f32).max(1e-12)
    }

    /// Build the Wiener solver for a frame with per-bin noise variance `noise_var` and mean channel
    /// power `chan_power` (both in the same units — see [`DelayCe::channel_power`]).
    ///
    /// `c = (AᴴA + σ²_h · R⁻¹)⁻¹ Aᴴ h` with `R = diag(P_ch · w_j)` is the MMSE tap estimate under the
    /// exponential delay-power prior `w`. It collapses to plain least squares as σ² → 0 and to a
    /// heavily damped, short-delay fit at low SNR, which is exactly the wanted behaviour.
    pub fn solver(&self, noise_var: f32, chan_power: f32) -> CeSolver {
        let n_pilots = self.pilots.len();
        let l = self.taus.len();
        let sigma2_h = (noise_var / (PILOT_AMPLITUDE * PILOT_AMPLITUDE)).max(1e-12) as f64;
        // `chan_power` is E|h_k|² = P_signal + σ²_h; the prior wants the signal part alone.
        let p_ch = (chan_power as f64 - sigma2_h).max(1e-12);
        let ridge: Vec<f64> = self
            .prior_inv
            .iter()
            .map(|inv_w| (sigma2_h * inv_w / p_ch).max(self.lambda_floor))
            .collect();

        let pinv = ridge_pseudo_inverse(&self.a, &self.ata, n_pilots, l, &ridge);
        let residual_debias = residual_debias(&self.a, &pinv, n_pilots, l);

        // Accumulate each recon entry in f64 and round once: `pinv` entries can reach ~10³ while the
        // recon they sum to is O(1), so per-term f32 rounding would set the noiseless CE-MSE floor.
        let mut recon = vec![Complex32::new(0.0, 0.0); self.total * n_pilots];
        for rel in 0..self.total {
            for k in 0..n_pilots {
                let mut acc = Complex64::new(0.0, 0.0);
                for j in 0..l {
                    acc += self.b[rel * l + j] * pinv[j * n_pilots + k];
                }
                recon[rel * n_pilots + k] = Complex32::new(acc.re as f32, acc.im as f32);
            }
        }
        let recon_row_energy = (0..self.total)
            .map(|rel| {
                recon[rel * n_pilots..(rel + 1) * n_pilots]
                    .iter()
                    .map(|c| c.norm_sqr())
                    .sum()
            })
            .collect();

        CeSolver {
            total: self.total,
            pilots: self.pilots.clone(),
            pilot_values: self.pilot_values.clone(),
            recon,
            recon_row_energy,
            residual_debias,
        }
    }
}

impl CeSolver {
    /// Estimate the channel at every occupied SC. Returns estimates indexed by `sc - first_sc`.
    pub fn estimate(&self, freq: &[Complex32]) -> Vec<Complex32> {
        let n_pilots = self.pilots.len();
        let h_pilot: Vec<Complex32> = self
            .pilots
            .iter()
            .zip(self.pilot_values.iter())
            .map(|(&sc, &pv)| freq[sc] / pv)
            .collect();
        (0..self.total)
            .map(|rel| {
                self.recon[rel * n_pilots..(rel + 1) * n_pilots]
                    .iter()
                    .zip(h_pilot.iter())
                    .fold(Complex32::new(0.0, 0.0), |acc, (&r, &h)| acc + r * h)
            })
            .collect()
    }

    /// Factor that turns this fit's mean pilot residual into an unbiased σ². See [`residual_debias`].
    pub fn noise_debias(&self) -> f32 {
        self.residual_debias
    }

    /// Per-subcarrier channel-estimate error variance `ε²_k`, for a frame with per-bin noise `noise_var`.
    ///
    /// `ĥ = R·h_pilot` and the pilot observations carry noise of variance `σ²_h = σ² / |pilot|²`, so the
    /// estimate's own error variance at subcarrier `k` is `σ²_h · Σ_j |R[k][j]|²`. This is what
    /// [`mmse_llr_noise_var`] needs to stop pretending the channel estimate is exact.
    pub fn ce_error_var_per_sc(&self, noise_var: f32) -> Vec<f32> {
        let sigma2_h = noise_var / (PILOT_AMPLITUDE * PILOT_AMPLITUDE);
        self.recon_row_energy.iter().map(|e| sigma2_h * e).collect()
    }
}

/// Unit-delay steering phasor `exp(-j2π · sc · τ / N_FFT)`.
fn steer(sc: f64, tau: f64) -> Complex64 {
    let ph = -std::f64::consts::TAU * sc * tau / FFT_SIZE as f64;
    Complex64::new(ph.cos(), ph.sin())
}

/// `P / ‖I − A·pinv‖²_F` — the factor that debiases the mean pilot residual into σ².
///
/// The fit is measured against the same pilots it consumes, so the residual only carries the noise
/// the fit *rejects*. For an orthogonal projection that fraction is exactly `(P−L)/P`, but a
/// ridge-regularised fit is an oblique, shrinking projection: its rejected fraction is `‖I − S‖²_F / P`
/// and depends on the ridge, so it must be computed rather than assumed.
fn residual_debias(a: &[Complex64], pinv: &[Complex64], p: usize, l: usize) -> f32 {
    let mut frob = 0.0f64;
    for i in 0..p {
        for j in 0..p {
            let mut s = Complex64::new(0.0, 0.0);
            for m in 0..l {
                s += a[i * l + m] * pinv[m * p + j];
            }
            let d = if i == j {
                Complex64::new(1.0, 0.0) - s
            } else {
                -s
            };
            frob += d.norm_sqr();
        }
    }
    if frob < 1e-9 {
        1.0
    } else {
        (p as f64 / frob) as f32
    }
}

/// `(AᴴA + diag(ridge))⁻¹ Aᴴ` for a `p × l` row-major complex `A`, returned `l × p` row-major.
///
/// Gauss-Jordan on the `l × 2l` augmented system in `f64`; `l ≤ 17` here, and the near-collinear
/// delay basis loses every significant bit of an `f32` mantissa in the normal equations.
fn ridge_pseudo_inverse(
    a: &[Complex64],
    ata: &[Complex64],
    p: usize,
    l: usize,
    ridge: &[f64],
) -> Vec<Complex64> {
    let zero = Complex64::new(0.0, 0.0);
    let mut m = vec![zero; l * 2 * l];
    for i in 0..l {
        for j in 0..l {
            m[i * 2 * l + j] = if i == j {
                ata[i * l + j] + Complex64::new(ridge[i], 0.0)
            } else {
                ata[i * l + j]
            };
        }
        m[i * 2 * l + l + i] = Complex64::new(1.0, 0.0);
    }
    for c in 0..l {
        let mut piv = c;
        for r in c + 1..l {
            if m[r * 2 * l + c].norm() > m[piv * 2 * l + c].norm() {
                piv = r;
            }
        }
        for j in 0..2 * l {
            m.swap(c * 2 * l + j, piv * 2 * l + j);
        }
        let d = m[c * 2 * l + c];
        if d.norm() < 1e-30 {
            continue;
        }
        for j in 0..2 * l {
            m[c * 2 * l + j] /= d;
        }
        for r in 0..l {
            if r == c {
                continue;
            }
            let f = m[r * 2 * l + c];
            if f.norm() < 1e-30 {
                continue;
            }
            for j in 0..2 * l {
                let v = m[c * 2 * l + j];
                m[r * 2 * l + j] -= f * v;
            }
        }
    }
    let mut out = vec![zero; l * p];
    for i in 0..l {
        for k in 0..p {
            let mut s = zero;
            for j in 0..l {
                s += m[i * 2 * l + l + j] * a[k * l + j].conj();
            }
            out[i * p + k] = s;
        }
    }
    out
}

/// Compute the LLR noise variance for soft demodulation after MMSE equalization and IDFT.
///
/// Returns `(llr_noise_var, alpha_avg)` where `alpha_avg` is the mean MMSE signal attenuation
/// across data SCs.  Dividing equalized symbols by `alpha_avg` restores unit-constellation
/// scale; `llr_noise_var` is then the calibrated noise floor for max-log-MAP LLRs.
///
/// Three terms, only the first of which this used to model:
/// 1. **additive noise** through the equalizer, `σ²·|C_k|²`;
/// 2. **residual ISI**, `var(α_k)` — the DFT de-spread averages the per-SC gains, so their *spread*
///    survives as self-interference. Zero on a flat channel; dominant at a spectral notch;
/// 3. **channel-estimate error**, `|C_k|²·ε²_k` from [`CeSolver::ce_error_var_per_sc`].
///
/// Omitting 2 and 3 made the LLRs over-confident: at 12 dB on a flat channel the measured error rate
/// among bits with `|L| ≈ 6` was 9× what `1/(1+e^{|L|})` promises, and 71× at `|L| ≈ 12`. A *uniform*
/// error is harmless (soft Viterbi is scale-invariant) — what costs is that the missing terms vary per
/// symbol, so faded and clean symbols were weighted against each other wrongly.
pub fn mmse_llr_noise_var(
    p: &ScFdmaParams,
    h_est: &[Complex32],
    noise_var: f32,
    ce_error_var: &[f32],
) -> (f32, f32) {
    let sigma2 = noise_var;
    let mut alpha_sum = 0.0f32;
    let mut alpha_sq_sum = 0.0f32;
    let mut eff_var_sum = 0.0f32;
    let mut ce_sum = 0.0f32;
    let mut count = 0usize;

    for (rel, h) in h_est.iter().enumerate() {
        if is_pilot(p, p.first_sc + rel) {
            continue;
        }
        let h_sq = h.norm_sqr();
        let denom = (h_sq + sigma2).max(1e-9);
        let alpha = h_sq / denom;
        // MMSE equalizer tap: |C_k|² = |Ĥ_k|² / (|Ĥ_k|² + σ²)².
        let c_sq = h_sq / (denom * denom).max(1e-12);
        // MMSE output noise per SC: σ² × |C_k|².
        eff_var_sum += sigma2 * c_sq;
        // Channel-estimate error passes through the same tap: |C_k|² × ε²_k.
        ce_sum += c_sq * ce_error_var.get(rel).copied().unwrap_or(0.0);
        alpha_sum += alpha;
        alpha_sq_sum += alpha * alpha;
        count += 1;
    }

    if count == 0 {
        return (sigma2, 1.0);
    }

    let n = count as f32;
    let alpha_avg = (alpha_sum / n).max(1e-6);
    let eff_var_avg = eff_var_sum / n;
    let ce_avg = ce_sum / n;
    // Residual ISI. The DFT de-spread averages the per-SC gains, so only their *spread* survives as
    // self-interference: E|α_k − ᾱ|² = E[α²] − ᾱ². Zero on a flat channel, and the dominant term at a
    // spectral notch — which is exactly where the LLRs used to claim certainty they did not have.
    let alpha_var = (alpha_sq_sum / n - alpha_avg * alpha_avg).max(0.0);
    // After dividing symbols by alpha_avg, effective noise variance is the sum of the three terms
    // scaled by 1/ᾱ².
    let llr_noise_var = ((eff_var_avg + alpha_var + ce_avg) / (alpha_avg * alpha_avg)).max(1e-6);
    (llr_noise_var, alpha_avg)
}

/// Zero-forcing equalization: divide each data SC bin by its channel estimate.
///
/// Returns equalized frequency-domain symbols for data SCs only.
pub fn zf_equalize(p: &ScFdmaParams, freq: &[Complex32], h_est: &[Complex32]) -> Vec<Complex32> {
    let mut out = Vec::with_capacity(p.n_data);
    for (rel, &h_in) in freq[p.first_sc..=p.last_sc].iter().enumerate() {
        let sc = p.first_sc + rel;
        if is_pilot(p, sc) {
            continue;
        }
        let h = h_est[rel];
        let eq = if h.norm_sqr() < 1e-6 { h_in } else { h_in / h };
        out.push(eq);
    }
    out
}

/// Estimate noise variance from pilot residuals.
///
/// Computes the mean squared error between the received pilots and the channel estimate applied to
/// the known pilot amplitude, then DEBIASES it. The estimate is fitted to the same pilot
/// observations it is measured against, so the raw residual is only the noise component the fit
/// *rejects*, not the full noise power. Un-debiased this under-reports σ² by several dB, which
/// under-regularises MMSE and over-states confidence in the soft LLRs at exactly the low-SNR regime
/// where soft-FEC and HARQ weighting live.
///
/// Only the localized (block-pilot) layout uses this now — the pilot-comb modes take σ² straight from
/// the comb, which no channel-estimate error can bias. `debias` is the estimator's own rejected-noise
/// factor: [`flat_ce_debias`] for the localized single-tap fit, [`CeSolver::noise_debias`] otherwise.
pub fn estimate_noise_var(
    p: &ScFdmaParams,
    freq: &[Complex32],
    h_est: &[Complex32],
    debias: f32,
) -> f32 {
    let pilots = pilot_positions(p);
    if pilots.is_empty() {
        return 1e-3;
    }
    let sum: f32 = pilots
        .iter()
        .enumerate()
        .map(|(k, &sc)| {
            let rel = sc - p.first_sc;
            let received = freq[sc];
            let predicted = h_est[rel] * pilot_value(p, k);
            let diff = received - predicted;
            diff.norm_sqr()
        })
        .sum();
    let raw = sum / pilots.len() as f32;
    (raw * debias.max(1.0)).max(1e-6)
}

/// Debias factor for [`flat_channel_estimate`]: one averaged complex gain fitted to `P` pilots, so
/// the residual keeps `(P−1)/P` of the noise.
pub fn flat_ce_debias(p: &ScFdmaParams) -> f32 {
    let n = pilot_positions(p).len();
    if n < 2 {
        1.0
    } else {
        n as f32 / (n - 1) as f32
    }
}

/// Estimate the Rician K-factor (linear ratio) from per-subcarrier channel taps.
///
/// The estimator uses the first two moments of instantaneous power |h|^2.
/// Returns 0.0 for near-Rayleigh channels and larger values for strong LOS.
pub fn estimate_rician_k_linear(h_est: &[Complex32]) -> f32 {
    if h_est.len() < 2 {
        return 0.0;
    }

    let powers: Vec<f32> = h_est.iter().map(|h| h.norm_sqr()).collect();
    let mean_power = powers.iter().sum::<f32>() / powers.len() as f32;
    if mean_power <= 1e-9 {
        return 0.0;
    }

    let var_power = powers
        .iter()
        .map(|p| {
            let d = *p - mean_power;
            d * d
        })
        .sum::<f32>()
        / powers.len() as f32;

    let mut r = var_power / (mean_power * mean_power);
    if !r.is_finite() {
        return 0.0;
    }

    // For Rician fading, r is in (0, 1] where 1 is Rayleigh (K=0).
    r = r.clamp(1e-6, 1.0);
    if (r - 1.0).abs() < 1e-4 {
        return 0.0;
    }

    let t = (1.0 - r).max(0.0);
    ((t + t.sqrt()) / r).max(0.0)
}

/// Minimum mean-square-error equalization.
///
/// Regularises the ZF solution with the estimated noise variance so that
/// weak subcarriers do not amplify noise — critical for 16QAM and 64QAM.
///
/// `W_MMSE[k] = H*[k] / (|H[k]|² + σ²)`
pub fn mmse_equalize(
    p: &ScFdmaParams,
    freq: &[Complex32],
    h_est: &[Complex32],
    noise_var: f32,
) -> Vec<Complex32> {
    let mut out = Vec::with_capacity(p.n_data);
    for (rel, &h_in) in freq[p.first_sc..=p.last_sc].iter().enumerate() {
        let sc = p.first_sc + rel;
        if is_pilot(p, sc) {
            continue;
        }
        let h = h_est[rel];
        let denom = h.norm_sqr() + noise_var;
        let eq = if denom < 1e-9 {
            h_in
        } else {
            h_in * h.conj() / denom
        };
        out.push(eq);
    }
    out
}

/// Compute per-symbol FFT spectra for up to 8 symbols.
///
/// Returns an empty `Vec` when fewer than two complete symbols are available.
/// Each entry is a full `FFT_SIZE`-point complex spectrum with the cyclic prefix
/// stripped.  Used by both `estimate_coh_bw_hz` and `estimate_cfo_hz` so the
/// FFT work is done once when both estimates are needed together.
pub(crate) fn compute_pilot_spectra(samples: &[f32], _p: &ScFdmaParams) -> Vec<Vec<Complex32>> {
    let n_syms = samples.len() / SYM_LEN;
    if n_syms < 2 {
        return vec![];
    }
    let n_use = n_syms.min(8);
    let scale = 1.0 / (FFT_SIZE as f32).sqrt();
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);
    let mut spectra = Vec::with_capacity(n_use);
    for sym_idx in 0..n_use {
        let start = sym_idx * SYM_LEN + CP;
        if start + FFT_SIZE > samples.len() {
            break;
        }
        let mut freq: Vec<Complex32> = samples[start..start + FFT_SIZE]
            .iter()
            .map(|&s| Complex32::new(s * scale, 0.0))
            .collect();
        fft.process(&mut freq);
        spectra.push(freq);
    }
    spectra
}

/// Estimate coherence bandwidth from pre-computed pilot spectra.
///
/// Returns `None` when fewer than two spectra or fewer than four pilots are
/// available.  Call `compute_pilot_spectra` once and pass the result to both
/// this function and `cfo_from_spectra` to avoid redundant FFT work.
pub(crate) fn coh_bw_from_spectra(spectra: &[Vec<Complex32>], p: &ScFdmaParams) -> Option<f32> {
    if spectra.len() < 2 {
        return None;
    }
    let pilots = pilot_positions(p);
    let n_pilots = pilots.len();
    if n_pilots < 4 {
        return None;
    }

    let mut r1_num = Complex32::new(0.0, 0.0);
    let mut pow0 = 0.0f32;
    let mut pow1 = 0.0f32;

    for freq in spectra {
        let h: Vec<Complex32> = pilots
            .iter()
            .enumerate()
            .map(|(k, &sc)| freq[sc] / pilot_value(p, k))
            .collect();
        for i in 0..n_pilots - 1 {
            r1_num += h[i].conj() * h[i + 1];
            pow0 += h[i].norm_sqr();
            pow1 += h[i + 1].norm_sqr();
        }
    }

    let denom = (pow0 * pow1).sqrt();
    if denom < 1e-8 {
        return None;
    }
    let r1_mag = (r1_num.norm() / denom).clamp(0.001, 0.9999);
    let pilot_sep_hz = p.pilot_spacing as f32 * SC_SPACING_HZ;
    let ratio_sq = (1.0 / (r1_mag * r1_mag) - 1.0).max(1e-6);
    Some((pilot_sep_hz / ratio_sq.sqrt()).clamp(10.0, 2000.0))
}

/// Estimate CFO in Hz from pre-computed pilot spectra.
///
/// Returns `None` when fewer than two spectra or no pilots are available.
pub(crate) fn cfo_from_spectra(spectra: &[Vec<Complex32>], p: &ScFdmaParams) -> Option<f32> {
    use std::f32::consts::PI;

    if spectra.len() < 2 {
        return None;
    }
    let pilots = pilot_positions(p);
    if pilots.is_empty() {
        return None;
    }

    let mut phase_sum = 0.0f32;
    let mut count = 0u32;
    for i in 0..(spectra.len() - 1) {
        for &k in &pilots {
            if k < FFT_SIZE {
                let conj_prod = spectra[i][k].conj() * spectra[i + 1][k];
                phase_sum += conj_prod.arg();
                count += 1;
            }
        }
    }
    if count == 0 {
        return None;
    }

    let mean_phase = phase_sum / count as f32;
    let t_sym = SYM_LEN as f32 / SAMPLE_RATE as f32;
    Some(mean_phase / (2.0 * PI * t_sym))
}

/// Estimate channel coherence bandwidth in Hz from inter-pilot complex correlations.
///
/// Under the exponential PDP model: |r₁|² ≈ 1 / (1 + (Δf_pilot/B_c)²), which
/// inverts to B_c = Δf_pilot / √(1/|r₁|² − 1).  Returns `None` when fewer than
/// two symbols or fewer than four pilots are available.
pub fn estimate_coh_bw_hz(samples: &[f32], p: &ScFdmaParams) -> Option<f32> {
    let spectra = compute_pilot_spectra(samples, p);
    coh_bw_from_spectra(&spectra, p)
}

/// Estimate the carrier frequency offset (CFO) in Hz using inter-symbol pilot
/// phase drift across consecutive SC-FDMA symbols.
///
/// Identical algorithm to the OFDM CFO estimator: the DFT-spreading step in
/// SC-FDMA does not affect pilot subcarriers (pilots bypass DFT precoding),
/// so inter-symbol pilot phase drift directly reveals the CFO.
///
/// Unambiguous range: `±Fs / (2 × SYM_LEN) ≈ ±13.9 Hz`.
///
/// Returns `None` when there are fewer than two complete symbols or no pilots.
pub fn estimate_cfo_hz(samples: &[f32], p: &ScFdmaParams) -> Option<f32> {
    let spectra = compute_pilot_spectra(samples, p);
    cfo_from_spectra(&spectra, p)
}

#[cfg(test)]
#[allow(clippy::needless_range_loop)]
mod tests {
    use super::*;
    use crate::params::{SCFDMA16, SCFDMA52, SCFDMA52_LP};

    /// The debiased noise-var estimate recovers the true σ² (flat channel), where the raw pilot
    /// residual would under-report it by the delay-basis projection factor (P/(P−L) = 13/4 ≈ 3.25×).
    #[test]
    fn estimate_noise_var_is_unbiased() {
        // Deterministic Box–Muller complex noise from a small LCG (no rng dep).
        let mut state: u64 = 0x1234_5678_9abc_def0;
        let mut next = || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((state >> 33) as f32) / ((1u64 << 31) as f32) // ~U[0,1)
        };
        for (p, tol) in [(SCFDMA52, 0.30f32), (SCFDMA52_LP, 0.18)] {
            let sigma2 = 0.04f32; // true per-bin complex noise power
                                  // The Wiener solver's rejected-noise fraction depends on its ridge, so build it at the
                                  // channel/noise ratio the test actually simulates (flat unit channel, this σ²).
            let ce = (!p.localized).then(|| {
                DelayCe::new(&p).solver(sigma2, PILOT_AMPLITUDE * PILOT_AMPLITUDE + sigma2)
            });
            let pilots = pilot_positions(&p);
            let mut acc = 0.0f32;
            let trials = 4000;
            for _ in 0..trials {
                let mut freq = vec![Complex32::new(0.0, 0.0); FFT_SIZE];
                for &sc in &pilots {
                    // h_true = 1 (flat) → received = A + noise.
                    let (u1, u2) = (next().max(1e-6), next());
                    let mag = (-2.0 * (sigma2 / 2.0) * u1.ln()).sqrt();
                    let ang = std::f32::consts::TAU * u2;
                    freq[sc] = Complex32::new(PILOT_AMPLITUDE + mag * ang.cos(), mag * ang.sin());
                }
                let (h_est, debias) = match &ce {
                    Some(s) => (s.estimate(&freq), s.noise_debias()),
                    None => (flat_channel_estimate(&p, &freq), flat_ce_debias(&p)),
                };
                acc += estimate_noise_var(&p, &freq, &h_est, debias);
            }
            let est = acc / trials as f32;
            let rel_err = (est - sigma2).abs() / sigma2;
            assert!(
                rel_err < tol,
                "localized={}: debiased est {est:.4} vs true {sigma2:.4} (rel err {rel_err:.2})",
                p.localized
            );
        }
    }

    #[test]
    fn scfdma16_pilot_positions() {
        let pilots = pilot_positions(&SCFDMA16);
        assert_eq!(pilots, vec![42, 47, 52, 57]);
        assert_eq!(pilots.len(), SCFDMA16.n_pilots);
    }

    #[test]
    fn scfdma52_pilot_positions() {
        let pilots = pilot_positions(&SCFDMA52);
        assert_eq!(pilots.len(), SCFDMA52.n_pilots);
        assert_eq!(pilots[0], 20);
        assert_eq!(*pilots.last().unwrap(), 80);
    }

    #[test]
    fn rician_k_estimator_rayleigh_like_near_zero() {
        // Deterministic Box-Muller Gaussian taps with zero-mean I/Q.
        let mut state = 0x1234_5678_9abc_def0u64;
        let mut taps = Vec::with_capacity(256);
        for _ in 0..256 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let u1 = ((state >> 11) as f64) * (1.0 / ((1u64 << 53) as f64));
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let u2 = ((state >> 11) as f64) * (1.0 / ((1u64 << 53) as f64));
            let u1 = u1.clamp(1e-12, 1.0 - 1e-12);
            let r = (-2.0 * u1.ln()).sqrt() as f32;
            let theta = (2.0 * std::f64::consts::PI * u2) as f32;
            taps.push(Complex32::new(r * theta.cos(), r * theta.sin()));
        }

        let k = estimate_rician_k_linear(&taps);
        assert!(k >= 0.0);
        assert!(k < 1.5, "expected low K for diffuse channel, got {k}");
    }

    #[test]
    fn rician_k_estimator_los_dominant_higher() {
        let diffuse: Vec<Complex32> = (0..128)
            .map(|i| Complex32::new((i as f32 * 0.11).sin(), (i as f32 * 0.23).cos()))
            .collect();
        let los: Vec<Complex32> = diffuse
            .iter()
            .map(|h| Complex32::new(2.0 + h.re * 0.2, 0.1 + h.im * 0.2))
            .collect();

        let k_diffuse = estimate_rician_k_linear(&diffuse);
        let k_los = estimate_rician_k_linear(&los);
        assert!(k_los > k_diffuse, "expected LOS channel to raise K");
    }

    #[test]
    fn delay_ce_flat_channel_all_ones() {
        // Flat channel: H[k]=1 for all occupied SCs.  Pilot observations are
        // exactly PILOT_AMPLITUDE so LS gives h=1.0 at every pilot SC.
        let p = &SCFDMA52;
        let mut freq = vec![Complex32::new(0.0, 0.0); FFT_SIZE];
        for sc in p.first_sc..=p.last_sc {
            freq[sc] = Complex32::new(PILOT_AMPLITUDE, 0.0);
        }
        let h_est = DelayCe::new(p).solver(1e-6, 1.0).estimate(&freq);
        assert_eq!(h_est.len(), p.total_sc());
        for (i, h) in h_est.iter().enumerate() {
            assert!(
                (h.re - 1.0).abs() < 0.01 && h.im.abs() < 0.01,
                "SC rel {i}: expected h≈1+0j, got {h:?}"
            );
        }
    }

    #[test]
    fn delay_ce_less_noise_than_ls_under_awgn() {
        // AWGN on pilot observations: the delay basis fits 9 taps to 13 observations, averaging the
        // noise the raw LS pilot estimates carry — lower RMS error than LS + linear interpolation.
        let p = &SCFDMA52;
        // Deterministic PRNG noise (LCG) at pilot positions.
        let mut state = 0xDEAD_BEEF_u64;
        let noise_std = 0.15_f32; // ~16 dB below pilot amplitude
        let mut freq = vec![Complex32::new(0.0, 0.0); FFT_SIZE];
        // Data and pilot SCs: true channel H=1.
        for sc in p.first_sc..=p.last_sc {
            freq[sc] = Complex32::new(PILOT_AMPLITUDE, 0.0);
        }
        // Corrupt pilot SCs with additive noise.
        for &sc in pilot_positions(p).iter() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let ni = ((state >> 11) as f32) / ((1u64 << 53) as f32) * 2.0 - 1.0;
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let nq = ((state >> 11) as f32) / ((1u64 << 53) as f32) * 2.0 - 1.0;
            freq[sc] += Complex32::new(ni * noise_std, nq * noise_std);
        }

        // Wiener fit at the simulated pilot-noise level (uniform ±noise_std ⇒ σ² = 2·std²/3).
        let sigma2 = 2.0 * noise_std * noise_std / 3.0;
        let h_dft = DelayCe::new(p)
            .solver(sigma2, PILOT_AMPLITUDE * PILOT_AMPLITUDE + sigma2)
            .estimate(&freq);
        let h_ls = ls_estimate(p, &freq);

        // RMS error over all total SCs: the delay-basis CE must beat LS.
        let rms = |est: &[Complex32]| {
            let mse: f32 = est
                .iter()
                .map(|h| (h.re - 1.0).powi(2) + h.im.powi(2))
                .sum::<f32>()
                / est.len() as f32;
            mse.sqrt()
        };
        let rms_dft = rms(&h_dft);
        let rms_ls = rms(&h_ls);
        assert!(
            rms_dft < rms_ls,
            "delay-basis CE RMS {rms_dft:.4} should be less than LS RMS {rms_ls:.4}"
        );
    }

    #[test]
    fn delay_ce_output_length_matches_total_sc() {
        // Output slice must cover all occupied SCs regardless of pilot count.
        for p in [&SCFDMA16, &SCFDMA52] {
            let freq = vec![Complex32::new(PILOT_AMPLITUDE, 0.0); FFT_SIZE];
            let h = DelayCe::new(p).solver(1e-6, 1.0).estimate(&freq);
            assert_eq!(
                h.len(),
                p.total_sc(),
                "mode first_sc={} last_sc={}",
                p.first_sc,
                p.last_sc
            );
        }
    }
}
