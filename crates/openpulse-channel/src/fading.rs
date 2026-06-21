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
