//! QRM (man-made interference) — phase-coherent discrete tones.

use rand::SeedableRng;
use rand_distr::{Distribution, Normal};

use crate::{ChannelError, ChannelModel, QrmConfig};

/// Phase-coherent CW tone interference with optional background noise.
pub struct QrmChannel {
    config: QrmConfig,
    /// Persistent phase accumulator per tone (radians), preserves phase across blocks.
    phases: Vec<f32>,
    rng: rand::rngs::StdRng,
}

impl QrmChannel {
    pub fn new(config: QrmConfig) -> Result<Self, ChannelError> {
        if config.sample_rate == 0 {
            return Err(ChannelError::InvalidParameter("sample_rate must be > 0".into()));
        }
        let phases = vec![0.0f32; config.tones.len()];
        let rng = match config.seed {
            Some(s) => rand::rngs::StdRng::seed_from_u64(s),
            None => rand::rngs::StdRng::from_entropy(),
        };
        Ok(Self { config, phases, rng })
    }
}

impl ChannelModel for QrmChannel {
    fn apply(&mut self, input: &[f32]) -> Vec<f32> {
        if input.is_empty() {
            return Vec::new();
        }
        let n = input.len();
        let sr = self.config.sample_rate as f32;
        let rms = (input.iter().map(|&s| s * s).sum::<f32>() / n as f32).sqrt();
        let rms = if rms > 0.0 { rms } else { 1e-4 };

        let bg_sigma = self.config.noise_floor_snr_db.map(|snr| {
            rms / 10f32.powf(snr / 20.0)
        });

        let mut out = input.to_vec();
        for sample in out.iter_mut() {
            for (t, tone) in self.config.tones.iter().enumerate() {
                let phase_step = 2.0 * std::f32::consts::PI * tone.frequency_hz / sr;
                self.phases[t] += phase_step;
                *sample += tone.amplitude * rms * self.phases[t].sin();
            }
            if let Some(sigma) = bg_sigma {
                *sample += Normal::new(0.0f32, sigma).unwrap().sample(&mut self.rng);
            }
        }
        // Wrap phases to avoid float drift.
        for p in &mut self.phases {
            *p %= 2.0 * std::f32::consts::PI;
        }
        out
    }

    fn generate_noise(&mut self, length: usize) -> Vec<f32> {
        // Tones are not additive noise in isolation; return zeros.
        vec![0.0; length]
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{QrmConfig, ToneConfig};

    #[test]
    fn output_length_preserved() {
        let mut ch = QrmChannel::new(QrmConfig {
            tones: vec![ToneConfig { frequency_hz: 1000.0, amplitude: 0.5 }],
            noise_floor_snr_db: None,
            sample_rate: 8000,
            seed: None,
        })
        .unwrap();
        let input = vec![1.0f32; 256];
        assert_eq!(ch.apply(&input).len(), 256);
    }

    #[test]
    fn rejects_zero_sample_rate() {
        let res = QrmChannel::new(QrmConfig {
            tones: vec![],
            noise_floor_snr_db: None,
            sample_rate: 0,
            seed: None,
        });
        assert!(res.is_err());
    }
}
