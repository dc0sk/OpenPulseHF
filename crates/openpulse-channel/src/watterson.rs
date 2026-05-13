//! Watterson two-ray ITU-R F.1487 ionospheric channel.
//!
//! Models two delayed Rayleigh-faded rays, each with independent complex
//! Gaussian fading envelopes shaped by a Doppler-spread spectral filter.
//!
//! # Implementation notes
//!
//! The Doppler envelope is synthesised in the frequency domain:
//!   1. Generate N complex Gaussian samples where N = next_power_of_two(signal_len).
//!   2. FFT → apply Gaussian spectral filter centred at DC with σ proportional
//!      to `doppler_spread_hz / (sample_rate / N)`.
//!   3. IFFT → time-domain fading envelope (first `signal_len` samples used).
//!   4. Scale so that E[|h|²] = 1.
//!
//! Using N = next_power_of_two(signal_len) gives proper temporal correlation over the
//! full call rather than jumping to independent states at fixed-size block boundaries.

use rand::SeedableRng;
use rand_distr::{Distribution, Normal};
use rustfft::{num_complex::Complex, FftPlanner};

type Complex32 = Complex<f32>;

use crate::{ChannelError, ChannelModel, WattersonConfig};

/// Two-ray Watterson ionospheric fading channel.
pub struct WattersonChannel {
    config: WattersonConfig,
    rng: rand::rngs::StdRng,
    planner: FftPlanner<f32>,
}

impl WattersonChannel {
    pub fn new(config: WattersonConfig) -> Result<Self, ChannelError> {
        if !config.doppler_spread_hz.is_finite() || config.doppler_spread_hz < 0.0 {
            return Err(ChannelError::InvalidParameter(
                "doppler_spread_hz must be a non-negative finite value".into(),
            ));
        }
        if !config.snr_db.is_finite() {
            return Err(ChannelError::InvalidParameter(
                "snr_db must be finite".into(),
            ));
        }
        if !config.delay_spread_ms.is_finite() || config.delay_spread_ms < 0.0 {
            return Err(ChannelError::InvalidParameter(
                "delay_spread_ms must be a non-negative finite value".into(),
            ));
        }
        if config.sample_rate == 0 {
            return Err(ChannelError::InvalidParameter(
                "sample_rate must be > 0".into(),
            ));
        }

        let rng = match config.seed {
            Some(s) => rand::rngs::StdRng::seed_from_u64(s),
            None => rand::rngs::StdRng::from_entropy(),
        };
        let planner = FftPlanner::new();

        Ok(Self {
            config,
            rng,
            planner,
        })
    }

    /// Generate `n` fading envelope samples with Doppler-spectrum shaping.
    ///
    /// Uses a single FFT of length ≥ n (next power of two) so all samples within
    /// the call share a single coherent realization.  This gives correct temporal
    /// correlation across the full signal length rather than jumping to independent
    /// states at each fixed-size block boundary.
    fn make_envelope(&mut self, n: usize) -> Vec<Complex32> {
        let fft_size = n.next_power_of_two().max(4);
        let normal = Normal::new(0.0f32, 1.0).unwrap();

        // Random complex Gaussian spectrum.
        let mut spec: Vec<Complex<f32>> = (0..fft_size)
            .map(|_| Complex::new(normal.sample(&mut self.rng), normal.sample(&mut self.rng)))
            .collect();

        // Gaussian Doppler shaping: sigma = doppler_hz / bin_width.
        // With fft_size ≥ n (the full signal length), one bin is sample_rate/fft_size Hz,
        // giving meaningful resolution for real Doppler spreads rather than sub-bin clamping.
        let sr = self.config.sample_rate as f32;
        let sigma_bins = (self.config.doppler_spread_hz / (sr / fft_size as f32)).max(0.5);

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
        let ifft = self.planner.plan_fft_inverse(fft_size);
        ifft.process(&mut spec);

        // Normalize to unit mean-square: E[|h|^2] = 2*filter_energy/fft_size (IFFT is unscaled).
        let scale = 1.0 / (2.0 * filter_energy).sqrt();
        spec[..n]
            .iter()
            .map(|c| Complex32::new(c.re * scale, c.im * scale))
            .collect()
    }
}

impl ChannelModel for WattersonChannel {
    fn apply(&mut self, input: &[f32]) -> Vec<f32> {
        let n = input.len();
        if n == 0 {
            return Vec::new();
        }

        // Generate full-length envelopes so the fading is temporally correlated
        // across the entire call — no discontinuous jumps at fixed block boundaries.
        let env0 = self.make_envelope(n);
        let env1 = self.make_envelope(n);

        let delay_samples =
            (self.config.delay_spread_ms / 1000.0 * self.config.sample_rate as f32) as usize;
        let rms = (input.iter().map(|&s| s * s).sum::<f32>() / n as f32).sqrt();
        let noise_sigma = if rms > 0.0 {
            rms / 10f32.powf(self.config.snr_db / 20.0)
        } else {
            1e-4
        };
        let noise_dist = Normal::new(0.0f32, noise_sigma).unwrap();

        // Use the real part of the complex fading coefficient (× √2 for unit mean-square).
        // The complex Gaussian envelope encodes a random carrier phase in its argument.
        // Using only the magnitude (norm) discards this phase, creating deterministic
        // frequency-domain nulls when the delay phase (2π·fc·τ) happens to equal an odd
        // multiple of π.  Using the real part randomises the sign, breaking static nulls.
        let sqrt2 = std::f32::consts::SQRT_2;
        let mut out = vec![0.0f32; n];
        for i in 0..n {
            // Ray 0: direct path; effective amplitude = Re[h0] × √2.
            let ray0 = input[i] * env0[i].re * sqrt2;
            // Ray 1: delayed path (zero-padded for samples before the buffer).
            let ray1 = if i >= delay_samples {
                input[i - delay_samples] * env1[i].re * sqrt2
            } else {
                0.0
            };
            out[i] = ray0 + ray1 + noise_dist.sample(&mut self.rng);
        }
        out
    }

    fn generate_noise(&mut self, length: usize) -> Vec<f32> {
        // Multiplicative fading is not independent additive noise.
        vec![0.0; length]
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WattersonConfig;

    /// The Watterson output must exhibit non-trivial amplitude variation across
    /// blocks (coefficient of variation > 10 %).
    ///
    /// Uses the extreme profile (10 Hz Doppler) where sigma_bins > 1 so the
    /// fading envelope varies meaningfully within each 1024-sample window.
    #[test]
    fn non_trivial_fading_envelope() {
        let cfg = WattersonConfig::extreme(Some(7));
        let mut ch = WattersonChannel::new(cfg).unwrap();

        let block_size = 256usize;
        let n_blocks = 20usize;
        let signal: Vec<f32> = (0..block_size)
            .map(|i| (2.0 * std::f32::consts::PI * 1500.0 * i as f32 / 8000.0).sin())
            .collect();

        let mut block_rms: Vec<f32> = Vec::with_capacity(n_blocks);
        for _ in 0..n_blocks {
            let out = ch.apply(&signal);
            let rms = (out.iter().map(|&s| s * s).sum::<f32>() / block_size as f32).sqrt();
            block_rms.push(rms);
        }

        let mean = block_rms.iter().sum::<f32>() / n_blocks as f32;
        let variance = block_rms.iter().map(|&r| (r - mean).powi(2)).sum::<f32>() / n_blocks as f32;
        let cv = variance.sqrt() / mean; // coefficient of variation

        assert!(
            cv > 0.10,
            "coefficient of variation {:.3} should be > 0.10 (non-trivial fading)",
            cv
        );
    }

    #[test]
    fn rejects_negative_doppler() {
        let mut cfg = WattersonConfig::moderate_f1(None);
        cfg.doppler_spread_hz = -1.0;
        assert!(WattersonChannel::new(cfg).is_err());
    }
}
