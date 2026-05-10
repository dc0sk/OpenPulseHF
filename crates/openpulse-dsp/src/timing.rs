//! Gardner timing error detector for symbol synchronisation.
//!
//! The Gardner algorithm is a non-data-aided timing recovery loop.
//!
//! Error formula (applies after matched filtering):
//!   e[n] = s_mid × (s_next − s_prev)
//!
//! Where s_prev is the previous symbol boundary sample, s_mid is the sample
//! at the midpoint between symbol boundaries (sps/2 samples after s_prev), and
//! s_next is the current symbol boundary sample.  For a Nyquist-filtered signal
//! with perfect timing the midpoint is at the zero-crossing of the ISI-free eye,
//! making the mean error exactly zero and preventing mu from drifting.

/// Gardner timing error detector with an integrated first-order loop filter.
pub struct GardnerDetector {
    /// Proportional gain for the timing error integrator.
    gain: f32,
    /// Accumulated fractional timing offset (in samples), clamped to ±0.49 to prevent symbol slips.
    pub mu: f32,
    /// Sample at the previous symbol boundary (sps samples ago).
    s_prev: f32,
    /// Sample at the midpoint between boundaries (captured at phase == strobe/2).
    s_mid: f32,
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
            s_prev: 0.0,
            s_mid: 0.0,
            phase: 0,
            sps,
        }
    }

    /// Feed one sample into the detector.
    ///
    /// Returns `Some((timing_error, mu))` at each symbol strobe (every `sps`
    /// samples, adjusted by the accumulated `mu`), or `None` otherwise.
    pub fn update(&mut self, sample: f32) -> Option<(f32, f32)> {
        self.phase += 1;

        // Strobe at (sps + mu).round() samples; clamp to at least 1 to avoid a tight spin.
        let strobe = ((self.sps as f32 + self.mu).round() as usize).max(1);

        // Capture the midpoint sample at strobe/2 (halfway between boundaries).
        if self.phase == strobe / 2 {
            self.s_mid = sample;
        }

        if self.phase < strobe {
            return None;
        }
        self.phase = 0;

        // Proper Gardner error: zero when timing is perfect (midpoint at eye zero-crossing).
        let error = self.s_mid * (sample - self.s_prev);
        self.mu += self.gain * error;
        // Clamp mu strictly below ±0.5 so the strobe interval (sps + mu).round() never
        // changes from sps.  Allowing |mu| ≥ 0.5 would cause the strobe to round to
        // sps±1, skipping or doubling a symbol and corrupting all subsequent bytes.
        self.mu = self.mu.clamp(-0.49, 0.49);
        self.s_prev = sample;

        Some((error, self.mu))
    }

    /// Reset the detector state.
    pub fn reset(&mut self) {
        self.mu = 0.0;
        self.s_prev = 0.0;
        self.s_mid = 0.0;
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
        assert_eq!(g.s_prev, 0.0);
        assert_eq!(g.s_mid, 0.0);
        assert_eq!(g.phase, 0);
    }

    #[test]
    fn mu_clamped_within_bounds_on_square_wave() {
        // A square wave alternating +1/-1 produces large Gardner errors at every symbol
        // transition (midpoint is not a zero-crossing of the eye).  The ±0.49 clamp must
        // absorb these errors and prevent mu from escaping its bounds.
        let sps = 4usize;
        let mut g = GardnerDetector::new(sps, 0.1);
        let symbols: Vec<f32> = (0..20)
            .map(|i| if i % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        let mut samples = vec![0.0f32; symbols.len() * sps];
        for (idx, &sym) in symbols.iter().enumerate() {
            for j in 0..sps {
                samples[idx * sps + j] = sym;
            }
        }
        g.pre_arm();
        for &s in &samples {
            g.update(s);
        }
        assert!(
            g.mu.abs() <= 0.49,
            "mu escaped ±0.49 clamp: final mu = {}",
            g.mu
        );
    }

    #[test]
    fn zero_error_on_constant_signal() {
        // A constant signal has no transitions: error = s_mid × (s_next - s_prev) = 0 exactly.
        // mu must not drift from its initial value.
        let mut g = GardnerDetector::new(8, 0.1);
        g.pre_arm();
        let mu_start = g.mu;
        for _ in 0..200 {
            g.update(1.0);
        }
        assert_eq!(g.mu, mu_start, "mu drifted on constant (zero-error) signal");
    }
}
