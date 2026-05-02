//! Watterson two-ray ITU-R F.1487 ionospheric channel.
//!
//! Models two delayed Rayleigh-faded rays, each with independent complex
//! Gaussian fading envelopes shaped by a Doppler-spread spectral filter.
//!
//! # Implementation notes
//!
//! The Doppler envelope is synthesised in the frequency domain:
//!   1. Generate ENVELOPE_FFT_SIZE complex Gaussian samples.
//!   2. FFT → apply Gaussian spectral filter centred at DC with σ proportional
//!      to `doppler_spread_hz / sample_rate`.
//!   3. IFFT → time-domain fading envelope.
//!   4. Scale so that the inter-block amplitude variation is Rayleigh-distributed.
//!
//! For the Good F1 profile (Doppler = 0.1 Hz at 8000 Hz), the Doppler spread
//! is sub-bin at FFT_SIZE=1024 (7.8 Hz/bin).  The resulting envelope is nearly
//! constant-amplitude rather than truly diffuse fading — this is expected and
//! documented per the sharp-edges section of CLAUDE.md.

use rand::SeedableRng;
use rand_distr::{Distribution, Normal};
use rustfft::{num_complex::Complex, FftPlanner};

type Complex32 = Complex<f32>;

use crate::{ChannelError, ChannelModel, WattersonConfig};

/// FFT size for Doppler envelope generation.
const ENVELOPE_FFT_SIZE: usize = 1024;

/// Two-ray Watterson ionospheric fading channel.
pub struct WattersonChannel {
    config: WattersonConfig,
    rng: rand::rngs::StdRng,
    planner: FftPlanner<f32>,
    /// Sample counter for continuous phase tracking.
    sample_idx: usize,
    /// Pre-computed fading coefficients for ray 0 and ray 1.
    env0: Vec<Complex32>,
    env1: Vec<Complex32>,
    env_cursor: usize,
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

        let mut ch = Self {
            config,
            rng,
            planner,
            sample_idx: 0,
            env0: Vec::new(),
            env1: Vec::new(),
            env_cursor: 0,
        };
        ch.refill_envelopes();
        Ok(ch)
    }

    /// Generate ENVELOPE_FFT_SIZE fading coefficients for both rays.
    fn refill_envelopes(&mut self) {
        self.env0 = self.make_envelope();
        self.env1 = self.make_envelope();
        self.env_cursor = 0;
    }

    fn make_envelope(&mut self) -> Vec<Complex32> {
        let n = ENVELOPE_FFT_SIZE;
        let normal = Normal::new(0.0f32, 1.0).unwrap();

        // Random complex Gaussian spectrum.
        let mut spec: Vec<Complex<f32>> = (0..n)
            .map(|_| Complex::new(normal.sample(&mut self.rng), normal.sample(&mut self.rng)))
            .collect();

        // Apply Gaussian Doppler shaping filter in frequency domain.
        // Bin k corresponds to frequency k * sample_rate / n.
        let sr = self.config.sample_rate as f32;
        let sigma_bins = self.config.doppler_spread_hz / (sr / n as f32);
        // Minimum 0.5-bin width so the filter is never a Dirac delta.
        let sigma_bins = sigma_bins.max(0.5);

        let filter_energy: f32 = (0..n)
            .map(|k| {
                let freq = if k <= n / 2 {
                    k as f32
                } else {
                    k as f32 - n as f32
                };
                (-0.5 * (freq / sigma_bins).powi(2)).exp().powi(2)
            })
            .sum::<f32>();

        for (k, s) in spec.iter_mut().enumerate() {
            let freq = if k <= n / 2 {
                k as f32
            } else {
                k as f32 - n as f32
            };
            let h = (-0.5 * (freq / sigma_bins).powi(2)).exp();
            *s *= h;
        }

        // IFFT to get time-domain fading envelope.
        let ifft = self.planner.plan_fft_inverse(n);
        ifft.process(&mut spec);

        // Normalize so fading samples have unit mean-square amplitude:
        // E[|h_ifft(t)|^2] = 2 * filter_energy (rustfft IFFT is unscaled)
        // scale = 1 / sqrt(2 * filter_energy) → E[|h_scaled|^2] = 1
        let scale = 1.0 / (2.0 * filter_energy).sqrt();
        spec.iter()
            .map(|c| Complex32::new(c.re * scale, c.im * scale))
            .collect()
    }

    fn fading_coeff(&mut self, sample: usize) -> (Complex32, Complex32) {
        if self.env_cursor + sample >= self.env0.len() {
            self.refill_envelopes();
        }
        let idx = (self.env_cursor + sample) % ENVELOPE_FFT_SIZE;
        (self.env0[idx], self.env1[idx])
    }
}

impl ChannelModel for WattersonChannel {
    fn apply(&mut self, input: &[f32]) -> Vec<f32> {
        if input.is_empty() {
            return Vec::new();
        }

        let delay_samples =
            (self.config.delay_spread_ms / 1000.0 * self.config.sample_rate as f32) as usize;
        let n = input.len();
        let rms = (input.iter().map(|&s| s * s).sum::<f32>() / n as f32).sqrt();
        let noise_sigma = if rms > 0.0 {
            rms / 10f32.powf(self.config.snr_db / 20.0)
        } else {
            1e-4
        };
        let noise_dist = Normal::new(0.0f32, noise_sigma).unwrap();

        let mut out = vec![0.0f32; n];
        for i in 0..n {
            let (h0, h1) = self.fading_coeff(i);
            // Ray 0: direct path; use Rayleigh envelope magnitude.
            let ray0 = input[i] * h0.norm();
            // Ray 1: delayed path (zero-padded for samples before the buffer).
            let ray1 = if i >= delay_samples {
                input[i - delay_samples] * h1.norm()
            } else {
                0.0
            };
            out[i] = ray0 + ray1 + noise_dist.sample(&mut self.rng);
        }
        self.env_cursor += n;
        self.sample_idx += n;
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
