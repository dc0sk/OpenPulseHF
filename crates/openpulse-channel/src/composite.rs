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
            .map(|cfg| build_channel(cfg, seed))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { stages })
    }
}

impl ChannelModel for CompositeChannel {
    fn apply(&mut self, input: &[f32]) -> Vec<f32> {
        self.stages.iter_mut().fold(input.to_vec(), |acc, stage| stage.apply(&acc))
    }

    fn generate_noise(&mut self, length: usize) -> Vec<f32> {
        self.stages.iter_mut().fold(vec![0.0f32; length], |acc, stage| stage.apply(&acc))
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
                ChannelModelConfig::Awgn(AwgnConfig { snr_db: 20.0, seed: Some(1) }),
                ChannelModelConfig::Awgn(AwgnConfig { snr_db: 20.0, seed: Some(2) }),
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
}
