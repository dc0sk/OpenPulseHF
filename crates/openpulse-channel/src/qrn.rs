//! QRN (atmospheric noise) — Middleton Class-A impulsive noise model.

use rand::SeedableRng;
use rand_distr::{Distribution, Normal, Poisson, Uniform};

use crate::{ChannelError, ChannelModel, QrnConfig};

/// Middleton Class-A atmospheric noise (background Gaussian + impulsive spikes).
pub struct QrnChannel {
    config: QrnConfig,
    rng: rand::rngs::StdRng,
}

impl QrnChannel {
    pub fn new(config: QrnConfig) -> Result<Self, ChannelError> {
        if !config.gaussian_snr_db.is_finite() {
            return Err(ChannelError::InvalidParameter(
                "gaussian_snr_db must be finite".into(),
            ));
        }
        if !config.impulse_rate_hz.is_finite() || config.impulse_rate_hz < 0.0 {
            return Err(ChannelError::InvalidParameter(
                "impulse_rate_hz must be a non-negative finite value".into(),
            ));
        }
        if !config.impulse_amplitude_ratio.is_finite() || config.impulse_amplitude_ratio < 0.0 {
            return Err(ChannelError::InvalidParameter(
                "impulse_amplitude_ratio must be a non-negative finite value".into(),
            ));
        }
        if config.sample_rate == 0 {
            return Err(ChannelError::InvalidParameter("sample_rate must be > 0".into()));
        }
        let rng = match config.seed {
            Some(s) => rand::rngs::StdRng::seed_from_u64(s),
            None => rand::rngs::StdRng::from_entropy(),
        };
        Ok(Self { config, rng })
    }
}

impl ChannelModel for QrnChannel {
    fn apply(&mut self, input: &[f32]) -> Vec<f32> {
        if input.is_empty() {
            return Vec::new();
        }
        let n = input.len();
        let rms = (input.iter().map(|&s| s * s).sum::<f32>() / n as f32).sqrt();
        let rms = if rms > 0.0 { rms } else { 1e-4 };

        let bg_sigma = rms / 10f32.powf(self.config.gaussian_snr_db / 20.0);
        let bg_dist = Normal::new(0.0f32, bg_sigma).unwrap();

        let mut out: Vec<f32> = input
            .iter()
            .map(|&s| s + bg_dist.sample(&mut self.rng))
            .collect();

        let expected_spikes =
            self.config.impulse_rate_hz * n as f32 / self.config.sample_rate as f32;
        if expected_spikes > 0.0 {
            let n_spikes =
                Poisson::new(expected_spikes as f64).unwrap().sample(&mut self.rng) as usize;
            let pos_dist = Uniform::new(0usize, n);
            let spike_sigma = rms * self.config.impulse_amplitude_ratio;
            let spike_dist = Normal::new(0.0f32, spike_sigma).unwrap();
            let dur = self.config.max_spike_duration_samples.max(1) as usize;

            for _ in 0..n_spikes {
                let start = pos_dist.sample(&mut self.rng);
                let amp = spike_dist.sample(&mut self.rng);
                for sample in out[start..(start + dur).min(n)].iter_mut() {
                    *sample += amp;
                }
            }
        }
        out
    }

    fn generate_noise(&mut self, length: usize) -> Vec<f32> {
        let sigma = 1.0 / 10f32.powf(self.config.gaussian_snr_db / 20.0);
        let bg_dist = Normal::new(0.0f32, sigma).unwrap();
        (0..length).map(|_| bg_dist.sample(&mut self.rng)).collect()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::QrnConfig;

    #[test]
    fn apply_returns_same_length() {
        let mut ch = QrnChannel::new(QrnConfig {
            gaussian_snr_db: 10.0,
            impulse_rate_hz: 100.0,
            impulse_amplitude_ratio: 10.0,
            max_spike_duration_samples: 3,
            sample_rate: 8000,
            seed: Some(1),
        })
        .unwrap();
        let input = vec![0.5f32; 512];
        assert_eq!(ch.apply(&input).len(), 512);
    }

    #[test]
    fn rejects_infinite_impulse_rate() {
        assert!(QrnChannel::new(QrnConfig {
            gaussian_snr_db: 10.0,
            impulse_rate_hz: f32::INFINITY,
            impulse_amplitude_ratio: 1.0,
            max_spike_duration_samples: 1,
            sample_rate: 8000,
            seed: None,
        })
        .is_err());
    }

    #[test]
    fn rejects_negative_amplitude_ratio() {
        assert!(QrnChannel::new(QrnConfig {
            gaussian_snr_db: 10.0,
            impulse_rate_hz: 10.0,
            impulse_amplitude_ratio: -1.0,
            max_spike_duration_samples: 1,
            sample_rate: 8000,
            seed: None,
        })
        .is_err());
    }
}
