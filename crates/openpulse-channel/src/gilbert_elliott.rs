//! Gilbert-Elliott two-state Markov burst-error channel.

use rand::Rng;
use rand::SeedableRng;
use rand_distr::StandardNormal;

use crate::{ChannelError, ChannelModel, GilbertElliottConfig};

/// Two-state (Good/Bad) Markov channel with AWGN in each state.
pub struct GilbertElliottChannel {
    config: GilbertElliottConfig,
    rng: rand::rngs::StdRng,
    /// `true` = currently in Bad (burst) state.
    in_bad: bool,
}

impl GilbertElliottChannel {
    /// Step the Markov chain **once at the start of each symbol**, holding the state through the symbol.
    /// `i` is the absolute sample index; a transition fires only when `i` lands on a symbol boundary, so
    /// a Bad run covers whole contiguous symbols (a real burst) instead of flickering every sample.
    #[inline]
    fn step_state(&mut self, i: usize) {
        if !i.is_multiple_of(self.config.symbol_samples.max(1)) {
            return;
        }
        let u: f32 = self.rng.gen();
        if self.in_bad {
            if u < self.config.p_bg {
                self.in_bad = false;
            }
        } else if u < self.config.p_gb {
            self.in_bad = true;
        }
    }

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
        if !config.snr_good_db.is_finite() {
            return Err(ChannelError::InvalidParameter(
                "snr_good_db must be finite".into(),
            ));
        }
        if !config.snr_bad_db.is_finite() {
            return Err(ChannelError::InvalidParameter(
                "snr_bad_db must be finite".into(),
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
        input
            .iter()
            .enumerate()
            .map(|(i, &s)| {
                self.step_state(i);
                let sigma = if self.in_bad { sigma_bad } else { sigma_good };
                s + sigma * self.rng.sample::<f32, _>(StandardNormal)
            })
            .collect()
    }

    fn generate_noise(&mut self, length: usize) -> Vec<f32> {
        let sigma_good = self.noise_sigma_for_snr(self.config.snr_good_db, 1.0);
        let sigma_bad = self.noise_sigma_for_snr(self.config.snr_bad_db, 1.0);
        (0..length)
            .map(|i| {
                self.step_state(i);
                let sigma = if self.in_bad { sigma_bad } else { sigma_good };
                sigma * self.rng.sample::<f32, _>(StandardNormal)
            })
            .collect()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GilbertElliottConfig;

    /// Drive the *actual* channel and confirm its Bad runs are bursts of whole **symbols** with mean
    /// length 1/p_bg symbols — the property that makes it a valid burst-error channel. Recovers the
    /// per-symbol state from the output noise energy (Bad is ~20 dB louder than Good on `moderate`).
    #[test]
    fn bursts_span_whole_symbols_with_mean_one_over_pbg() {
        let sps = 16usize;
        let mut cfg = GilbertElliottConfig::moderate(Some(42));
        cfg.symbol_samples = sps;
        let expected_mean = 1.0 / cfg.p_bg as f64; // symbols
        let n_syms = 40_000usize;

        let mut ch = GilbertElliottChannel::new(cfg.clone()).unwrap();
        let noise = ch.generate_noise(n_syms * sps); // pure noise, state held per symbol

        // Good σ² ≈ 10^(-20/10) = 0.01, Bad σ² ≈ 10^(0/10) = 1.0 (rms = 1.0); split at 0.1.
        let bad: Vec<bool> = (0..n_syms)
            .map(|k| {
                let e = noise[k * sps..(k + 1) * sps]
                    .iter()
                    .map(|v| v * v)
                    .sum::<f32>()
                    / sps as f32;
                e > 0.1
            })
            .collect();

        let mut runs: Vec<usize> = Vec::new();
        let mut start: Option<usize> = None;
        for (k, &b) in bad.iter().enumerate() {
            match (b, start) {
                (true, None) => start = Some(k),
                (false, Some(s)) => {
                    runs.push(k - s);
                    start = None;
                }
                _ => {}
            }
        }
        assert!(!runs.is_empty(), "no bursts observed");
        // A genuine burst spans multiple symbols (not sub-symbol flicker): mean well above 1 symbol.
        let observed = runs.iter().sum::<usize>() as f64 / runs.len() as f64;
        assert!(
            observed > 3.0,
            "bursts averaged {observed:.1} symbols — a per-sample chain would flicker near 1"
        );
        assert!(
            (observed - expected_mean).abs() < expected_mean * 0.20,
            "mean burst {observed:.1} symbols not within 20% of 1/p_bg = {expected_mean:.1}"
        );
    }

    #[test]
    fn rejects_invalid_transition_probs() {
        let mut cfg = GilbertElliottConfig::moderate(None);
        cfg.p_gb = 1.5;
        assert!(GilbertElliottChannel::new(cfg).is_err());
    }
}
