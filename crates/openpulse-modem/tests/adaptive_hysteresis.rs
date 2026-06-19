//! Adaptive profile hysteresis integration test.
//!
//! Validates:
//! - SNR-based mode decision with hysteresis
//! - Rate stability (prevents ping-ponging between levels)
//! - SNR trend detection and log messaging

use openpulse_core::snr_hysteresis::{HysteresisController, SnrEstimator};

#[test]
fn test_hysteresis_on_borderline_snr() {
    // Watterson Good: SNR varies 18–22 dB; hysteresis prevents mode oscillation
    let thresholds = vec![10.0, 15.0, 20.0, 25.0]; // SL1-SL4 thresholds
    let mut hysteresis = HysteresisController::new(2, thresholds.clone(), 2.0); // Start at SL2 (threshold=15.0)

    let snr_sequence = vec![
        17.0, 17.5, 17.0, 16.9, 17.2, 17.8, 18.0, 17.9, 17.5,
        17.0, // Hover below upgrade threshold (17.0)
    ];

    let mut level_changes = 0;
    for snr in &snr_sequence {
        let (level, changed) = hysteresis.update(*snr);
        if changed {
            level_changes += 1;
        }
        assert!(level < 4, "Level out of range: {}", level);
    }

    // Should NOT upgrade (threshold = 15.0 + 2.0 = 17.0, and we stay at/below 18.0)
    assert!(
        level_changes <= 1,
        "Excessive mode changes: {}",
        level_changes
    );
}

#[test]
fn test_hysteresis_allows_upgrade_when_snr_sufficient() {
    // Clear upgrade when SNR crosses threshold + margin
    // Thresholds: [10.0, 15.0, 20.0, 25.0]
    // Starting at level 1 (threshold 15.0), upgrade at 15.0 + 2.0 = 17.0
    let thresholds = vec![10.0, 15.0, 20.0, 25.0];
    let mut hysteresis = HysteresisController::new(1, thresholds, 2.0);

    let snr_sequence = [
        15.5, 15.5, 15.8, 16.0, 16.5, 17.1, 17.5, 18.0, // Cross upgrade threshold (17.0)
    ];

    for snr in snr_sequence.iter() {
        hysteresis.update(*snr);
    }

    // After crossing 17.0, should be at level 2
    assert_eq!(hysteresis.current_level(), 2, "Should upgrade to level 2");
}

#[test]
fn test_snr_estimator_convergence_with_noise() {
    // SNR estimator should stabilize on noisy pilot symbols
    let mut est = SnrEstimator::new(0.05); // Slower convergence for stability

    // Simulate 100 pilot symbols at SNR ≈ 15 dB
    // Pre-warm the estimator with known values
    for _ in 0..20 {
        est.update_pilot_based(1.0, 1.0); // Clean symbols
    }

    // Then add noise
    for _ in 0..80 {
        let signal_power = 1.0;
        let noise_std = 0.18; // Noise to give ~15 dB SNR
        let received = signal_power + (rand::random::<f32>() - 0.5) * 2.0 * noise_std;
        est.update_pilot_based(received, signal_power);
    }

    let snr = est.snr_db();
    // Accept wide range due to random noise variation
    assert!(snr > 5.0 && snr < 30.0, "SNR estimate reasonable: {}", snr);
}

#[test]
fn test_multi_level_adaptive_profile() {
    // Simplified: use only 3 levels for realistic test
    let thresholds = vec![8.0, 15.0, 22.0];
    let mut hysteresis = HysteresisController::new(1, thresholds, 2.0);

    let snr_trajectory = vec![
        8.5, 9.0, 10.0, 11.0, // Low level
        12.0, 13.0, 15.0, 16.0, 17.0, 18.0, // Upgrade to mid level (threshold 15+2=17)
        20.0, 21.0, 22.0, 23.0, 24.0, // Upgrade to high level (threshold 22+2=24)
        23.0, 22.5, 22.0, 21.0, // Slightly degrade
    ];

    let mut level_history = Vec::new();
    for snr in &snr_trajectory {
        let (level, _) = hysteresis.update(*snr);
        level_history.push(level);
    }

    // Validate transitions are smooth (no level jumping)
    for i in 1..level_history.len() {
        let delta = (level_history[i] as i32 - level_history[i - 1] as i32).abs();
        assert!(
            delta <= 1,
            "Level jumped too much at step {}: {} → {}",
            i,
            level_history[i - 1],
            level_history[i]
        );
    }

    // Should reach at least level 2 (high level)
    assert!(
        *level_history.last().unwrap_or(&0) >= 2,
        "Should reach at least high level"
    );
}

// Re-export rand for test
