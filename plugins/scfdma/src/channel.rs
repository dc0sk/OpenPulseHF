//! Pilot layout, LS channel estimation, ZF/MMSE equalization, and CFO estimation for SC-FDMA.
//!
//! Pilot layout is identical to OFDM: every 5th SC starting at first_sc+4.

use num_complex::Complex32;
use rustfft::FftPlanner;

use crate::params::{ScFdmaParams, CP, FFT_SIZE, PILOT_AMPLITUDE, SAMPLE_RATE, SYM_LEN};

/// Return the absolute SC indices of all pilot subcarriers for `p`.
pub fn pilot_positions(p: &ScFdmaParams) -> Vec<usize> {
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

/// `true` when absolute SC index `sc` is a pilot for this mode.
pub fn is_pilot(p: &ScFdmaParams, sc: usize) -> bool {
    if p.pilot_spacing == 0 {
        return false;
    }
    if sc < p.first_sc || sc > p.last_sc {
        return false;
    }
    let offset = sc - p.first_sc;
    offset % p.pilot_spacing == (p.pilot_spacing - 1)
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
        .map(|&sc| {
            let h = freq[sc] / Complex32::new(PILOT_AMPLITUDE, 0.0);
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

/// DFT-domain channel estimation (DFT-CE).
///
/// Replaces LS + linear interpolation with IDFT → delay-window → direct evaluation.
/// The physical channel has at most `CP` samples of delay spread; by transforming the
/// P pilot observations to a P-point CIR and zeroing taps beyond that window, noise
/// energy outside the CP window is discarded before reconstructing H at every SC.
///
/// The reconstruction step evaluates the DFT at each occupied SC position analytically
/// rather than via zero-padding, which correctly handles the non-zero `first_sc` offset.
///
/// Returns estimates indexed by `sc - first_sc` (length = `p.total_sc()`).
pub fn dft_ce_estimate(p: &ScFdmaParams, freq: &[Complex32]) -> Vec<Complex32> {
    let total = p.total_sc();
    let pilots = pilot_positions(p);
    let n_pilots = pilots.len();

    if n_pilots < 2 {
        return ls_estimate(p, freq);
    }

    // --- Step 1: LS at pilot SCs ---
    let mut h_pilot: Vec<Complex32> = pilots
        .iter()
        .map(|&sc| freq[sc] / Complex32::new(PILOT_AMPLITUDE, 0.0))
        .collect();

    // --- Step 2: IDFT(P) → P-point warped CIR h'[l] ---
    // CIR tap l represents physical delay  d_l = l × N_FFT / total_sc  samples.
    let mut planner = FftPlanner::<f32>::new();
    let idft = planner.plan_fft_inverse(n_pilots);
    idft.process(&mut h_pilot);
    let inv_p = 1.0 / n_pilots as f32;
    for h in &mut h_pilot {
        *h *= inv_p;
    }

    // --- Step 3: CP-window — zero taps whose delay exceeds the cyclic prefix ---
    // d_l ≤ CP  ⟺  l ≤ CP × total_sc / N_FFT.
    // For SCFDMA52: ceil(32 × 65 / 256) = 9 taps (out of 13) kept.
    let l_max = (CP * total).div_ceil(FFT_SIZE).clamp(1, n_pilots);
    for h in h_pilot[l_max..].iter_mut() {
        *h = Complex32::new(0.0, 0.0);
    }

    // --- Step 4: Reconstruct H at all occupied SCs ---
    // H_est[first_sc + rel] = Σ_{l=0}^{l_max-1} h'[l] × exp(-j2π (rel - offset) l / total)
    // where offset = (first_pilot_sc - first_sc) = pilot_spacing - 1.
    let offset = p.pilot_spacing as isize - 1;
    (0..total)
        .map(|rel| {
            let freq_idx = rel as isize - offset;
            let mut sum = Complex32::new(0.0, 0.0);
            for (l, &tap) in h_pilot.iter().enumerate().take(l_max) {
                let phase = -std::f32::consts::TAU * freq_idx as f32 * l as f32 / total as f32;
                sum += tap * Complex32::new(phase.cos(), phase.sin());
            }
            sum
        })
        .collect()
}

/// Compute the LLR noise variance for soft demodulation after MMSE equalization and IDFT.
///
/// Returns `(llr_noise_var, alpha_avg)` where `alpha_avg` is the mean MMSE signal attenuation
/// across data SCs.  Dividing equalized symbols by `alpha_avg` restores unit-constellation
/// scale; `llr_noise_var` is then the correctly calibrated noise floor for min-log-MAP LLRs.
pub fn mmse_llr_noise_var(p: &ScFdmaParams, h_est: &[Complex32], noise_var: f32) -> (f32, f32) {
    let sigma2 = noise_var;
    let mut alpha_sum = 0.0f32;
    let mut eff_var_sum = 0.0f32;
    let mut count = 0usize;

    for (rel, h) in h_est.iter().enumerate() {
        if is_pilot(p, p.first_sc + rel) {
            continue;
        }
        let h_sq = h.norm_sqr();
        let denom = (h_sq + sigma2).max(1e-9);
        let alpha = h_sq / denom;
        // MMSE output noise per SC: σ² × |H|² / (|H|² + σ²)²
        let eff_var = sigma2 * h_sq / (denom * denom).max(1e-12);
        alpha_sum += alpha;
        eff_var_sum += eff_var;
        count += 1;
    }

    if count == 0 {
        return (sigma2, 1.0);
    }

    let alpha_avg = (alpha_sum / count as f32).max(1e-6);
    let eff_var_avg = eff_var_sum / count as f32;
    // After dividing symbols by alpha_avg, effective noise variance is:
    // σ²_LLR = eff_var_avg / alpha_avg²
    let llr_noise_var = (eff_var_avg / (alpha_avg * alpha_avg)).max(1e-6);
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
/// Computes the mean squared error between the received pilots and the
/// LS channel estimate applied to the known pilot amplitude.  This gives a
/// per-symbol noise power estimate used to regularise MMSE equalization.
pub fn estimate_noise_var(p: &ScFdmaParams, freq: &[Complex32], h_est: &[Complex32]) -> f32 {
    let pilots = pilot_positions(p);
    if pilots.is_empty() {
        return 1e-3;
    }
    let sum: f32 = pilots
        .iter()
        .map(|&sc| {
            let rel = sc - p.first_sc;
            let received = freq[sc];
            let predicted = h_est[rel] * Complex32::new(PILOT_AMPLITUDE, 0.0);
            let diff = received - predicted;
            diff.norm_sqr()
        })
        .sum();
    (sum / pilots.len() as f32).max(1e-6)
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
    use std::f32::consts::PI;

    let n_syms = samples.len() / SYM_LEN;
    if n_syms < 2 {
        return None;
    }
    let pilots = pilot_positions(p);
    if pilots.is_empty() {
        return None;
    }

    let n_use = n_syms.min(8);
    let scale = 1.0 / (FFT_SIZE as f32).sqrt();
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);

    let mut spectra: Vec<Vec<Complex32>> = Vec::with_capacity(n_use);
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
    if spectra.len() < 2 {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::{SCFDMA16, SCFDMA52};

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
    fn dft_ce_flat_channel_all_ones() {
        // Flat channel: H[k]=1 for all occupied SCs.  Pilot observations are
        // exactly PILOT_AMPLITUDE so LS gives h=1.0 at every pilot SC.
        // DFT-CE must recover h≈1.0 at every total SC position.
        let p = &SCFDMA52;
        let mut freq = vec![Complex32::new(0.0, 0.0); FFT_SIZE];
        for sc in p.first_sc..=p.last_sc {
            freq[sc] = Complex32::new(PILOT_AMPLITUDE, 0.0);
        }
        let h_est = dft_ce_estimate(p, &freq);
        assert_eq!(h_est.len(), p.total_sc());
        for (i, h) in h_est.iter().enumerate() {
            assert!(
                (h.re - 1.0).abs() < 0.01 && h.im.abs() < 0.01,
                "SC rel {i}: expected h≈1+0j, got {h:?}"
            );
        }
    }

    #[test]
    fn dft_ce_less_noise_than_ls_under_awgn() {
        // AWGN on pilot observations: DFT-CE exploits the CP window to average
        // noise across all pilots, giving lower RMS error than LS interpolation.
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

        let h_dft = dft_ce_estimate(p, &freq);
        let h_ls = ls_estimate(p, &freq);

        // RMS error over all total SCs: DFT-CE must beat LS.
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
            "DFT-CE RMS {rms_dft:.4} should be less than LS RMS {rms_ls:.4}"
        );
    }

    #[test]
    fn dft_ce_output_length_matches_total_sc() {
        // Output slice must cover all occupied SCs regardless of pilot count.
        for p in [&SCFDMA16, &SCFDMA52] {
            let freq = vec![Complex32::new(PILOT_AMPLITUDE, 0.0); FFT_SIZE];
            let h = dft_ce_estimate(p, &freq);
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
