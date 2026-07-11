//! Shared fading DSP primitives used by the Watterson and flat-fading channels.
//!
//! Both channels apply a complex, Doppler-shaped Rayleigh gain to the analytic signal of a
//! real passband input — `out = Re{ analytic(s) · h }` — which preserves the Rayleigh
//! magnitude `|h|` and applies a true carrier-phase rotation. Keeping these in one place
//! avoids divergence in the (load-bearing) envelope and Hilbert routines.

use rand::rngs::StdRng;
use rand::Rng;
use rand_distr::StandardNormal;
use rustfft::{num_complex::Complex, FftPlanner};

type Complex32 = Complex<f32>;

/// Generate `n` complex Doppler-shaped Rayleigh fading envelope samples (E[|h|²] = 1).
///
/// The fading process is band-limited to the Doppler spread, so it is generated at a low
/// internal rate `fs_env ≫ doppler` and linearly interpolated up to the signal rate. At the
/// signal rate the envelope is hugely oversampled relative to its bandwidth (e.g. a 0.1 Hz
/// process at 8 kHz), so interpolation is essentially exact — while the shaping FFT stays
/// small. Generating directly at the signal rate would force a ~2^18 FFT for F1's 0.1 Hz
/// (bin_width must be ≤ doppler/2), which dominated channel CPU.
///
/// A single coherent realization spans the whole call (one shaping FFT), so the temporal
/// correlation is correct across the full length rather than resetting at block boundaries.
pub(crate) fn doppler_envelope(
    rng: &mut StdRng,
    planner: &mut FftPlanner<f32>,
    n: usize,
    doppler_spread_hz: f32,
    sample_rate: u32,
) -> Vec<Complex32> {
    const TARGET_SIGMA_BINS: f32 = 2.0;
    const MAX_FFT: usize = 1 << 16;
    let sr = sample_rate as f32;

    // Low internal rate: ≥ ~8 samples per Doppler cycle, floored at 50 Hz, capped at the
    // signal rate (decim = 1, i.e. no decimation, only if doppler is absurdly high).
    let fs_env = (doppler_spread_hz * 8.0).clamp(50.0, sr);
    let decim = sr / fs_env;
    // Low-rate sample count covering the n high-rate samples (+2 guard for the interp tail).
    let n_env = ((n as f32 / decim).ceil() as usize + 2).max(4);

    let signal_fft = n_env.next_power_of_two().max(4);
    let required_fft = if doppler_spread_hz > 1e-4 {
        (TARGET_SIGMA_BINS * fs_env / doppler_spread_hz).ceil() as usize
    } else {
        signal_fft
    };
    let fft_size = signal_fft.max(required_fft.next_power_of_two().min(MAX_FFT));

    // Random complex Gaussian spectrum.
    let mut spec: Vec<Complex<f32>> = (0..fft_size)
        .map(|_| {
            Complex::new(
                rng.sample::<f32, _>(StandardNormal),
                rng.sample::<f32, _>(StandardNormal),
            )
        })
        .collect();

    // Gaussian Doppler shaping at the fs_env bin scale; the 0.5 floor is defense-in-depth for
    // the doppler≈0 case and the MAX_FFT cap.
    let sigma_bins = (doppler_spread_hz / (fs_env / fft_size as f32)).max(0.5);
    let filter_energy: f32 = (0..fft_size)
        .map(|k| {
            let freq = if k <= fft_size / 2 {
                k as f32
            } else {
                k as f32 - fft_size as f32
            };
            (-0.5 * (freq / sigma_bins).powi(2)).exp().powi(2)
        })
        .sum::<f32>();
    for (k, s) in spec.iter_mut().enumerate() {
        let freq = if k <= fft_size / 2 {
            k as f32
        } else {
            k as f32 - fft_size as f32
        };
        *s *= (-0.5 * (freq / sigma_bins).powi(2)).exp();
    }

    let ifft = planner.plan_fft_inverse(fft_size);
    ifft.process(&mut spec);

    // Normalize to unit mean-square (the 1/N from Parseval cancels rustfft's unnormalized
    // IFFT, so E[|h|²] = 2·filter_energy independent of fft_size).
    let scale = 1.0 / (2.0 * filter_energy).sqrt();
    let env_lo: Vec<Complex32> = spec[..n_env]
        .iter()
        .map(|c| Complex32::new(c.re * scale, c.im * scale))
        .collect();

    // Linear-interpolate the low-rate envelope up to n samples at the signal rate.
    let last = env_lo.len() - 1;
    (0..n)
        .map(|i| {
            let pos = i as f32 / decim;
            let j = pos.floor() as usize;
            let frac = pos - j as f32;
            let a = env_lo[j.min(last)];
            let b = env_lo[(j + 1).min(last)];
            Complex32::new(a.re + (b.re - a.re) * frac, a.im + (b.im - a.im) * frac)
        })
        .collect()
}

/// Number of oscillators in the sum-of-sinusoids fader — enough for a near-Gaussian `h` (CLT).
const SOS_OSCILLATORS: usize = 48;

/// Phase-continuous Rayleigh fader with a Gaussian Doppler PSD (E[|h|²] = 1).
///
/// Unlike [`doppler_envelope`], which synthesises one self-contained realization per call and so
/// *re-randomises* the fade at every `apply()` boundary, this holds oscillator phase as state:
/// [`next_block`](Self::next_block) resumes where the previous block ended. A caller that feeds the
/// channel frame-by-frame therefore sees one temporally-correlated fade — consecutive frames are
/// correlated at low Doppler (long coherence) and decorrelate at high Doppler — instead of an
/// independent draw per frame. `h[k] = (1/√M) Σ_m exp(j(2π f_m k/fs + φ_m))` with `f_m ~ N(0, σ_d)`
/// (a Gaussian spread of Doppler shifts → Gaussian PSD) and `φ_m ~ U(0, 2π)`, drawn once at
/// construction; the random phases make the cross terms vanish in expectation, so E[|h|²] = 1.
pub(crate) struct SosFader {
    /// Per-oscillator angular increment (rad/sample).
    omega: Vec<f64>,
    /// Per-oscillator running phase, wrapped to `[0, 2π)` (f64 to avoid drift over long runs).
    phase: Vec<f64>,
    /// `1/√M` amplitude normalisation for E[|h|²] = 1.
    scale: f32,
}

impl SosFader {
    /// Draw a fresh fading realization (fixed Doppler shifts + phases) from `rng`.
    pub(crate) fn new(rng: &mut StdRng, doppler_spread_hz: f32, sample_rate: u32) -> Self {
        let fs = sample_rate as f64;
        let sigma = doppler_spread_hz as f64;
        let two_pi = 2.0 * std::f64::consts::PI;
        let mut omega = Vec::with_capacity(SOS_OSCILLATORS);
        let mut phase = Vec::with_capacity(SOS_OSCILLATORS);
        for _ in 0..SOS_OSCILLATORS {
            let f_hz: f64 = rng.sample::<f64, _>(StandardNormal) * sigma;
            omega.push(two_pi * f_hz / fs);
            phase.push(rng.gen::<f64>() * two_pi);
        }
        Self {
            omega,
            phase,
            scale: (SOS_OSCILLATORS as f32).sqrt().recip(),
        }
    }

    /// Emit the next `n` fading coefficients, advancing internal phase (continuous across calls).
    pub(crate) fn next_block(&mut self, n: usize) -> Vec<Complex32> {
        let two_pi = 2.0 * std::f64::consts::PI;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let mut re = 0.0f64;
            let mut im = 0.0f64;
            for (ph, &w) in self.phase.iter_mut().zip(self.omega.iter()) {
                re += ph.cos();
                im += ph.sin();
                *ph += w;
                if *ph >= two_pi {
                    *ph -= two_pi;
                } else if *ph < 0.0 {
                    *ph += two_pi;
                }
            }
            out.push(Complex32::new(
                re as f32 * self.scale,
                im as f32 * self.scale,
            ));
        }
        out
    }
}

/// Analytic signal of a real input via the FFT Hilbert method (re = input, im = Hilbert).
pub(crate) fn analytic_signal(planner: &mut FftPlanner<f32>, x: &[f32]) -> Vec<Complex32> {
    let n = x.len();
    let mut buf: Vec<Complex32> = x.iter().map(|&v| Complex32::new(v, 0.0)).collect();
    planner.plan_fft_forward(n).process(&mut buf);
    let half = n.div_ceil(2); // index of the first negative-frequency bin
    for v in buf.iter_mut().take(half).skip(1) {
        *v *= 2.0; // double the positive frequencies
    }
    for v in buf.iter_mut().skip(half) {
        *v = Complex32::new(0.0, 0.0); // zero the negative frequencies
    }
    planner.plan_fft_inverse(n).process(&mut buf);
    let scale = 1.0 / n as f32;
    for v in buf.iter_mut() {
        *v *= scale;
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    /// Audit H8: `analytic_signal` must reproduce the real input and hold a constant envelope for a
    /// pure tone — the property the Watterson quadrature fix (Re{analytic(s)·h}) depends on.
    #[test]
    fn analytic_signal_of_a_cosine_has_constant_envelope_and_recovers_the_real_part() {
        let n = 512;
        let f = 8.0; // cycles over the window
        let x: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * f * i as f32 / n as f32).cos())
            .collect();
        let mut planner = FftPlanner::<f32>::new();
        let a = analytic_signal(&mut planner, &x);

        // Interior samples (avoid FFT edge ringing): Re{analytic} ≈ input, |analytic| ≈ amplitude 1.
        for i in n / 8..n - n / 8 {
            assert!((a[i].re - x[i]).abs() < 1e-2, "re mismatch at {i}");
            assert!((a[i].norm() - 1.0).abs() < 5e-2, "envelope not ~1 at {i}");
        }
    }

    /// A generated Doppler envelope must be power-normalised (~unit mean power) and non-trivially
    /// time-varying — a flat or zero envelope is the "multipath improves decode" model-bug signature.
    #[test]
    fn doppler_envelope_is_power_normalised_and_varies() {
        let mut rng = StdRng::seed_from_u64(1);
        let mut planner = FftPlanner::<f32>::new();
        let n = 8192;
        let env = doppler_envelope(&mut rng, &mut planner, n, 1.0, 8000);
        assert_eq!(env.len(), n);

        let mean_power: f32 = env.iter().map(|c| c.norm_sqr()).sum::<f32>() / n as f32;
        assert!(
            (0.5..2.0).contains(&mean_power),
            "mean power {mean_power} should be ~1 (normalised)"
        );

        let mags: Vec<f32> = env.iter().map(|c| c.norm()).collect();
        let max = mags.iter().cloned().fold(0.0f32, f32::max);
        let min = mags.iter().cloned().fold(f32::MAX, f32::min);
        assert!(
            max - min > 0.05,
            "envelope is essentially flat (span {})",
            max - min
        );
    }
}
