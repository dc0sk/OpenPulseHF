//! SNR estimation for pilot-aided and reference-symbol demodulation.
//!
//! Provides SNR measurement techniques for adaptive threshold decisions.

/// SNR estimator using received power and noise variance.
pub struct SnrEstimator {
    /// Estimated signal power (exponential moving average)
    signal_power: f32,
    /// Estimated noise power (exponential moving average)
    noise_power: f32,
    /// EMA smoothing factor (0.01–0.1)
    alpha: f32,
}

impl SnrEstimator {
    /// Create a new SNR estimator.
    ///
    /// `alpha` — exponential moving average parameter (0.01–0.1 typical).
    ///  Lower values give longer averaging window, more stability.
    pub fn new(alpha: f32) -> Self {
        Self {
            signal_power: 1.0,
            noise_power: 0.1,
            alpha: alpha.clamp(0.001, 0.5),
        }
    }

    /// Update with received IQ sample and estimated channel magnitude.
    ///
    /// For pilot-based channels:
    /// - `received_mag` = |received symbol|
    /// - `channel_mag` = estimated channel gain
    /// - noise is estimated as (received − expected)²
    pub fn update_pilot_based(&mut self, received_mag: f32, channel_mag: f32) {
        let expected_mag = channel_mag;
        let error = received_mag - expected_mag;

        // Update signal power (expected power)
        let signal_power_inst = expected_mag * expected_mag;
        self.signal_power = (1.0 - self.alpha) * self.signal_power + self.alpha * signal_power_inst;

        // Update noise power (error power)
        let noise_power_inst = error * error;
        self.noise_power = (1.0 - self.alpha) * self.noise_power + self.alpha * noise_power_inst;

        // Ensure non-zero noise to avoid division by zero
        self.noise_power = self.noise_power.max(1e-6);
    }

    /// Update with received symbol energy and reference energy.
    ///
    /// Simple method: received energy vs reference.
    pub fn update_energy_based(&mut self, received_energy: f32, reference_energy: f32) {
        let error = received_energy - reference_energy;
        let noise_power_inst = error * error;

        self.signal_power = (1.0 - self.alpha) * self.signal_power + self.alpha * reference_energy;
        self.noise_power = (1.0 - self.alpha) * self.noise_power + self.alpha * noise_power_inst;
        self.noise_power = self.noise_power.max(1e-6);
    }

    /// Get current SNR in dB.
    pub fn snr_db(&self) -> f32 {
        10.0 * (self.signal_power / self.noise_power).log10()
    }

    /// Get current SNR as linear ratio.
    pub fn snr_linear(&self) -> f32 {
        self.signal_power / self.noise_power
    }

    /// Get signal power estimate.
    pub fn signal_power(&self) -> f32 {
        self.signal_power
    }

    /// Get noise power estimate.
    pub fn noise_power(&self) -> f32 {
        self.noise_power
    }

    /// Reset to initial state.
    pub fn reset(&mut self) {
        self.signal_power = 1.0;
        self.noise_power = 0.1;
    }
}

/// Rate-level hysteresis controller.
///
/// Prevents rapid oscillation between speed levels by enforcing
/// 2 dB margins (upgrade at +2 dB, downgrade at -2 dB from threshold).
pub struct HysteresisController {
    /// Current speed level (0–N, typically 0–14)
    current_level: u8,
    /// Hysteresis margin in dB (typical 2.0)
    margin_db: f32,
    /// SNR threshold per level (in dB)
    thresholds: Vec<f32>,
    /// History of SNR measurements (for trend analysis)
    snr_history: Vec<f32>,
    /// Max history depth
    max_history: usize,
}

impl HysteresisController {
    /// Create a new hysteresis controller.
    ///
    /// `initial_level` — starting speed level
    /// `thresholds` — SNR threshold for each level (in dB)
    ///   e.g. [5.0, 8.0, 11.0, 14.0, ...] for SL1-SL4
    /// `margin_db` — hysteresis margin (typical 2.0 dB)
    pub fn new(initial_level: u8, thresholds: Vec<f32>, margin_db: f32) -> Self {
        Self {
            current_level: initial_level,
            margin_db,
            thresholds,
            snr_history: Vec::with_capacity(10),
            max_history: 10,
        }
    }

    /// Update with new SNR measurement and return recommended level.
    ///
    /// Returns (new_level, changed) where changed indicates if level switched.
    pub fn update(&mut self, snr_db: f32) -> (u8, bool) {
        self.snr_history.push(snr_db);
        if self.snr_history.len() > self.max_history {
            self.snr_history.remove(0);
        }

        let old_level = self.current_level;

        // Upgrade if SNR exceeds current threshold + margin
        if self.current_level < (self.thresholds.len() - 1) as u8 {
            let upgrade_threshold = self.thresholds[self.current_level as usize] + self.margin_db;
            if snr_db > upgrade_threshold {
                self.current_level += 1;
            }
        }

        // Downgrade if SNR falls below (current-1) threshold - margin
        if self.current_level > 0 {
            let downgrade_threshold =
                self.thresholds[(self.current_level - 1) as usize] - self.margin_db;
            if snr_db < downgrade_threshold {
                self.current_level -= 1;
            }
        }

        (self.current_level, self.current_level != old_level)
    }

    /// Get current level.
    pub fn current_level(&self) -> u8 {
        self.current_level
    }

    /// Get SNR trend (positive = improving, negative = degrading).
    pub fn snr_trend(&self) -> f32 {
        if self.snr_history.len() < 2 {
            return 0.0;
        }

        let recent = self.snr_history[self.snr_history.len() - 1];
        let past = self.snr_history[0];
        recent - past
    }

    /// Reset state.
    pub fn reset(&mut self, initial_level: u8) {
        self.current_level = initial_level;
        self.snr_history.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snr_estimator_pilot_based() {
        let mut est = SnrEstimator::new(0.1);

        // Perfect pilot: received = expected
        for _ in 0..10 {
            est.update_pilot_based(1.0, 1.0); // No error
        }

        let snr = est.snr_db();
        assert!(snr > 13.0, "Clean pilot should give high SNR: {}", snr);
    }

    #[test]
    fn test_snr_estimator_energy_based() {
        let mut est = SnrEstimator::new(0.1);

        // Reference energy = 1.0, received = 1.1 (small noise)
        for _ in 0..100 {
            est.update_energy_based(1.1, 1.0);
        }

        let snr = est.snr_db();
        assert!((snr - 20.0).abs() < 2.0, "SNR should be ~20 dB: {}", snr);
    }

    #[test]
    fn test_snr_convergence() {
        let mut est = SnrEstimator::new(0.2);

        for _ in 0..50 {
            let signal = 10.0;
            let noise = 1.0;
            est.update_energy_based(signal + noise, signal);
        }

        let snr_db = est.snr_db();
        assert!(
            snr_db > 9.0 && snr_db < 11.0,
            "Should converge to ~10 dB: {}",
            snr_db
        );
    }

    #[test]
    fn test_hysteresis_prevents_oscillation() {
        let thresholds = vec![5.0, 8.0, 11.0, 14.0];
        let mut ctrl = HysteresisController::new(1, thresholds.clone(), 2.0);

        // SNR near threshold but within hysteresis
        let (level, changed1) = ctrl.update(7.9); // Below 8.0 + 2.0 = 10.0
        assert!(level == 1);
        assert!(!changed1);

        let (level, changed2) = ctrl.update(7.8); // Still within hysteresis
        assert!(level == 1);
        assert!(!changed2);

        // SNR exceeds upgrade threshold
        let (level, changed3) = ctrl.update(10.5); // Above 8.0 + 2.0 = 10.0
        assert!(level == 2);
        assert!(changed3);
    }

    #[test]
    fn test_hysteresis_boundaries() {
        let thresholds = vec![5.0, 8.0, 11.0];
        let mut ctrl = HysteresisController::new(1, thresholds, 2.0);

        // Try to upgrade from level 1
        let (level, changed) = ctrl.update(10.5); // 8.0 + 2.0 = 10.0
        assert_eq!(level, 2);
        assert!(changed);

        // Stay in level 2
        let (level, changed) = ctrl.update(10.0);
        assert_eq!(level, 2);
        assert!(!changed);

        // Try to upgrade beyond max
        let (level, changed) = ctrl.update(20.0);
        assert_eq!(level, 2); // Should stay at max
        assert!(!changed);
    }

    #[test]
    fn test_snr_trend_calculation() {
        let thresholds = vec![5.0, 8.0, 11.0];
        let mut ctrl = HysteresisController::new(1, thresholds, 2.0);

        ctrl.update(7.0);
        ctrl.update(8.0);
        ctrl.update(9.0);
        ctrl.update(10.0);

        let trend = ctrl.snr_trend();
        assert!(trend > 0.0, "Trend should be positive (improving)");
    }
}
