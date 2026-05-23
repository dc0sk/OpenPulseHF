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

use rand::Rng;
use rand::SeedableRng;
use rand_distr::StandardNormal;
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
    ///
    /// For low Doppler spreads (e.g. F1 = 0.1 Hz), the signal-length FFT alone
    /// yields a sub-bin shaping filter (σ_bins ≪ 1) that would collapse to the
    /// 0.5 floor and produce a near-constant envelope.  The FFT is therefore
    /// enlarged so σ_bins ≥ `TARGET_SIGMA_BINS`, up to `MAX_FFT` samples
    /// (~2 MB of `Complex<f32>`).
    fn make_envelope(&mut self, n: usize) -> Vec<Complex32> {
        const TARGET_SIGMA_BINS: f32 = 2.0;
        const MAX_FFT: usize = 1 << 18;
        let signal_fft = n.next_power_of_two().max(4);
        let sr = self.config.sample_rate as f32;
        let required_fft = if self.config.doppler_spread_hz > 1e-4 {
            (TARGET_SIGMA_BINS * sr / self.config.doppler_spread_hz).ceil() as usize
        } else {
            signal_fft
        };
        let fft_size = signal_fft.max(required_fft.next_power_of_two().min(MAX_FFT));
        // Random complex Gaussian spectrum.
        let mut spec: Vec<Complex<f32>> = (0..fft_size)
            .map(|_| {
                Complex::new(
                    self.rng.sample::<f32, _>(StandardNormal),
                    self.rng.sample::<f32, _>(StandardNormal),
                )
            })
            .collect();

        // Gaussian Doppler shaping: sigma = doppler_hz / bin_width.
        // `fft_size` is sized above so σ_bins ≥ TARGET_SIGMA_BINS for non-trivial
        // Doppler; the 0.5 floor remains as defense-in-depth for the doppler≈0 case
        // and for the MAX_FFT cap.
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

        // Normalize to unit mean-square.  For rustfft's unnormalized IFFT, each time-domain
        // sample satisfies E[|h[n]|^2] = Σ_k E[|X[k]|^2] = 2·filter_energy (independent of
        // fft_size — the 1/N from Parseval cancels the N from the unnormalized transform).
        let scale = 1.0 / (2.0 * filter_energy).sqrt();
        spec[..n]
            .iter()
            .map(|c| Complex32::new(c.re * scale, c.im * scale))
            .collect()
    }

    /// Apply Watterson fading to complex baseband input using one coherent
    /// channel realization for both I and Q rails.
    pub fn apply_complex(&mut self, i_in: &[f32], q_in: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let n = i_in.len().min(q_in.len());
        if n == 0 {
            return (Vec::new(), Vec::new());
        }

        let env0 = self.make_envelope(n);
        let env1 = self.make_envelope(n);
        let delay_samples =
            (self.config.delay_spread_ms / 1000.0 * self.config.sample_rate as f32) as usize;

        let signal_rms = (i_in
            .iter()
            .zip(q_in.iter())
            .take(n)
            .map(|(&i, &q)| i * i + q * q)
            .sum::<f32>()
            / n as f32)
            .sqrt();

        // Per-component sigma so the total complex-noise RMS tracks the requested SNR.
        let noise_sigma = if signal_rms > 0.0 {
            signal_rms / (10f32.powf(self.config.snr_db / 20.0) * std::f32::consts::SQRT_2)
        } else {
            1e-4
        };

        let mut out_i = vec![0.0_f32; n];
        let mut out_q = vec![0.0_f32; n];

        for idx in 0..n {
            let x0 = Complex32::new(i_in[idx], q_in[idx]);
            let x1 = if idx >= delay_samples {
                Complex32::new(i_in[idx - delay_samples], q_in[idx - delay_samples])
            } else {
                Complex32::new(0.0, 0.0)
            };

            let y = env0[idx] * x0 + env1[idx] * x1;
            out_i[idx] = y.re + noise_sigma * self.rng.sample::<f32, _>(StandardNormal);
            out_q[idx] = y.im + noise_sigma * self.rng.sample::<f32, _>(StandardNormal);
        }

        (out_i, out_q)
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
            out[i] = ray0 + ray1 + noise_sigma * self.rng.sample::<f32, _>(StandardNormal);
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

    /// The Good-F1 profile (Doppler = 0.1 Hz) historically collapsed to a
    /// near-constant envelope because the shaping σ_bins fell below the 0.5
    /// floor.  After auto-sizing the FFT for low-Doppler resolution, the
    /// envelope must show non-trivial variation across a full call.
    ///
    /// At 0.1 Hz the coherence time is on the order of 10 s, so a multi-second
    /// signal is needed for the windows to span more than one fading dwell.
    #[test]
    fn f1_envelope_has_non_trivial_variation() {
        let cfg = WattersonConfig::good_f1(Some(7));
        let mut ch = WattersonChannel::new(cfg).unwrap();

        let n = 80_000usize; // 10 s @ 8 kHz — multiple coherence times of F1
        let signal: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * 1500.0 * i as f32 / 8000.0).sin())
            .collect();
        let out = ch.apply(&signal);

        let window = 4000usize; // 0.5 s windows
        let n_windows = n / window;
        let window_rms: Vec<f32> = (0..n_windows)
            .map(|w| {
                let start = w * window;
                let end = start + window;
                (out[start..end].iter().map(|&s| s * s).sum::<f32>() / window as f32).sqrt()
            })
            .collect();
        let mean = window_rms.iter().sum::<f32>() / n_windows as f32;
        let variance =
            window_rms.iter().map(|&r| (r - mean).powi(2)).sum::<f32>() / n_windows as f32;
        let cv = variance.sqrt() / mean;

        assert!(
            cv > 0.10,
            "F1 envelope CV {:.4} should exceed 0.10; the FFT auto-sizing in make_envelope \
             must keep σ_bins clear of the 0.5 floor",
            cv
        );
    }
}
