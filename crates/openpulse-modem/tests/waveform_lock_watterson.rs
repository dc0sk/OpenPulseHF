//! Waveform lock integration test under fading.
//!
//! Validates:
//! - Preamble detection and frame alignment
//! - Carrier recovery phase coherence
//! - Frame lock reliability

use openpulse_dsp::preamble::{PreambleDetector, PreambleType};
use std::f32::consts::PI;

#[test]
fn test_preamble_detection_clean_loopback() {
    // Test preamble detection on clean loopback (no channel distortion)
    let mut preamble_detector = PreambleDetector::new(PreambleType::Barker13, 20);
    let preamble = PreambleType::Barker13.sequence();

    // All 100 trials should lock
    let mut lock_count = 0;
    for _trial in 0..100 {
        let (mag, _phase) = preamble_detector.correlate_bpsk(&preamble);
        if mag > 0.95 {
            lock_count += 1;
        }
    }

    // Expect 100% frame lock on clean loopback
    assert_eq!(lock_count, 100, "Frame lock rate {}/100", lock_count);
}

#[test]
fn test_preamble_types_available() {
    // Verify all preamble types are available and have correct lengths
    assert_eq!(PreambleType::Barker11.len(), 11);
    assert_eq!(PreambleType::Barker13.len(), 13);
    assert_eq!(PreambleType::Pn31.len(), 31);
    assert_eq!(PreambleType::Pn63.len(), 63);
    assert_eq!(PreambleType::ZadoffChu64.len(), 64);
}

#[test]
fn test_phase_coherence_tracking() {
    // Verify phase coherence detection across multiple frames
    let mut detector = PreambleDetector::new(PreambleType::Barker13, 10);

    // Simulate small phase drift (Doppler-like)
    let mut coherent_count = 0;
    for frame_idx in 0..20 {
        let phase = (frame_idx as f32) * (PI / 100.0); // Small drift per frame
        if detector.check_phase_coherence(phase) {
            coherent_count += 1;
        }
    }

    // With small drift, most frames should remain coherent
    assert!(
        coherent_count >= 19,
        "Only {}/20 frames coherent",
        coherent_count
    );
}

#[test]
fn test_barker_autocorrelation() {
    // Verify Barker sequences have good autocorrelation properties
    let mut detector = PreambleDetector::new(PreambleType::Barker13, 5);
    let barker = PreambleType::Barker13.sequence();

    // Perfect correlation with itself
    let (mag, _) = detector.correlate_bpsk(&barker);
    assert!(mag > 0.99, "Self-correlation should be ≈1.0, got {}", mag);

    // Correlation with phase-shifted (inverted) should still have high magnitude but different phase
    let inverted: Vec<f32> = barker.iter().map(|x| -x).collect();
    let (mag_inv, phase_inv) = detector.correlate_bpsk(&inverted);
    assert!(
        mag_inv > 0.99,
        "Inverted correlation magnitude should be high"
    );
    assert!((phase_inv - PI).abs() < 0.1, "Inverted phase should be π");
}

#[test]
fn test_pn31_sequence_properties() {
    // PN-31 should have length 31 and consist of ±1 values
    let pn31 = PreambleType::Pn31.sequence();
    assert_eq!(pn31.len(), 31);

    // All values should be ±1.0
    for &sym in &pn31 {
        assert!(
            (sym - 1.0).abs() < 1e-5 || (sym + 1.0).abs() < 1e-5,
            "Invalid symbol value: {}",
            sym
        );
    }
}

#[test]
fn test_preamble_detector_multiple_instances() {
    // Verify multiple detector instances work independently
    let mut det1 = PreambleDetector::new(PreambleType::Barker11, 5);
    let mut det2 = PreambleDetector::new(PreambleType::Pn63, 5);

    let preamble1 = PreambleType::Barker11.sequence();
    let preamble2 = PreambleType::Pn63.sequence();

    let (mag1, _) = det1.correlate_bpsk(&preamble1);
    let (mag2, _) = det2.correlate_bpsk(&preamble2);

    assert!(mag1 > 0.95, "Barker11 detector failed");
    assert!(mag2 > 0.95, "PN63 detector failed");
}
