//! Gardner timing error detector for symbol synchronisation.
//!
//! The Gardner algorithm is a non-data-aided timing recovery loop that requires
//! two samples per symbol period: one at the midpoint and one at the symbol
//! boundary (early and late are derived from adjacent boundary samples).
//!
//! Error formula (applies after matched filtering):
//!   e[n] = s_mid × (s_next − s_prev)
//!
//! The loop integrator accumulates timing corrections and drives a numerically
//! controlled oscillator (NCO) that adjusts where in the sample stream each
//! symbol boundary falls.

/// Gardner timing error detector with an integrated first-order loop filter.
pub struct GardnerDetector {
    /// Proportional gain for the timing error integrator.
    gain: f32,
    /// Accumulated fractional timing offset (in samples, can exceed ±0.5).
    pub mu: f32,
    /// Stored samples: [prev_boundary, midpoint, current_boundary].
    samples: [f32; 3],
    /// Number of samples accumulated since the last boundary.
    phase: usize,
    /// Samples per symbol (integer part used for strobe generation).
    sps: usize,
}

impl GardnerDetector {
    /// Create a new detector.
    ///
    /// `sps` — integer samples per symbol (must be ≥ 2; e.g. 8 for 1000 baud @ 8 kHz).
    /// `gain` — loop gain; 0.01–0.05 is typical for well-conditioned signals.
    pub fn new(sps: usize, gain: f32) -> Self {
        assert!(sps >= 2, "GardnerDetector requires sps >= 2 (got {sps})");
        Self {
            gain,
            mu: 0.0,
            samples: [0.0; 3],
            phase: 0,
            sps,
        }
    }

    /// Feed one sample into the detector.
    ///
    /// Returns `Some((timing_error, mu))` at each symbol strobe (every `sps`
    /// samples, adjusted by the accumulated `mu`), or `None` otherwise.
    pub fn update(&mut self, sample: f32) -> Option<(f32, f32)> {
        // Rotate sample buffer
        self.samples[0] = self.samples[1];
        self.samples[1] = self.samples[2];
        self.samples[2] = sample;
        self.phase += 1;

        // Strobe at (sps + mu).round() samples; clamp to at least 1 to avoid a tight spin.
        let strobe = ((self.sps as f32 + self.mu).round() as usize).max(1);
        if self.phase < strobe {
            return None;
        }
        self.phase = 0;

        let s_prev = self.samples[0];
        let s_mid = self.samples[1];
        let s_next = self.samples[2];

        // Gardner error: zero when timing is perfect
        let error = s_mid * (s_next - s_prev);
        self.mu += self.gain * error;
        // Clamp mu to prevent run-away
        self.mu = self.mu.clamp(-2.0, 2.0);

        Some((error, self.mu))
    }

    /// Reset the detector state.
    pub fn reset(&mut self) {
        self.mu = 0.0;
        self.samples = [0.0; 3];
        self.phase = 0;
    }

    /// Pre-arm the detector so the next sample fed to [`update`] triggers a strobe.
    ///
    /// Use this when the initial ISI-free sampling position is already known
    /// (e.g. from a brute-force preamble search) and the first sample passed
    /// to `update` should be output immediately rather than discarded.
    pub fn pre_arm(&mut self) {
        self.phase = self.sps - 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fires_every_sps_samples_when_aligned() {
        let mut g = GardnerDetector::new(8, 0.01);
        let mut strobe_count = 0usize;
        for i in 0..80 {
            let s = (i as f32 * 0.1).sin();
            if g.update(s).is_some() {
                strobe_count += 1;
            }
        }
        // Expect ~10 strobes in 80 samples at sps=8
        assert!(
            (8..=12).contains(&strobe_count),
            "unexpected strobe count {strobe_count}"
        );
    }

    #[test]
    fn mu_stays_bounded_on_noise() {
        let mut g = GardnerDetector::new(8, 0.05);
        for i in 0..8000 {
            let s = ((i as f32) * 0.1).sin() + 0.5 * ((i as f32) * 0.37).cos();
            g.update(s);
        }
        assert!(g.mu.abs() <= 2.0, "mu out of bounds: {}", g.mu);
    }

    #[test]
    fn reset_clears_all_state() {
        let mut g = GardnerDetector::new(8, 0.05);
        for _ in 0..100 {
            g.update(1.0);
        }
        g.reset();
        assert_eq!(g.mu, 0.0);
        assert_eq!(g.samples, [0.0, 0.0, 0.0]);
        assert_eq!(g.phase, 0);
    }
}
