//! Blind SNR estimation from received samples (M2M4 moment estimator).
//!
//! The adaptive rate loop needs an *absolute* SNR to pick a level. The old
//! LLR-magnitude proxy (`mean|LLR|`) is only a relative confidence indicator
//! (≈ −2 dB on a clean path), so it can't drive the ladder. This module provides
//! a real estimate from the second and fourth moments of the signal envelope —
//! the classic M2M4 estimator, exact for a constant-modulus (PSK) signal in
//! complex-Gaussian noise and a good approximation for QAM.

use crate::iq::hilbert_iq;

/// Clamp range for the returned dB estimate (keeps pathological inputs bounded).
const SNR_FLOOR_DB: f32 = -10.0;
const SNR_CEIL_DB: f32 = 40.0;

/// M2M4 SNR estimate (dB) from baseband I/Q over the whole input.
///
/// For an M-PSK signal in complex-Gaussian noise: with `M2 = E[|r|²]` and
/// `M4 = E[|r|⁴]`, signal power `S = √(2·M2² − M4)` and noise power `N = M2 − S`.
/// The full envelope distribution is needed for the moments to be correct, so this
/// does **not** gate samples — pass the active signal region (the caller's captured
/// frame buffer). Trailing/leading silence biases the estimate low (conservative).
/// Returns [`SNR_FLOOR_DB`] for empty or silent input; result clamped to
/// `[SNR_FLOOR_DB, SNR_CEIL_DB]`.
pub fn m2m4_snr_db(i: &[f32], q: &[f32]) -> f32 {
    let n = i.len().min(q.len());
    if n < 2 {
        return SNR_FLOOR_DB;
    }
    let mut sum_p = 0.0f64; // Σ|r|²
    let mut sum_p2 = 0.0f64; // Σ|r|⁴
    for k in 0..n {
        let p = (i[k] * i[k] + q[k] * q[k]) as f64;
        sum_p += p;
        sum_p2 += p * p;
    }
    let count = n as f64;
    let m2 = sum_p / count;
    let m4 = sum_p2 / count;
    if m2 <= 0.0 {
        return SNR_FLOOR_DB;
    }

    // S = sqrt(2 M2² − M4); guard the radicand (negative for noise-dominated or
    // strongly non-PSK input → treat as no recoverable signal).
    let s = (2.0 * m2 * m2 - m4).max(0.0).sqrt();
    let noise = (m2 - s).max(1e-12);
    let snr_lin = (s / noise).max(1e-6);
    ((10.0 * snr_lin.log10()) as f32).clamp(SNR_FLOOR_DB, SNR_CEIL_DB)
}

/// M2M4 SNR estimate (dB) from a real passband buffer: forms the baseband I/Q via
/// [`hilbert_iq`] at carrier `fc` (Hz) / sample rate `fs` (Hz), then estimates.
pub fn m2m4_snr_db_from_real(samples: &[f32], fc: f32, fs: f32) -> f32 {
    let (i, q) = hilbert_iq(samples, fc, fs);
    m2m4_snr_db(&i, &q)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::{Rng, SeedableRng};

    /// Constant-modulus (random-phase PSK) I/Q with additive complex Gaussian noise
    /// at a target SNR (dB). Returns (I, Q).
    fn psk_iq_with_awgn(n: usize, snr_db: f32, rng: &mut StdRng) -> (Vec<f32>, Vec<f32>) {
        let s = 1.0f32; // unit signal power per component pair (|sym| = 1)
        let snr_lin = 10f32.powf(snr_db / 10.0);
        let noise_sigma = (s / snr_lin).sqrt() / std::f32::consts::SQRT_2; // per-component
        let mut i = Vec::with_capacity(n);
        let mut q = Vec::with_capacity(n);
        for _ in 0..n {
            let phase = rng.gen::<f32>() * std::f32::consts::TAU;
            let (sy, sx) = phase.sin_cos();
            // Box-Muller complex Gaussian.
            let u1: f32 = rng.gen::<f32>().max(1e-7);
            let u2: f32 = rng.gen();
            let mag = (-2.0 * u1.ln()).sqrt();
            let ni = noise_sigma * mag * (std::f32::consts::TAU * u2).cos();
            let nq = noise_sigma * mag * (std::f32::consts::TAU * u2).sin();
            i.push(sx + ni);
            q.push(sy + nq);
        }
        (i, q)
    }

    #[test]
    fn m2m4_tracks_known_snr_within_a_couple_db() {
        let mut rng = StdRng::seed_from_u64(11);
        for &target in &[0.0f32, 5.0, 10.0, 15.0, 20.0] {
            let (i, q) = psk_iq_with_awgn(20_000, target, &mut rng);
            let est = m2m4_snr_db(&i, &q);
            assert!(
                (est - target).abs() <= 2.5,
                "M2M4 estimate {est:.1} dB too far from target {target:.1} dB"
            );
        }
    }

    #[test]
    fn m2m4_is_monotonic_in_snr() {
        let mut rng = StdRng::seed_from_u64(7);
        let (li, lq) = psk_iq_with_awgn(20_000, 3.0, &mut rng);
        let (hi_i, hi_q) = psk_iq_with_awgn(20_000, 18.0, &mut rng);
        let lo = m2m4_snr_db(&li, &lq);
        let hi = m2m4_snr_db(&hi_i, &hi_q);
        assert!(
            hi > lo,
            "higher SNR must estimate higher: hi={hi:.1} lo={lo:.1}"
        );
    }

    #[test]
    fn empty_and_silent_inputs_return_floor() {
        assert_eq!(m2m4_snr_db(&[], &[]), SNR_FLOOR_DB);
        assert_eq!(m2m4_snr_db(&[0.0; 64], &[0.0; 64]), SNR_FLOOR_DB);
    }

    #[test]
    fn clean_signal_estimates_high() {
        // Noiseless constant-modulus tone → near the ceiling.
        let i: Vec<f32> = (0..1000).map(|k| (k as f32 * 0.3).cos()).collect();
        let q: Vec<f32> = (0..1000).map(|k| (k as f32 * 0.3).sin()).collect();
        assert!(m2m4_snr_db(&i, &q) >= 25.0);
    }
}
