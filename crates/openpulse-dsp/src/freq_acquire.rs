//! Dedicated burst frequency-acquisition stage (qdetector-style).
//!
//! Recovers timing, carrier-frequency offset, phase, and gain *jointly* from one
//! known preamble, in two passes (see `docs/dev/freq-acquisition-design.md`):
//!
//! 1. **Coarse (joint timing + CFO).** For each candidate timing τ, de-rotate the
//!    received window by the known preamble (`rx[τ+n]·conj(p[n])`) and take an
//!    `L`-point FFT. De-rotation removes the preamble modulation, leaving
//!    `γ·exp(j(2π·f·n+φ))` whose FFT is a single peak at the CFO bin — so the
//!    maximum bin magnitude over all `(τ, k)` is a carrier-phase- and
//!    CFO-insensitive timing metric, and its bin is the coarse CFO.
//! 2. **Fine (CFO + phase + gain).** At the winning τ, zero-pad the de-rotated
//!    sequence and FFT again for finer bins, then quadratically interpolate the
//!    magnitude peak for sub-bin CFO. The peak's complex value gives phase and
//!    gain.
//!
//! This is the "coarse-from-preamble, fine-from-de-rotated-payload" two-stage the
//! per-mode AFC-settle path lacks. It is a pure function over complex baseband —
//! no engine state, no receive-loop coupling (that is a later phase).
//!
//! CFO is reported in **cycles per sample** in `[-0.5, 0.5)`; multiply by the
//! sample rate for Hz. The preamble should be (near) constant modulus (PSK), so
//! the de-rotation weighting `|p[n]|²` does not distort the CFO peak.

use num_complex::Complex32;
use rustfft::FftPlanner;

/// Result of a frequency-acquisition pass.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Acquisition {
    /// Sample offset of the preamble start within the search window (τ̂).
    pub timing: usize,
    /// Carrier-frequency offset in cycles/sample, `[-0.5, 0.5)` (Δφ̂). × fs → Hz.
    pub cfo_cycles_per_sample: f32,
    /// Carrier phase at the preamble start, radians (φ̂).
    pub phase: f32,
    /// Amplitude scale of the received preamble vs the reference (γ̂).
    pub gain: f32,
    /// Normalised correlation peak in `[0, 1]` — detection confidence.
    pub metric: f32,
}

/// Jointly acquire timing + CFO + phase + gain from a known preamble.
///
/// `rx` is complex baseband; `preamble` is the known reference (same sample rate,
/// near constant modulus). `timing` candidates are scanned over
/// `[search_start, search_end]` (inclusive of `search_start`, exclusive of
/// `search_end`); the caller should bound this to a window around the coarse
/// onset to keep the per-τ FFT cost down. Returns `None` if the inputs are too
/// short to hold the preamble at any searched offset.
pub fn acquire(
    rx: &[Complex32],
    preamble: &[Complex32],
    search_start: usize,
    search_end: usize,
) -> Option<Acquisition> {
    let l = preamble.len();
    if l < 2 || rx.len() < l {
        return None;
    }
    let last_tau = rx.len() - l; // last τ that still fits the whole preamble
    let lo = search_start.min(last_tau);
    let hi = search_end.min(last_tau + 1).max(lo + 1);

    let preamble_energy: f32 = preamble.iter().map(|p| p.norm_sqr()).sum();
    if preamble_energy <= 0.0 {
        return None;
    }
    let conj_pre: Vec<Complex32> = preamble.iter().map(|p| p.conj()).collect();

    let mut planner = FftPlanner::<f32>::new();
    let coarse_fft = planner.plan_fft_forward(l);

    // ── Pass 1: joint coarse timing + CFO ──────────────────────────────────────
    let mut best_tau = lo;
    let mut best_metric = -1.0f32;
    let mut scratch = vec![Complex32::new(0.0, 0.0); l];
    for tau in lo..hi {
        for n in 0..l {
            scratch[n] = rx[tau + n] * conj_pre[n];
        }
        coarse_fft.process(&mut scratch);
        let peak_pow = scratch.iter().map(|c| c.norm_sqr()).fold(0.0f32, f32::max);
        // Normalise by the received window energy so timing prefers a true
        // preamble match over a high-energy noise burst (Cauchy–Schwarz bound).
        let rx_energy: f32 = rx[tau..tau + l].iter().map(|c| c.norm_sqr()).sum();
        let metric = if rx_energy > 0.0 {
            peak_pow / (rx_energy * preamble_energy)
        } else {
            0.0
        };
        if metric > best_metric {
            best_metric = metric;
            best_tau = tau;
        }
    }

    // ── Pass 2: fine CFO + phase + gain at the winning τ (zero-padded FFT) ──────
    let m = (l * 4).next_power_of_two();
    let fine_fft = planner.plan_fft_forward(m);
    let mut buf = vec![Complex32::new(0.0, 0.0); m];
    for n in 0..l {
        buf[n] = rx[best_tau + n] * conj_pre[n];
    }
    fine_fft.process(&mut buf);

    // Peak bin and parabolic (quadratic) interpolation on magnitude.
    let mut k_peak = 0usize;
    let mut peak_mag = -1.0f32;
    for (k, c) in buf.iter().enumerate() {
        let mag = c.norm();
        if mag > peak_mag {
            peak_mag = mag;
            k_peak = k;
        }
    }
    let mag = |k: usize| buf[(k + m) % m].norm();
    let a = mag((k_peak + m - 1) % m);
    let b = peak_mag;
    let c = mag((k_peak + 1) % m);
    let denom = a - 2.0 * b + c;
    let delta = if denom.abs() > f32::EPSILON {
        (0.5 * (a - c) / denom).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    let k_fine = k_peak as f32 + delta;
    // Map [0, m) bins to signed [-m/2, m/2) and normalise to cycles/sample.
    let k_signed = if k_fine >= m as f32 / 2.0 {
        k_fine - m as f32
    } else {
        k_fine
    };
    let cfo = k_signed / m as f32;

    // Phase and gain from the de-rotated sum evaluated at the *exact* refined CFO
    // (a one-shot DTFT). Reading the integer FFT bin instead would carry the
    // Dirichlet-kernel linear phase of the bin-vs-true-frequency residual (a
    // ~0.2 rad phase error) and scalloping loss in the magnitude.
    let mut acc = Complex32::new(0.0, 0.0);
    for n in 0..l {
        let ph = -2.0 * std::f32::consts::PI * cfo * n as f32;
        acc += rx[best_tau + n] * conj_pre[n] * Complex32::new(ph.cos(), ph.sin());
    }
    let phase = acc.im.atan2(acc.re);
    // |acc| ≈ γ·Σ|p|² on the CFO; divide it out for the amplitude scale.
    let gain = acc.norm() / preamble_energy;

    Some(Acquisition {
        timing: best_tau,
        cfo_cycles_per_sample: cfo,
        phase,
        gain,
        metric: best_metric.clamp(0.0, 1.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    /// A length-`l` constant-modulus (BPSK ±1) pseudo-random preamble.
    fn make_preamble(l: usize) -> Vec<Complex32> {
        let mut state = 0x2A_u32;
        (0..l)
            .map(|_| {
                state = state.wrapping_mul(1103515245).wrapping_add(12345);
                let bit = (state >> 16) & 1;
                Complex32::new(if bit == 0 { 1.0 } else { -1.0 }, 0.0)
            })
            .collect()
    }

    /// Embed `preamble` at `offset` in a noise window, with applied CFO/phase/gain.
    #[allow(clippy::too_many_arguments)]
    fn synth(
        preamble: &[Complex32],
        total: usize,
        offset: usize,
        cfo: f32,
        phase: f32,
        gain: f32,
        noise: f32,
        seed: u32,
    ) -> Vec<Complex32> {
        let mut state = seed;
        let mut rng = || {
            state = state.wrapping_mul(1103515245).wrapping_add(12345);
            ((state >> 8) & 0xFFFF) as f32 / 65535.0 - 0.5
        };
        let mut rx = vec![Complex32::new(0.0, 0.0); total];
        for s in rx.iter_mut() {
            *s = Complex32::new(rng() * noise, rng() * noise);
        }
        for (n, &p) in preamble.iter().enumerate() {
            let ph = 2.0 * PI * cfo * n as f32 + phase;
            let rot = Complex32::new(ph.cos(), ph.sin());
            rx[offset + n] += p * rot * gain;
        }
        rx
    }

    #[test]
    fn recovers_timing_cfo_phase_clean() {
        let pre = make_preamble(256);
        let (offset, cfo, phase, gain) = (300usize, 0.012f32, 0.7f32, 1.5f32);
        let rx = synth(&pre, 1024, offset, cfo, phase, gain, 0.0, 1);
        let a = acquire(&rx, &pre, 0, 1024).expect("acquire");
        assert_eq!(a.timing, offset, "timing");
        assert!(
            (a.cfo_cycles_per_sample - cfo).abs() < 1e-3,
            "cfo {}",
            a.cfo_cycles_per_sample
        );
        // phase wraps; compare on the unit circle.
        let dphi = (a.phase - phase).rem_euclid(2.0 * PI);
        assert!(!(0.1..=2.0 * PI - 0.1).contains(&dphi), "phase {}", a.phase);
        assert!((a.gain - gain).abs() / gain < 0.1, "gain {}", a.gain);
        assert!(a.metric > 0.9, "metric {}", a.metric);
    }

    #[test]
    fn recovers_cfo_across_range() {
        let pre = make_preamble(256);
        // ±0.06 cycles/sample spans well beyond the per-symbol ambiguity a
        // differential estimator wraps at; the FFT resolves the bin.
        for &cfo in &[-0.06f32, -0.03, -0.005, 0.0, 0.005, 0.03, 0.06] {
            let rx = synth(&pre, 1024, 256, cfo, 0.3, 1.0, 0.0, 7);
            let a = acquire(&rx, &pre, 0, 1024).expect("acquire");
            assert!(
                (a.cfo_cycles_per_sample - cfo).abs() < 1.5e-3,
                "cfo {cfo} -> {}",
                a.cfo_cycles_per_sample
            );
            assert_eq!(a.timing, 256, "timing at cfo {cfo}");
        }
    }

    #[test]
    fn robust_to_awgn() {
        let pre = make_preamble(256);
        let (offset, cfo) = (400usize, 0.02f32);
        let mut ok = 0;
        for seed in 0..12u32 {
            // noise amplitude 0.5 vs unit preamble ≈ moderate SNR
            let rx = synth(&pre, 1200, offset, cfo, 1.0, 1.0, 0.5, seed * 17 + 3);
            let a = acquire(&rx, &pre, 0, 1200).expect("acquire");
            if a.timing.abs_diff(offset) <= 2 && (a.cfo_cycles_per_sample - cfo).abs() < 3e-3 {
                ok += 1;
            }
        }
        assert!(ok >= 11, "AWGN acquisition {ok}/12");
    }

    #[test]
    fn metric_low_on_noise_only() {
        let pre = make_preamble(256);
        let rx = synth(&pre, 1024, 0, 0.0, 0.0, 0.0, 0.5, 99); // gain 0 → no preamble
        let a = acquire(&rx, &pre, 0, 1024).expect("acquire");
        assert!(a.metric < 0.3, "noise metric {}", a.metric);
    }

    #[test]
    fn too_short_returns_none() {
        let pre = make_preamble(256);
        assert!(acquire(&[Complex32::new(1.0, 0.0); 10], &pre, 0, 10).is_none());
        assert!(acquire(&[], &pre, 0, 0).is_none());
    }

    #[test]
    fn bounded_search_window() {
        let pre = make_preamble(128);
        let rx = synth(&pre, 2048, 1500, 0.01, 0.0, 1.0, 0.3, 5);
        // Search only a window around the true onset.
        let a = acquire(&rx, &pre, 1450, 1560).expect("acquire");
        assert_eq!(a.timing, 1500);
    }
}
