//! AWGN channel: additive white Gaussian noise at a fixed SNR.

use rand::SeedableRng;
use rand_distr::{Distribution, Normal};

use crate::{AwgnConfig, ChannelError, ChannelModel};

/// AWGN channel that scales noise to match the RMS of each input block.
pub struct AwgnChannel {
    config: AwgnConfig,
    rng: rand::rngs::StdRng,
}

impl AwgnChannel {
    pub fn new(config: AwgnConfig) -> Result<Self, ChannelError> {
        if !config.snr_db.is_finite() {
            return Err(ChannelError::InvalidParameter(
                "snr_db must be finite".into(),
            ));
        }
        let rng = match config.seed {
            Some(s) => rand::rngs::StdRng::seed_from_u64(s),
            None => rand::rngs::StdRng::from_entropy(),
        };
        Ok(Self { config, rng })
    }

    fn noise_sigma(&self, signal_rms: f32) -> f32 {
        // noise_sigma = signal_rms / 10^(snr_db / 20)
        signal_rms / 10f32.powf(self.config.snr_db / 20.0)
    }
}

impl ChannelModel for AwgnChannel {
    fn apply(&mut self, input: &[f32]) -> Vec<f32> {
        if input.is_empty() {
            return Vec::new();
        }
        let rms = (input.iter().map(|&s| s * s).sum::<f32>() / input.len() as f32).sqrt();
        let sigma = if rms > 0.0 {
            self.noise_sigma(rms)
        } else {
            1e-4
        };
        let dist = Normal::new(0.0f32, sigma).unwrap();
        input
            .iter()
            .map(|&s| s + dist.sample(&mut self.rng))
            .collect()
    }

    fn generate_noise(&mut self, length: usize) -> Vec<f32> {
        // Without a reference signal, use unit-RMS noise scaled by the SNR formula at RMS=1.
        let sigma = self.noise_sigma(1.0);
        let dist = Normal::new(0.0f32, sigma).unwrap();
        (0..length).map(|_| dist.sample(&mut self.rng)).collect()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_infinite_snr() {
        assert!(AwgnChannel::new(AwgnConfig {
            snr_db: f32::INFINITY,
            seed: None
        })
        .is_err());
    }

    /// At SNR = 0 dB the noise power should equal the signal power (within ±0.5 dB).
    #[test]
    fn snr_zero_db_equal_power() {
        let mut ch = AwgnChannel::new(AwgnConfig {
            snr_db: 0.0,
            seed: Some(1),
        })
        .unwrap();

        // Unit-amplitude 1500 Hz sine, 8000 Hz sample rate, 8000 samples.
        let n = 8000usize;
        let freq = 1500.0f32;
        let signal: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / 8000.0).sin())
            .collect();

        let signal_power = signal.iter().map(|&s| s * s).sum::<f32>() / n as f32;
        let noisy = ch.apply(&signal);
        // noise = noisy − signal
        let noise_power = noisy
            .iter()
            .zip(signal.iter())
            .map(|(&n, &s)| (n - s).powi(2))
            .sum::<f32>()
            / n as f32;

        let ratio_db = 10.0 * (noise_power / signal_power).log10();
        assert!(
            ratio_db.abs() < 0.5,
            "noise/signal ratio {ratio_db:.2} dB should be near 0 dB at SNR=0"
        );
    }

    #[test]
    fn deterministic_with_seed() {
        let cfg = AwgnConfig {
            snr_db: 10.0,
            seed: Some(99),
        };
        let input = vec![0.5f32; 128];
        let mut ch1 = AwgnChannel::new(cfg.clone()).unwrap();
        let mut ch2 = AwgnChannel::new(cfg).unwrap();
        assert_eq!(ch1.apply(&input), ch2.apply(&input));
    }
}
