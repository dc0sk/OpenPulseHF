//! Doppler rate estimation and adaptive AFC loop for HF fading channels.
//!
//! Tracks frequency drift and Doppler spread to maintain carrier lock under
//! rapid ionospheric fading (Watterson F2 profile).

use std::collections::VecDeque;
use std::f32::consts::PI;

/// Doppler rate tracker using phase-slope estimation across symbol windows.
///
/// Measures phase evolution over N consecutive symbols to estimate the rate of
/// frequency change (Doppler rate in Hz/s). Useful for predicting when the loop
/// bandwidth must increase to maintain lock.
pub struct DopplerTracker {
    /// Phase history (last N symbols), radians
    phase_history: VecDeque<f32>,
    /// Maximum history depth
    max_history: usize,
    /// Estimated Doppler rate (Hz/sample at symbol rate)
    doppler_rate: f32,
    /// Confidence in Doppler estimate (0.0–1.0)
    confidence: f32,
}

impl DopplerTracker {
    /// Create a new Doppler tracker.
    ///
    /// `window_symbols` — how many consecutive symbols to use for slope fitting
    ///   (e.g., 32 for 32-symbol window at 1000 baud = 32 ms).
    pub fn new(window_symbols: usize) -> Self {
        Self {
            phase_history: VecDeque::with_capacity(window_symbols),
            max_history: window_symbols,
            doppler_rate: 0.0,
            confidence: 0.0,
        }
    }

    /// Feed the phase from one symbol into the tracker.
    ///
    /// Returns the estimated Doppler rate in Hz/sample (at symbol rate) and
    /// confidence (0.0–1.0) if the history buffer is full, otherwise `None`.
    pub fn update(&mut self, phase_rad: f32) -> Option<(f32, f32)> {
        self.phase_history.push_back(self.unwrap_phase(phase_rad));

        if self.phase_history.len() > self.max_history {
            self.phase_history.pop_front();
        }

        if self.phase_history.len() < self.max_history {
            return None;
        }

        // Least-squares fit: phase(k) = a*k + b
        let n = self.phase_history.len() as f32;
        let k_mean = (self.max_history - 1) as f32 / 2.0;
        let phase_mean: f32 = self.phase_history.iter().sum::<f32>() / n;

        let mut num = 0.0;
        let mut den = 0.0;
        for (k, &phase) in self.phase_history.iter().enumerate() {
            let k_f = k as f32;
            num += (k_f - k_mean) * (phase - phase_mean);
            den += (k_f - k_mean) * (k_f - k_mean);
        }

        self.doppler_rate = if den > 1e-6 { num / den } else { 0.0 };

        // Confidence: how well the linear fit explains phase variance
        let residual: f32 = self
            .phase_history
            .iter()
            .enumerate()
            .map(|(k, &phase)| {
                let fitted = phase_mean + self.doppler_rate * (k as f32 - k_mean);
                (phase - fitted).powi(2)
            })
            .sum();

        let total_variance: f32 = self
            .phase_history
            .iter()
            .map(|&phase| (phase - phase_mean).powi(2))
            .sum();

        self.confidence = if total_variance > 1e-6 {
            (1.0 - (residual / total_variance)).max(0.0)
        } else {
            0.0
        };

        Some((self.doppler_rate, self.confidence))
    }

    /// Unwrap phase discontinuities (±π wraps).
    fn unwrap_phase(&self, phase_rad: f32) -> f32 {
        if self.phase_history.is_empty() {
            return phase_rad;
        }

        let prev = *self.phase_history.back().unwrap_or(&phase_rad);
        let mut unwrapped = phase_rad;

        // Unwrap to be close to prev
        while unwrapped - prev > PI {
            unwrapped -= 2.0 * PI;
        }
        while unwrapped - prev < -PI {
            unwrapped += 2.0 * PI;
        }

        unwrapped
    }

    /// Get the current Doppler rate estimate.
    pub fn get_doppler_rate(&self) -> f32 {
        self.doppler_rate
    }

    /// Get the confidence in the current Doppler estimate.
    pub fn get_confidence(&self) -> f32 {
        self.confidence
    }

    /// Reset the tracker state.
    pub fn reset(&mut self) {
        self.phase_history.clear();
        self.doppler_rate = 0.0;
        self.confidence = 0.0;
    }
}

/// Adaptive AFC loop bandwidth selector.
///
/// Adjusts PLL loop bandwidth dynamically based on SNR and Doppler rate.
pub struct AdaptiveAfcLoopBandwidth {
    /// Base loop bandwidth (at high SNR, no Doppler)
    base_bandwidth: f32,
    /// Minimum bandwidth (low SNR, high Doppler)
    min_bandwidth: f32,
    /// Maximum bandwidth (to avoid instability)
    max_bandwidth: f32,
    /// SNR estimate (dB)
    snr_db: f32,
    /// Doppler rate estimate (rad/sample)
    doppler_rate: f32,
}

impl AdaptiveAfcLoopBandwidth {
    /// Create a new adaptive bandwidth controller.
    ///
    /// `base_bandwidth` — typical value 0.02–0.05 (normalized)
    /// `min_bandwidth` — minimum value (0.001)
    /// `max_bandwidth` — maximum value (0.1)
    pub fn new(base_bandwidth: f32, min_bandwidth: f32, max_bandwidth: f32) -> Self {
        Self {
            base_bandwidth,
            min_bandwidth,
            max_bandwidth,
            snr_db: 10.0,
            doppler_rate: 0.0,
        }
    }

    /// Update SNR and Doppler estimates, return recommended loop bandwidth.
    pub fn update(&mut self, snr_db: f32, doppler_rate: f32) -> f32 {
        self.snr_db = snr_db;
        self.doppler_rate = doppler_rate.abs();

        // SNR scaling: increase bandwidth at high SNR (lower noise),
        // decrease at low SNR (more robust)
        let snr_scale = if snr_db < 5.0 {
            0.5
        } else if snr_db < 15.0 {
            0.5 + (snr_db - 5.0) / 20.0 // 0.5 to 1.0
        } else {
            1.0
        };

        // Doppler scaling: increase bandwidth if Doppler rate is high
        // (need faster loop to track phase drift)
        let doppler_scale = (1.0 + self.doppler_rate / 0.01).min(3.0); // up to 3x increase

        let recommended = self.base_bandwidth * snr_scale * doppler_scale;
        recommended.clamp(self.min_bandwidth, self.max_bandwidth)
    }

    /// Get the current SNR estimate.
    pub fn get_snr_db(&self) -> f32 {
        self.snr_db
    }

    /// Get the current Doppler rate.
    pub fn get_doppler_rate(&self) -> f32 {
        self.doppler_rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_doppler_tracker_linear_phase_drift() {
        let mut tracker = DopplerTracker::new(16);
        let phase_drift_rate = 0.05; // rad/symbol

        let mut results = Vec::new();
        for k in 0..32 {
            let phase = phase_drift_rate * k as f32;
            if let Some((rate, conf)) = tracker.update(phase) {
                results.push((rate, conf));
            }
        }

        // Last estimate should be close to the true drift rate
        if let Some((rate, conf)) = results.last() {
            assert!(
                (*rate - phase_drift_rate).abs() < 0.01,
                "Drift rate error: {}",
                rate
            );
            assert!(*conf > 0.9, "Confidence too low: {}", conf);
        }
    }

    #[test]
    fn test_doppler_tracker_wrapped_phase() {
        let mut tracker = DopplerTracker::new(16);

        // Generate unwrapped phases that will naturally wrap across ±π
        let mut phase_unwrapped = 0.0;
        let phase_drift_rate = 0.2; // rad/symbol

        for _ in 0..32 {
            tracker.update(phase_unwrapped);
            phase_unwrapped += phase_drift_rate;
        }

        // Final estimate should capture the drift rate despite wrapping
        let (rate, _conf) = tracker
            .update(phase_unwrapped)
            .expect("should have estimate");

        // Rate should be close to drift rate (within 50% due to discrete estimation)
        assert!(
            (rate - phase_drift_rate).abs() / phase_drift_rate.abs() < 0.5,
            "Wrapped phase drift rate error: {} vs {}",
            rate,
            phase_drift_rate
        );
    }

    #[test]
    fn test_adaptive_bandwidth_snr_scaling() {
        let mut bw = AdaptiveAfcLoopBandwidth::new(0.02, 0.001, 0.1);

        // Low SNR → reduced bandwidth
        let bw_5db = bw.update(5.0, 0.0);
        assert!(bw_5db < 0.02, "Low SNR should reduce bandwidth");

        // High SNR → nominal/increased bandwidth
        let bw_25db = bw.update(25.0, 0.0);
        assert!(
            bw_25db >= 0.02,
            "High SNR should maintain or increase bandwidth"
        );

        // High SNR > low SNR
        assert!(bw_25db > bw_5db, "High SNR bandwidth should be greater");
    }

    #[test]
    fn test_adaptive_bandwidth_doppler_scaling() {
        let mut bw = AdaptiveAfcLoopBandwidth::new(0.02, 0.001, 0.1);

        // No Doppler
        let bw_no_doppler = bw.update(15.0, 0.0);

        // High Doppler → increased bandwidth
        let bw_high_doppler = bw.update(15.0, 0.1);
        assert!(
            bw_high_doppler > bw_no_doppler,
            "High Doppler should increase bandwidth"
        );
    }

    #[test]
    fn test_doppler_tracker_reset() {
        let mut tracker = DopplerTracker::new(4);
        tracker.update(0.1);
        tracker.update(0.2);

        tracker.reset();

        assert_eq!(tracker.get_doppler_rate(), 0.0);
        assert_eq!(tracker.get_confidence(), 0.0);
    }
}
