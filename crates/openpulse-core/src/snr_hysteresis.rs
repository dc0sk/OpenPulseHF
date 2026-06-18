//! SNR estimation for adaptive threshold and rate decisions.
//!
//! Three techniques: pilot-aided ([`SnrEstimator::update_pilot_based`]),
//! reference-energy ([`SnrEstimator::update_energy_based`]), and a **blind**
//! noise-floor method ([`SnrEstimator::set_noise_floor_from_samples`] +
//! [`SnrEstimator::snr_db_from_samples`]) that needs only a known-silent window
//! (e.g. a gap `DcdState` reports idle) and the active burst — no pilots or
//! reference symbols. Plus an [`es_n0_db`] bandwidth conversion and a
//! rate-level hysteresis controller.

use std::collections::VecDeque;

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

    /// Set the noise-floor power directly from a known-silent window.
    ///
    /// Blind — no pilots or reference symbols: the mean-square of a window the
    /// caller knows to contain only noise (e.g. a gap `DcdState` reports idle).
    /// Pairs with [`snr_db_from_samples`](Self::snr_db_from_samples).
    pub fn set_noise_floor_from_samples(&mut self, silent: &[f32]) {
        if silent.is_empty() {
            return;
        }
        let mean_sq = silent.iter().map(|&s| s * s).sum::<f32>() / silent.len() as f32;
        self.noise_power = mean_sq.max(1e-9);
    }

    /// Blind SNR (dB) of an active burst against the stored noise floor.
    ///
    /// `SNR = (P_active − P_noise) / P_noise` (the qo100 noise-floor method).
    /// Call [`set_noise_floor_from_samples`](Self::set_noise_floor_from_samples)
    /// first. Floors at a large negative value when the burst is at or below the
    /// noise floor.
    pub fn snr_db_from_samples(&self, active: &[f32]) -> f32 {
        if active.is_empty() {
            return f32::NEG_INFINITY;
        }
        let p_active = active.iter().map(|&s| s * s).sum::<f32>() / active.len() as f32;
        let signal = (p_active - self.noise_power).max(1e-9);
        10.0 * (signal / self.noise_power).log10()
    }
}

/// Convert a full-band SNR (dB) to Es/N0 (dB) given samples per symbol.
///
/// With noise spread over the sampled band (≈ `fs`) and signal energy over the
/// symbol-rate band (`Rs = fs / sps`), `Es/N0 = SNR · (fs/Rs) = SNR · sps`, i.e.
/// `Es/N0(dB) = SNR(dB) + 10·log₁₀(sps)` (the qo100 `Analysis.ipynb` relation).
pub fn es_n0_db(snr_db: f32, samples_per_symbol: f32) -> f32 {
    if samples_per_symbol <= 0.0 {
        return snr_db;
    }
    snr_db + 10.0 * samples_per_symbol.log10()
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
    snr_history: VecDeque<f32>,
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
            snr_history: VecDeque::with_capacity(10),
            max_history: 10,
        }
    }

    /// Update with new SNR measurement and return recommended level.
    ///
    /// Returns (new_level, changed) where changed indicates if level switched.
    pub fn update(&mut self, snr_db: f32) -> (u8, bool) {
        self.snr_history.push_back(snr_db);
        if self.snr_history.len() > self.max_history {
            self.snr_history.pop_front();
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

        let recent = *self.snr_history.back().unwrap_or(&0.0);
        let past = *self.snr_history.front().unwrap_or(&0.0);
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
    fn blind_snr_from_noise_floor_and_active_burst() {
        // Noise floor: constant |0.1| ⇒ power 0.01. Active burst: power 0.11 ⇒
        // SNR_linear = (0.11 − 0.01)/0.01 = 10 ⇒ 10 dB.
        let mut est = SnrEstimator::new(0.1);
        let silence: Vec<f32> = (0..1000)
            .map(|i| if i % 2 == 0 { 0.1 } else { -0.1 })
            .collect();
        est.set_noise_floor_from_samples(&silence);
        let a = 0.11f32.sqrt();
        let active: Vec<f32> = (0..1000).map(|i| if i % 2 == 0 { a } else { -a }).collect();
        let snr = est.snr_db_from_samples(&active);
        assert!(
            (snr - 10.0).abs() < 0.5,
            "blind SNR should be ~10 dB, got {snr}"
        );
    }

    #[test]
    fn es_n0_applies_sps_bandwidth_factor() {
        // 10 dB full-band SNR at 8 samples/symbol ⇒ Es/N0 = 10 + 10·log10(8).
        let e = es_n0_db(10.0, 8.0);
        assert!(
            (e - 19.03).abs() < 0.1,
            "Es/N0 should be ~19.03 dB, got {e}"
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
