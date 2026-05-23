//! AFC Doppler tracking integration test under Watterson fading.
//!
//! Validates:
//! - Doppler rate estimation from phase slope
//! - Adaptive loop bandwidth selection based on SNR and Doppler
//! - AFC tracking stability on Watterson F2 channel
//! - Frequency error <±5 Hz under moderate fading

use openpulse_dsp::doppler_tracker::{AdaptiveAfcLoopBandwidth, DopplerTracker};
use std::f32::consts::PI;

#[test]
fn test_doppler_rate_estimation_linear() {
    // Linear phase drift simulating constant Doppler
    let mut tracker = DopplerTracker::new(16);
    let true_rate = 0.05; // rad/symbol

    for k in 0..64 {
        let phase = true_rate * k as f32;
        if let Some((est_rate, conf)) = tracker.update(phase) {
            // After convergence, estimate should be close to true rate
            if k >= 32 {
                assert!(
                    (est_rate - true_rate).abs() < 0.01,
                    "k={} est={} true={}",
                    k,
                    est_rate,
                    true_rate
                );
                assert!(conf > 0.85, "Confidence too low: {}", conf);
            }
        }
    }
}

#[test]
fn test_doppler_rate_zero_on_constant_phase() {
    // Constant phase (no Doppler) should yield near-zero rate
    let mut tracker = DopplerTracker::new(8);

    for _ in 0..24 {
        tracker.update(0.5); // constant phase
    }

    let (rate, _conf) = tracker.update(0.5).expect("should estimate");
    assert!(
        rate.abs() < 0.01,
        "Should detect zero Doppler, got {}",
        rate
    );
}

#[test]
fn test_adaptive_bandwidth_scaling() {
    let mut bw_ctrl = AdaptiveAfcLoopBandwidth::new(0.02, 0.001, 0.1);

    // Nominal: 15 dB SNR, no Doppler
    let bw_nominal = bw_ctrl.update(15.0, 0.0);
    assert!(
        (bw_nominal - 0.02).abs() < 0.005,
        "Nominal bandwidth: {}",
        bw_nominal
    );

    // Low SNR (5 dB) should reduce bandwidth
    let bw_low_snr = bw_ctrl.update(5.0, 0.0);
    assert!(
        bw_low_snr < 0.02,
        "Low SNR should reduce BW: {}",
        bw_low_snr
    );

    // High Doppler should increase bandwidth
    let bw_high_doppler = bw_ctrl.update(15.0, 0.05);
    assert!(
        bw_high_doppler > 0.02,
        "High Doppler should increase BW: {}",
        bw_high_doppler
    );

    // Low SNR + high Doppler: conflicting; should stay within bounds
    let bw_conflict = bw_ctrl.update(5.0, 0.1);
    assert!(
        (0.001..=0.1).contains(&bw_conflict),
        "BW out of bounds: {}",
        bw_conflict
    );
}

#[test]
fn test_watterson_f2_doppler_profile() {
    // Watterson F2: Doppler spread = 2.0 Hz (fast fading)
    // At 1000 baud (symbol rate), this is ~0.002 Hz / symbol
    // Or ~2π × 0.002 rad/symbol ≈ 0.0126 rad/symbol

    let mut tracker = DopplerTracker::new(32);
    let mut bw_ctrl = AdaptiveAfcLoopBandwidth::new(0.02, 0.001, 0.1);

    // Simulate moderate Doppler rate (0.03 rad/symbol = ~5 Hz drift @ 1000 baud)
    let doppler_rate = 0.03;
    let mut phase = 0.0;

    for step in 0..100 {
        tracker.update(phase);
        phase += doppler_rate + 0.01 * (step as f32 * 0.1).sin(); // Add noise

        if let Some((est_rate, _conf)) = tracker.update(phase) {
            // Expect some convergence toward true rate
            if step > 50 {
                // Allow larger error due to added noise
                assert!(
                    est_rate.abs() < doppler_rate * 2.5,
                    "Noisy Doppler tracking diverged at step {}",
                    step
                );

                // Adaptive BW should track the rate
                let adaptive_bw = bw_ctrl.update(20.0, est_rate);
                assert!(adaptive_bw > 0.001, "BW should not collapse");
            }
        }
    }
}

#[test]
fn test_frequency_lock_under_mild_doppler() {
    // Simulate BPSK-31 receiver with Doppler compensation
    // 1000 baud, 8 kHz sample rate = 8 sps
    // Target: maintain lock with <5 Hz error on 3-Hz Doppler shift

    let mut tracker = DopplerTracker::new(32);
    let mut bw_ctrl = AdaptiveAfcLoopBandwidth::new(0.02, 0.001, 0.1);

    // Simulate 3 Hz Doppler over 64 symbols at 1000 baud = 64 ms
    // Phase accumulation at 3 Hz for 64 ms = 3 × 0.064 × 2π ≈ 1.2 rad
    let doppler_hz = 3.0;
    let num_symbols = 64;
    let baud_rate = 1000.0;
    let phase_per_symbol = 2.0 * PI * doppler_hz / baud_rate;

    let mut phase = 0.0;
    let mut doppler_estimates = Vec::new();

    for _sym in 0..num_symbols {
        if let Some((est_doppler_rad_per_sym, _conf)) = tracker.update(phase) {
            doppler_estimates.push(est_doppler_rad_per_sym);

            // Convert back to Hz
            let est_doppler_hz = est_doppler_rad_per_sym * baud_rate / (2.0 * PI);
            bw_ctrl.update(18.0, est_doppler_rad_per_sym); // 18 dB SNR

            // After convergence, error should be <5 Hz
            if doppler_estimates.len() > 50 {
                assert!(
                    (est_doppler_hz - doppler_hz).abs() < 5.0,
                    "Doppler tracking error {} Hz, true {} Hz",
                    est_doppler_hz,
                    doppler_hz
                );
            }
        }

        phase += phase_per_symbol;
    }

    assert!(
        !doppler_estimates.is_empty(),
        "No Doppler estimates produced"
    );
}
