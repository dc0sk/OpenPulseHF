//! Unit tests for `openpulse-dsp::doppler_tracker` — the Doppler phase-slope estimator and the
//! adaptive AFC loop-bandwidth controller — driven by synthetic phase sequences.
//!
//! **Scope, stated honestly because it was previously overstated.** This file was named
//! `afc_doppler_watterson.rs` and its header claimed it validated "AFC tracking stability on
//! Watterson F2" and "frequency error <±5 Hz under moderate fading". It does none of those things:
//! there is no `WattersonChannel`, no `ModemEngine`, no audio and no decode anywhere in it. It feeds
//! hand-built phase ramps to two standalone structs. That is a perfectly good unit test — it was just
//! cited as integration coverage it never provided (archetype scan 2026-07-29, finding 7).
//!
//! **The component under test has no production callers.** `DopplerTracker` and
//! `AdaptiveAfcLoopBandwidth` are referenced only from this file; the engine's real acquisition chain
//! is `energy gate → refine_onset → afc_mini_settle → decode → carrier tracker` and does not use
//! either. So a green run here says the estimator is arithmetically sound, and says nothing about the
//! modem's behaviour under fading. The real fade/AFC gates are `bpsk_snr_tracks_a_fade`,
//! `hpx_hf_rungs_survive_fade` and `waveform_lock_watterson`; they are independent of this file.

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

/// A noisy phase ramp must not make the rate estimate diverge, and the adaptive bandwidth must not
/// collapse while tracking it.
///
/// Previously named `test_watterson_f2_doppler_profile`, which it never was — the phase sequence is a
/// linear ramp plus a deterministic sinusoid, not a Watterson channel. Renamed rather than left to
/// keep implying fading coverage that does not exist here.
#[test]
fn test_noisy_phase_ramp_does_not_diverge() {
    let mut tracker = DopplerTracker::new(32);
    let mut bw_ctrl = AdaptiveAfcLoopBandwidth::new(0.02, 0.001, 0.1);

    // Moderate Doppler rate (0.03 rad/symbol ≈ 5 Hz drift @ 1000 baud) plus a small perturbation.
    let doppler_rate = 0.03;
    let mut phase = 0.0;

    for step in 0..100 {
        phase += doppler_rate + 0.01 * (step as f32 * 0.1).sin();

        if let Some((est_rate, _conf)) = tracker.update(phase) {
            if step > 50 {
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

/// The estimator must recover a 3 Hz drift to better than 5 Hz once its window has filled.
///
/// **This assertion used to be unreachable.** It is guarded by `doppler_estimates.len() > 50`, but the
/// test ran 64 symbols through a 32-symbol window, so at most 33 estimates could ever exist — the only
/// live assertion in the whole test was `!doppler_estimates.is_empty()`. A sabotage probe confirmed
/// it: replacing the `< 5.0` bound with an impossible `< -1.0` still passed. The symbol count now
/// exceeds the guard, and `estimates_after_guard` is asserted so the guard cannot silently go
/// unreachable again.
#[test]
fn test_frequency_lock_under_mild_doppler() {
    let mut tracker = DopplerTracker::new(32);
    let mut bw_ctrl = AdaptiveAfcLoopBandwidth::new(0.02, 0.001, 0.1);

    let doppler_hz = 3.0;
    let num_symbols = 256;
    let baud_rate = 1000.0;
    let phase_per_symbol = 2.0 * PI * doppler_hz / baud_rate;

    let mut phase = 0.0;
    let mut doppler_estimates = Vec::new();
    let mut estimates_after_guard = 0usize;

    for _sym in 0..num_symbols {
        if let Some((est_doppler_rad_per_sym, _conf)) = tracker.update(phase) {
            doppler_estimates.push(est_doppler_rad_per_sym);

            let est_doppler_hz = est_doppler_rad_per_sym * baud_rate / (2.0 * PI);
            bw_ctrl.update(18.0, est_doppler_rad_per_sym); // 18 dB SNR

            // After convergence, error should be <5 Hz
            if doppler_estimates.len() > 50 {
                estimates_after_guard += 1;
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
    // Anti-vacuity: the <5 Hz bound above must actually have been evaluated.
    assert!(
        estimates_after_guard > 100,
        "the convergence guard admitted only {estimates_after_guard} estimates — the headline \
         accuracy assertion is (nearly) unreachable again, which is the defect this test had"
    );
}
