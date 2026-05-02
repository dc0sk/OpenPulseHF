//! Chirp interference — linear frequency sweep with persistent phase.

use crate::{ChannelError, ChannelModel, ChirpConfig};

/// Linear-chirp interference source.
pub struct ChirpChannel {
    config: ChirpConfig,
    /// Persistent phase accumulator (radians).
    phase: f32,
}

impl ChirpChannel {
    pub fn new(config: ChirpConfig) -> Result<Self, ChannelError> {
        if config.sample_rate == 0 {
            return Err(ChannelError::InvalidParameter("sample_rate must be > 0".into()));
        }
        if config.period_s <= 0.0 {
            return Err(ChannelError::InvalidParameter("period_s must be > 0".into()));
        }
        Ok(Self { config, phase: 0.0 })
    }
}

impl ChannelModel for ChirpChannel {
    fn apply(&mut self, input: &[f32]) -> Vec<f32> {
        let sr = self.config.sample_rate as f32;
        let period_samples = self.config.period_s * sr;
        let f_start = self.config.f_start_hz;
        let f_end = self.config.f_end_hz;
        let amp = self.config.amplitude;

        let rms = if input.is_empty() {
            1.0
        } else {
            (input.iter().map(|&s| s * s).sum::<f32>() / input.len() as f32).sqrt().max(1e-4)
        };

        input
            .iter()
            .map(|&s| {
                // Instantaneous frequency: linear interpolation within sweep period.
                let t_in_period = (self.phase / (2.0 * std::f32::consts::PI * f_start))
                    .rem_euclid(period_samples)
                    / period_samples;
                let f_inst = f_start + (f_end - f_start) * t_in_period;
                let phase_step = 2.0 * std::f32::consts::PI * f_inst / sr;
                self.phase = (self.phase + phase_step) % (2.0 * std::f32::consts::PI);
                s + amp * rms * self.phase.sin()
            })
            .collect()
    }

    fn generate_noise(&mut self, length: usize) -> Vec<f32> {
        vec![0.0; length]
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ChirpConfig;

    #[test]
    fn output_length_preserved() {
        let mut ch = ChirpChannel::new(ChirpConfig {
            f_start_hz: 500.0,
            f_end_hz: 2000.0,
            period_s: 1.0,
            amplitude: 0.1,
            sample_rate: 8000,
        })
        .unwrap();
        let input = vec![0.0f32; 256];
        assert_eq!(ch.apply(&input).len(), 256);
    }

    #[test]
    fn rejects_zero_period() {
        assert!(ChirpChannel::new(ChirpConfig {
            f_start_hz: 100.0,
            f_end_hz: 2000.0,
            period_s: 0.0,
            amplitude: 1.0,
            sample_rate: 8000,
        })
        .is_err());
    }
}
