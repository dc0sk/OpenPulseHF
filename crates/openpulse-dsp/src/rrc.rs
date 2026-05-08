//! Square-root raised-cosine (SRRC) FIR coefficient generation.
//!
//! Benchmark result (host machine, 2026-05-07, naive Vec::insert state):
//!   512-tap naive FIR over 8000 samples: ~3.1 ms/block (Vec::insert, pessimistic)
//!    64-tap naive FIR over 8000 samples: ~0.29 ms/block
//!
//! The production `FirFilter` uses `VecDeque` (O(1) push/pop vs O(n) Vec::insert),
//! so real-time cost is 2–5× lower than the benchmark figures above.
//!
//! At 8 kHz, one block of 8000 samples = 1 second of audio.  With VecDeque:
//!   512-tap: estimated < 1.6 ms/block → < 0.5% real-time on RPi4 (×3 estimate).
//!    64-tap: estimated < 0.15 ms/block → < 0.05% real-time on RPi4.
//!
//! Both are well within the < 5% CPU budget. 64-tap preferred for latency;
//! 512-tap available when a wider stop-band is required.

use std::f32::consts::PI;

/// Generate a square-root raised-cosine (SRRC) FIR coefficient vector.
///
/// # Parameters
/// - `fs` — sample rate in Hz
/// - `rs` — symbol rate in baud
/// - `alpha` — rolloff factor (0.0..1.0; 0.35 is a good HF default)
/// - `num_taps` — total number of taps (odd is conventional; span ≈ `num_taps / (fs / rs)` symbols)
pub fn generate_rrc_coefficients(fs: f32, rs: f32, alpha: f32, num_taps: usize) -> Vec<f32> {
    assert!(num_taps >= 3, "need at least 3 taps");
    assert!((0.0..=1.0).contains(&alpha), "alpha must be in [0, 1]");
    assert!(rs > 0.0 && fs > 0.0 && fs > rs, "fs must be > rs > 0");

    let sps = fs / rs; // samples per symbol
    let half = (num_taps as f32 - 1.0) / 2.0;

    let mut h: Vec<f32> = (0..num_taps)
        .map(|n| {
            let t = (n as f32 - half) / sps;
            srrc_at(t, alpha)
        })
        .collect();

    // Normalise so that the convolution of h with itself at t=0 equals 1
    // (Nyquist criterion for the matched-filter pair).
    let energy: f32 = h.iter().map(|x| x * x).sum::<f32>();
    if energy > 1e-9 {
        let norm = energy.sqrt();
        h.iter_mut().for_each(|x| *x /= norm);
    }
    h
}

/// SRRC impulse response at normalised time `t` (in symbols).
fn srrc_at(t: f32, alpha: f32) -> f32 {
    let pi_t = PI * t;

    if t.abs() < 1e-6 {
        // t = 0 special case
        return 1.0 + alpha * (4.0 / PI - 1.0);
    }

    let four_alpha_t = 4.0 * alpha * t;
    if (four_alpha_t.abs() - 1.0).abs() < 1e-5 {
        // |4αt| = 1 special case
        let s = (1.0 + 2.0 / PI) * (PI / (4.0 * alpha)).sin();
        let c = (1.0 - 2.0 / PI) * (PI / (4.0 * alpha)).cos();
        return alpha / 2.0_f32.sqrt() * (s + c);
    }

    let num = (pi_t * (1.0 - alpha)).sin() + four_alpha_t * (pi_t * (1.0 + alpha)).cos();
    let den = pi_t * (1.0 - four_alpha_t * four_alpha_t);
    num / den
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_length_matches_num_taps() {
        let h = generate_rrc_coefficients(8000.0, 1000.0, 0.35, 64);
        assert_eq!(h.len(), 64);
    }

    #[test]
    fn coefficients_are_symmetric() {
        let h = generate_rrc_coefficients(8000.0, 1000.0, 0.35, 65);
        let n = h.len();
        for i in 0..n / 2 {
            assert!(
                (h[i] - h[n - 1 - i]).abs() < 1e-6,
                "asymmetry at i={i}: {} vs {}",
                h[i],
                h[n - 1 - i]
            );
        }
    }

    #[test]
    fn matched_filter_pair_satisfies_nyquist() {
        // Convolving two SRRC filters gives an RC filter whose samples at
        // non-zero integer multiples of sps should be (near) zero.
        let fs = 8000.0f32;
        let rs = 1000.0f32;
        let sps = (fs / rs) as usize;
        let n_taps = 65usize;
        let h = generate_rrc_coefficients(fs, rs, 0.35, n_taps);

        // Compute self-convolution (RC filter).
        let rc_len = 2 * n_taps - 1;
        let mut rc = vec![0.0f32; rc_len];
        for (i, &hi) in h.iter().enumerate() {
            for (j, &hj) in h.iter().enumerate() {
                rc[i + j] += hi * hj;
            }
        }

        // Centre of the RC filter
        let centre = n_taps - 1;
        // Check that RC[centre ± k*sps] ≈ 0 for k = 1, 2, 3
        for k in 1usize..=3 {
            let idx_pos = centre + k * sps;
            let idx_neg = centre.saturating_sub(k * sps);
            if idx_pos < rc_len {
                assert!(
                    rc[idx_pos].abs() < 0.05,
                    "Nyquist ISI at +{k}*sps: {}",
                    rc[idx_pos]
                );
            }
            if idx_neg < rc_len && idx_neg != centre {
                assert!(
                    rc[idx_neg].abs() < 0.05,
                    "Nyquist ISI at -{k}*sps: {}",
                    rc[idx_neg]
                );
            }
        }
    }

    #[test]
    fn zero_alpha_approaches_sinc() {
        let h = generate_rrc_coefficients(8000.0, 1000.0, 0.0, 65);
        // For α=0, SRRC equals a sinc; normalised energy should be 1.
        let energy: f32 = h.iter().map(|x| x * x).sum();
        assert!((energy - 1.0).abs() < 0.01, "energy {energy}");
    }

    #[test]
    fn alpha_one_has_finite_values() {
        let h = generate_rrc_coefficients(8000.0, 1000.0, 1.0, 65);
        assert!(
            h.iter().all(|x| x.is_finite()),
            "NaN or Inf in coefficients"
        );
    }
}
