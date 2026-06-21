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
/// Uses a single FFT of length ≥ n (next power of two) so all samples within the call share
/// a single coherent realization — correct temporal correlation across the full signal length
/// rather than independent states at fixed block boundaries.
///
/// For low Doppler spreads (e.g. F1 = 0.1 Hz) the signal-length FFT alone yields a sub-bin
/// shaping filter (σ_bins ≪ 1) that would collapse to the 0.5 floor and produce a near-constant
/// envelope. The FFT is therefore enlarged so σ_bins ≥ `TARGET_SIGMA_BINS`, up to `MAX_FFT`
/// samples (~2 MB of `Complex<f32>`).
pub(crate) fn doppler_envelope(
    rng: &mut StdRng,
    planner: &mut FftPlanner<f32>,
    n: usize,
    doppler_spread_hz: f32,
    sample_rate: u32,
) -> Vec<Complex32> {
    const TARGET_SIGMA_BINS: f32 = 2.0;
    const MAX_FFT: usize = 1 << 18;
    let signal_fft = n.next_power_of_two().max(4);
    let sr = sample_rate as f32;
    let required_fft = if doppler_spread_hz > 1e-4 {
        (TARGET_SIGMA_BINS * sr / doppler_spread_hz).ceil() as usize
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

    // Gaussian Doppler shaping: sigma = doppler_hz / bin_width. `fft_size` is sized above so
    // σ_bins ≥ TARGET_SIGMA_BINS for non-trivial Doppler; the 0.5 floor remains as
    // defense-in-depth for the doppler≈0 case and for the MAX_FFT cap.
    let sigma_bins = (doppler_spread_hz / (sr / fft_size as f32)).max(0.5);

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
        let h = (-0.5 * (freq / sigma_bins).powi(2)).exp();
        *s *= h;
    }

    // IFFT to time domain.
    let ifft = planner.plan_fft_inverse(fft_size);
    ifft.process(&mut spec);

    // Normalize to unit mean-square. For rustfft's unnormalized IFFT, each time-domain sample
    // satisfies E[|h[n]|^2] = Σ_k E[|X[k]|^2] = 2·filter_energy (independent of fft_size — the
    // 1/N from Parseval cancels the N from the unnormalized transform).
    let scale = 1.0 / (2.0 * filter_energy).sqrt();
    spec[..n]
        .iter()
        .map(|c| Complex32::new(c.re * scale, c.im * scale))
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
