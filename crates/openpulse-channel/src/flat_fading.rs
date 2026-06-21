//! Frequency-flat Rayleigh fading channel with realistic carrier-phase rotation.
//!
//! A single Doppler-shaped complex Rayleigh ray applied to the analytic signal of the real
//! input: `out = Re{ analytic(s) · h(t) }` plus AWGN. Unlike [`crate::qsb`] (amplitude-only),
//! the gain rotates the carrier phase by `arg(h)`, so it stresses carrier recovery the way
//! real fading does. Unlike [`crate::watterson`], it has no second delayed ray, so it is
//! frequency-flat (no multipath ISI) — the canonical flat-fading channel.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rand_distr::StandardNormal;
use rustfft::FftPlanner;

use crate::{ChannelError, ChannelModel};

/// Configuration for [`FlatFadingChannel`].
#[derive(Debug, Clone, PartialEq)]
pub struct FlatFadingConfig {
    /// Doppler spread in Hz (fading rate); larger = faster fading.
    pub doppler_spread_hz: f32,
    /// Average SNR in dB of the additive noise relative to the faded signal.
    pub snr_db: f32,
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// RNG seed for reproducible realizations (`None` = thread entropy).
    pub seed: Option<u64>,
}

impl Default for FlatFadingConfig {
    fn default() -> Self {
        Self {
            doppler_spread_hz: 0.5,
            snr_db: 20.0,
            sample_rate: 8000,
            seed: None,
        }
    }
}

impl FlatFadingConfig {
    /// Slow flat fade (0.2 Hz Doppler) — quiet HF / near-vertical incidence.
    pub fn slow(snr_db: f32, seed: Option<u64>) -> Self {
        Self {
            doppler_spread_hz: 0.2,
            snr_db,
            sample_rate: 8000,
            seed,
        }
    }

    /// Moderate flat fade (1.0 Hz Doppler) — typical mid-latitude HF.
    pub fn moderate(snr_db: f32, seed: Option<u64>) -> Self {
        Self {
            doppler_spread_hz: 1.0,
            snr_db,
            sample_rate: 8000,
            seed,
        }
    }

    /// Fast flat fade (5.0 Hz Doppler) — disturbed / high-latitude paths.
    pub fn fast(snr_db: f32, seed: Option<u64>) -> Self {
        Self {
            doppler_spread_hz: 5.0,
            snr_db,
            sample_rate: 8000,
            seed,
        }
    }
}

/// Frequency-flat Rayleigh fading channel with carrier-phase rotation.
pub struct FlatFadingChannel {
    config: FlatFadingConfig,
    rng: StdRng,
    planner: FftPlanner<f32>,
}

impl FlatFadingChannel {
    /// Construct the channel, validating the configuration.
    pub fn new(config: FlatFadingConfig) -> Result<Self, ChannelError> {
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
        if config.sample_rate == 0 {
            return Err(ChannelError::InvalidParameter(
                "sample_rate must be > 0".into(),
            ));
        }
        let rng = match config.seed {
            Some(s) => StdRng::seed_from_u64(s),
            None => StdRng::from_entropy(),
        };
        Ok(Self {
            config,
            rng,
            planner: FftPlanner::new(),
        })
    }
}

impl ChannelModel for FlatFadingChannel {
    fn apply(&mut self, input: &[f32]) -> Vec<f32> {
        let n = input.len();
        if n == 0 {
            return Vec::new();
        }

        let env = crate::fading::doppler_envelope(
            &mut self.rng,
            &mut self.planner,
            n,
            self.config.doppler_spread_hz,
            self.config.sample_rate,
        );
        let analytic = crate::fading::analytic_signal(&mut self.planner, input);

        let rms = (input.iter().map(|&s| s * s).sum::<f32>() / n as f32).sqrt();
        let noise_sigma = if rms > 0.0 {
            rms / 10f32.powf(self.config.snr_db / 20.0)
        } else {
            1e-4
        };

        let mut out = vec![0.0f32; n];
        for i in 0..n {
            out[i] =
                (analytic[i] * env[i]).re + noise_sigma * self.rng.sample::<f32, _>(StandardNormal);
        }
        out
    }

    fn generate_noise(&mut self, length: usize) -> Vec<f32> {
        // Multiplicative fading is not independent additive noise.
        vec![0.0; length]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Flat fading must produce non-trivial amplitude variation across a call (CV > 10%).
    #[test]
    fn non_trivial_fading_envelope() {
        let cfg = FlatFadingConfig {
            doppler_spread_hz: 2.0,
            snr_db: 60.0,
            sample_rate: 8000,
            seed: Some(7),
        };
        let mut ch = FlatFadingChannel::new(cfg).unwrap();
        let n = 8000usize;
        let signal: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * 1500.0 * i as f32 / 8000.0).sin())
            .collect();
        let out = ch.apply(&signal);

        let window = 500usize;
        let rms: Vec<f32> = (0..n / window)
            .map(|w| {
                let s = &out[w * window..(w + 1) * window];
                (s.iter().map(|&x| x * x).sum::<f32>() / window as f32).sqrt()
            })
            .collect();
        let mean = rms.iter().sum::<f32>() / rms.len() as f32;
        let cv =
            (rms.iter().map(|&r| (r - mean).powi(2)).sum::<f32>() / rms.len() as f32).sqrt() / mean;
        assert!(
            cv > 0.10,
            "flat-fade envelope CV {cv:.3} should exceed 0.10"
        );
    }

    /// Phase-realism guard: like the Watterson fix, flat fading scales by the Rayleigh
    /// magnitude (rarely near zero) — NOT by Re{h} (which collapses ~16% of the time at 90°).
    #[test]
    fn does_not_phase_annihilate() {
        let n = 1000usize;
        let tone: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * 1500.0 * i as f32 / 8000.0).sin())
            .collect();
        let in_rms = (tone.iter().map(|s| s * s).sum::<f32>() / n as f32).sqrt();

        let mut deep = 0;
        let seeds = 40u64;
        for seed in 0..seeds {
            let cfg = FlatFadingConfig {
                doppler_spread_hz: 0.5,
                snr_db: 60.0,
                sample_rate: 8000,
                seed: Some(seed),
            };
            let mut ch = FlatFadingChannel::new(cfg).unwrap();
            let out = ch.apply(&tone);
            let out_rms = (out.iter().map(|s| s * s).sum::<f32>() / n as f32).sqrt();
            if out_rms / in_rms < 0.2 {
                deep += 1;
            }
        }
        assert!(
            deep <= seeds as usize / 10,
            "{deep}/{seeds} flat-fade realizations collapsed below 0.2× — phase annihilation"
        );
    }

    /// Flat fading is frequency-flat: a single tone stays a single tone (no multipath ISI
    /// creating spectral nulls / new spectral content beyond the fading sidebands).
    #[test]
    fn preserves_tone_purity() {
        let cfg = FlatFadingConfig {
            doppler_spread_hz: 0.5,
            snr_db: 60.0,
            sample_rate: 8000,
            seed: Some(3),
        };
        let mut ch = FlatFadingChannel::new(cfg).unwrap();
        let n = 8000usize;
        let f0 = 1500.0f32;
        let tone: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * f0 * i as f32 / 8000.0).sin())
            .collect();
        let out = ch.apply(&tone);

        // Energy should concentrate near f0 (within the narrow Doppler sidebands), not spread
        // across the band as frequency-selective multipath would.
        let goertzel = |sig: &[f32], freq: f32| -> f32 {
            let w = 2.0 * std::f32::consts::PI * freq / 8000.0;
            let coeff = 2.0 * w.cos();
            let (mut s1, mut s2) = (0.0f32, 0.0f32);
            for &x in sig {
                let s = x + coeff * s1 - s2;
                s2 = s1;
                s1 = s;
            }
            s1 * s1 + s2 * s2 - coeff * s1 * s2
        };
        let at_tone = goertzel(&out, f0);
        let off_tone = goertzel(&out, f0 + 1000.0);
        assert!(
            at_tone > 50.0 * off_tone,
            "flat-faded tone energy should stay near f0 (at={at_tone:.1}, off={off_tone:.1})"
        );
    }

    #[test]
    fn rejects_negative_doppler() {
        let cfg = FlatFadingConfig {
            doppler_spread_hz: -1.0,
            ..FlatFadingConfig::default()
        };
        assert!(FlatFadingChannel::new(cfg).is_err());
    }

    #[test]
    fn preserves_length() {
        let mut ch = FlatFadingChannel::new(FlatFadingConfig::moderate(20.0, Some(1))).unwrap();
        assert_eq!(ch.apply(&[0.1; 256]).len(), 256);
        assert_eq!(ch.apply(&[]).len(), 0);
    }
}
