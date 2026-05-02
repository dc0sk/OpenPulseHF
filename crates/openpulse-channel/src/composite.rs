//! Composite channel: series pipeline through multiple channel models.

use crate::{build_channel, ChannelError, ChannelModel, CompositeConfig};

/// Applies a sequence of channel models in order.
pub struct CompositeChannel {
    stages: Vec<Box<dyn ChannelModel>>,
}

impl CompositeChannel {
    pub fn build(config: &CompositeConfig, seed: Option<u64>) -> Result<Self, ChannelError> {
        let stages = config
            .stages
            .iter()
            .enumerate()
            .map(|(i, cfg)| {
                // Derive a distinct per-stage seed from the override so correlated RNG
                // streams don't align across stages.
                let stage_seed = seed.map(|s| s ^ (i as u64).wrapping_mul(0x9e3779b97f4a7c15));
                build_channel(cfg, stage_seed)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { stages })
    }
}

impl ChannelModel for CompositeChannel {
    fn apply(&mut self, input: &[f32]) -> Vec<f32> {
        self.stages
            .iter_mut()
            .fold(input.to_vec(), |acc, stage| stage.apply(&acc))
    }

    fn generate_noise(&mut self, length: usize) -> Vec<f32> {
        // Sum independent additive noise from each stage (multiplicative-only
        // stages, e.g. QSB/Watterson, return zeros from their generate_noise).
        let mut out = vec![0.0f32; length];
        for stage in &mut self.stages {
            let noise = stage.generate_noise(length);
            for (o, n) in out.iter_mut().zip(noise.iter()) {
                *o += n;
            }
        }
        out
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AwgnConfig, ChannelModelConfig, CompositeConfig};

    #[test]
    fn two_stage_awgn_pipeline() {
        let cfg = CompositeConfig {
            stages: vec![
                ChannelModelConfig::Awgn(AwgnConfig {
                    snr_db: 20.0,
                    seed: Some(1),
                }),
                ChannelModelConfig::Awgn(AwgnConfig {
                    snr_db: 20.0,
                    seed: Some(2),
                }),
            ],
        };
        let mut ch = CompositeChannel::build(&cfg, None).unwrap();
        let input = vec![1.0f32; 128];
        let out = ch.apply(&input);
        assert_eq!(out.len(), 128);
    }

    #[test]
    fn empty_composite_is_passthrough() {
        let mut ch = CompositeChannel::build(&CompositeConfig { stages: vec![] }, None).unwrap();
        let input = vec![0.5f32; 16];
        assert_eq!(ch.apply(&input), input);
    }

    #[test]
    fn generate_noise_sums_stages() {
        let cfg = CompositeConfig {
            stages: vec![
                ChannelModelConfig::Awgn(AwgnConfig {
                    snr_db: 0.0,
                    seed: Some(1),
                }),
                ChannelModelConfig::Awgn(AwgnConfig {
                    snr_db: 0.0,
                    seed: Some(2),
                }),
            ],
        };
        let mut ch = CompositeChannel::build(&cfg, None).unwrap();
        let noise = ch.generate_noise(256);
        // Two AWGN stages both generate noise, so the output should be non-zero.
        assert!(noise.iter().any(|&s| s != 0.0));
    }

    #[test]
    fn distinct_stage_seeds_from_override() {
        // With a global seed override both stages should produce different outputs.
        let cfg = CompositeConfig {
            stages: vec![
                ChannelModelConfig::Awgn(AwgnConfig {
                    snr_db: 20.0,
                    seed: None,
                }),
                ChannelModelConfig::Awgn(AwgnConfig {
                    snr_db: 20.0,
                    seed: None,
                }),
            ],
        };
        let mut ch = CompositeChannel::build(&cfg, Some(99)).unwrap();
        let n1 = ch.stages[0].generate_noise(32);
        let n2 = ch.stages[1].generate_noise(32);
        assert_ne!(n1, n2, "per-stage seeds should produce distinct noise");
    }
}
