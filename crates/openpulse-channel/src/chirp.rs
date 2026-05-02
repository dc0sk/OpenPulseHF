//! Chirp interference — linear frequency sweep with persistent phase.

use crate::{ChannelError, ChannelModel, ChirpConfig};

/// Linear-chirp interference source.
pub struct ChirpChannel {
    config: ChirpConfig,
    /// Persistent phase accumulator (radians) for sin output.
    phase: f32,
    /// Total samples emitted; drives sweep position independently of wrapped phase.
    sample_count: u64,
}

impl ChirpChannel {
    pub fn new(config: ChirpConfig) -> Result<Self, ChannelError> {
        if config.sample_rate == 0 {
            return Err(ChannelError::InvalidParameter(
                "sample_rate must be > 0".into(),
            ));
        }
        if config.period_s <= 0.0 || !config.period_s.is_finite() {
            return Err(ChannelError::InvalidParameter(
                "period_s must be a positive finite value".into(),
            ));
        }
        if !config.f_start_hz.is_finite() || config.f_start_hz <= 0.0 {
            return Err(ChannelError::InvalidParameter(
                "f_start_hz must be a positive finite value".into(),
            ));
        }
        if !config.f_end_hz.is_finite() || config.f_end_hz <= 0.0 {
            return Err(ChannelError::InvalidParameter(
                "f_end_hz must be a positive finite value".into(),
            ));
        }
        Ok(Self {
            config,
            phase: 0.0,
            sample_count: 0,
        })
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
            (input.iter().map(|&s| s * s).sum::<f32>() / input.len() as f32)
                .sqrt()
                .max(1e-4)
        };

        input
            .iter()
            .map(|&s| {
                // Sweep position within [0, 1) derived from an explicit sample counter,
                // not from the wrapped phase (which would give near-zero t_in_period).
                let t_norm = (self.sample_count as f32 % period_samples) / period_samples;
                let f_inst = f_start + (f_end - f_start) * t_norm;
                let phase_step = 2.0 * std::f32::consts::PI * f_inst / sr;
                self.phase += phase_step;
                // Wrap periodically to prevent float precision loss.
                if self.phase > 1000.0 * std::f32::consts::PI {
                    self.phase -= 1000.0 * std::f32::consts::PI;
                }
                self.sample_count += 1;
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

    #[test]
    fn rejects_non_positive_start_freq() {
        assert!(ChirpChannel::new(ChirpConfig {
            f_start_hz: 0.0,
            f_end_hz: 2000.0,
            period_s: 1.0,
            amplitude: 1.0,
            sample_rate: 8000,
        })
        .is_err());
    }

    /// Verify the sweep actually changes instantaneous frequency over time.
    #[test]
    fn sweep_changes_frequency_over_period() {
        let sr = 8000u32;
        let mut ch = ChirpChannel::new(ChirpConfig {
            f_start_hz: 500.0,
            f_end_hz: 2000.0,
            period_s: 1.0,
            amplitude: 1.0,
            sample_rate: sr,
        })
        .unwrap();
        // Two 256-sample blocks far apart should produce different phase increments.
        let block = vec![0.0f32; 256];
        let first = ch.apply(&block);
        // Skip to near mid-period.
        let skip = vec![0.0f32; sr as usize / 2 - 256];
        ch.apply(&skip);
        let mid = ch.apply(&block);
        // The two outputs should differ (different instantaneous frequency).
        assert_ne!(first, mid, "sweep should change output over the period");
    }
}
