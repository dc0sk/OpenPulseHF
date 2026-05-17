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
}
