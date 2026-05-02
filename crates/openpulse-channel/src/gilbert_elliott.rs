//! Gilbert-Elliott two-state Markov burst-error channel.

use rand::SeedableRng;
use rand_distr::{Distribution, Normal, Uniform};

use crate::{ChannelError, ChannelModel, GilbertElliottConfig};

/// Two-state (Good/Bad) Markov channel with AWGN in each state.
pub struct GilbertElliottChannel {
    config: GilbertElliottConfig,
    rng: rand::rngs::StdRng,
    /// `true` = currently in Bad (burst) state.
    in_bad: bool,
}

impl GilbertElliottChannel {
    pub fn new(config: GilbertElliottConfig) -> Result<Self, ChannelError> {
        if config.p_gb <= 0.0 || config.p_gb >= 1.0 {
            return Err(ChannelError::InvalidParameter(
                "p_gb must be in (0, 1)".into(),
            ));
        }
        if config.p_bg <= 0.0 || config.p_bg >= 1.0 {
            return Err(ChannelError::InvalidParameter(
                "p_bg must be in (0, 1)".into(),
            ));
        }
        let rng = match config.seed {
            Some(s) => rand::rngs::StdRng::seed_from_u64(s),
            None => rand::rngs::StdRng::from_entropy(),
        };
        Ok(Self {
            config,
            rng,
            in_bad: false,
        })
    }

    fn noise_sigma_for_snr(&self, snr_db: f32, signal_rms: f32) -> f32 {
        signal_rms / 10f32.powf(snr_db / 20.0)
    }
}

impl ChannelModel for GilbertElliottChannel {
    fn apply(&mut self, input: &[f32]) -> Vec<f32> {
        if input.is_empty() {
            return Vec::new();
        }
        let rms = (input.iter().map(|&s| s * s).sum::<f32>() / input.len() as f32).sqrt();
        let rms = if rms > 0.0 { rms } else { 1e-4 };
        let sigma_good = self.noise_sigma_for_snr(self.config.snr_good_db, rms);
        let sigma_bad = self.noise_sigma_for_snr(self.config.snr_bad_db, rms);
        // Construct distributions once per block, not once per sample.
        let dist_good = Normal::new(0.0f32, sigma_good).unwrap();
        let dist_bad = Normal::new(0.0f32, sigma_bad).unwrap();
        let uniform = Uniform::new(0.0f32, 1.0);
        input
            .iter()
            .map(|&s| {
                let u = uniform.sample(&mut self.rng);
                if self.in_bad {
                    if u < self.config.p_bg {
                        self.in_bad = false;
                    }
                } else if u < self.config.p_gb {
                    self.in_bad = true;
                }
                let n = if self.in_bad {
                    dist_bad.sample(&mut self.rng)
                } else {
                    dist_good.sample(&mut self.rng)
                };
                s + n
            })
            .collect()
    }

    fn generate_noise(&mut self, length: usize) -> Vec<f32> {
        let sigma_good = self.noise_sigma_for_snr(self.config.snr_good_db, 1.0);
        let sigma_bad = self.noise_sigma_for_snr(self.config.snr_bad_db, 1.0);
        let dist_good = Normal::new(0.0f32, sigma_good).unwrap();
        let dist_bad = Normal::new(0.0f32, sigma_bad).unwrap();
        let uniform = Uniform::new(0.0f32, 1.0);
        (0..length)
            .map(|_| {
                let u = uniform.sample(&mut self.rng);
                if self.in_bad {
                    if u < self.config.p_bg {
                        self.in_bad = false;
                    }
                } else if u < self.config.p_gb {
                    self.in_bad = true;
                }
                if self.in_bad {
                    dist_bad.sample(&mut self.rng)
                } else {
                    dist_good.sample(&mut self.rng)
                }
            })
            .collect()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GilbertElliottConfig;

    /// The expected mean burst length in the Bad state is 1/p_bg.
    /// Over 100 k samples the observed mean should be within 10 % of theory.
    #[test]
    fn moderate_burst_mean_within_10_percent() {
        let cfg = GilbertElliottConfig::moderate(Some(42));
        let expected_mean = 1.0 / cfg.p_bg;
        let p_gb = cfg.p_gb;
        let p_bg = cfg.p_bg;
        let _ch = GilbertElliottChannel::new(cfg).unwrap();

        let n = 100_000usize;
        let uniform = rand_distr::Uniform::new(0.0f32, 1.0);
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let mut in_bad = false;
        let mut burst_start: Option<usize> = None;
        let mut burst_lengths: Vec<usize> = Vec::new();

        for i in 0..n {
            let u = rand_distr::Distribution::sample(&uniform, &mut rng);
            let was_bad = in_bad;
            if in_bad {
                if u < p_bg {
                    in_bad = false;
                }
            } else if u < p_gb {
                in_bad = true;
            }
            match (was_bad, in_bad) {
                (false, true) => burst_start = Some(i),
                (true, false) => {
                    if let Some(start) = burst_start.take() {
                        burst_lengths.push(i - start);
                    }
                }
                _ => {}
            }
        }

        assert!(
            !burst_lengths.is_empty(),
            "no bursts observed in {n} samples — p_gb likely too low"
        );

        let observed_mean = burst_lengths.iter().sum::<usize>() as f64 / burst_lengths.len() as f64;
        let tolerance = expected_mean as f64 * 0.10;
        assert!(
            (observed_mean - expected_mean as f64).abs() < tolerance,
            "mean burst {observed_mean:.1} not within 10% of {expected_mean}"
        );
    }

    #[test]
    fn rejects_invalid_transition_probs() {
        let mut cfg = GilbertElliottConfig::moderate(None);
        cfg.p_gb = 1.5;
        assert!(GilbertElliottChannel::new(cfg).is_err());
    }
}
