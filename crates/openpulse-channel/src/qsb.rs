//! QSB (fading) — multiplicative sinusoidal amplitude envelope.

use crate::{ChannelError, ChannelModel, QsbConfig};

/// Sinusoidal amplitude-fading channel.
pub struct QsbChannel {
    config: QsbConfig,
    /// Sample counter for continuous envelope phase.
    sample_idx: usize,
}

impl QsbChannel {
    pub fn new(config: QsbConfig) -> Result<Self, ChannelError> {
        if config.sample_rate == 0 {
            return Err(ChannelError::InvalidParameter("sample_rate must be > 0".into()));
        }
        if !config.fade_rate_hz.is_finite() || config.fade_rate_hz < 0.0 {
            return Err(ChannelError::InvalidParameter(
                "fade_rate_hz must be a non-negative finite value".into(),
            ));
        }
        if !(0.0..=1.0).contains(&config.fade_depth) {
            return Err(ChannelError::InvalidParameter(
                "fade_depth must be in [0, 1]".into(),
            ));
        }
        Ok(Self { config, sample_idx: 0 })
    }
}

impl ChannelModel for QsbChannel {
    fn apply(&mut self, input: &[f32]) -> Vec<f32> {
        let sr = self.config.sample_rate as f32;
        let phase_step = 2.0 * std::f32::consts::PI * self.config.fade_rate_hz / sr;
        let depth = self.config.fade_depth;
        // Envelope swings between `depth` and 1.0.
        let out = input
            .iter()
            .enumerate()
            .map(|(i, &s)| {
                let phase = phase_step * (self.sample_idx + i) as f32;
                // Maps sin ∈ [-1, 1] → envelope ∈ [depth, 1.0].
                let env = (1.0 + depth) / 2.0 + (1.0 - depth) / 2.0 * phase.sin();
                s * env
            })
            .collect();
        self.sample_idx += input.len();
        out
    }

    fn generate_noise(&mut self, length: usize) -> Vec<f32> {
        // QSB is multiplicative — no independent additive noise component.
        vec![0.0; length]
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::QsbConfig;

    #[test]
    fn fading_reduces_amplitude() {
        let mut ch = QsbChannel::new(QsbConfig {
            fade_rate_hz: 1.0,
            fade_depth: 0.0,
            sample_rate: 8000,
        })
        .unwrap();
        // At fade_depth=0 envelope ranges [0, 1]; near zero crossing the amplitude fades.
        let input = vec![1.0f32; 8000];
        let out = ch.apply(&input);
        let min = out.iter().cloned().fold(f32::INFINITY, f32::min);
        assert!(min < 0.1, "minimum amplitude {min} should approach 0 with full fade");
    }

    #[test]
    fn no_fade_passes_through() {
        let mut ch = QsbChannel::new(QsbConfig {
            fade_rate_hz: 1.0,
            fade_depth: 1.0,
            sample_rate: 8000,
        })
        .unwrap();
        let input = vec![1.0f32; 16];
        let out = ch.apply(&input);
        // With fade_depth=1 envelope is always 1.0; output equals input.
        for (a, b) in out.iter().zip(input.iter()) {
            assert!((a - b).abs() < 1e-5, "expected passthrough with fade_depth=1");
        }
    }

    #[test]
    fn rejects_infinite_fade_rate() {
        assert!(QsbChannel::new(QsbConfig {
            fade_rate_hz: f32::INFINITY,
            fade_depth: 0.5,
            sample_rate: 8000,
        })
        .is_err());
    }
}
