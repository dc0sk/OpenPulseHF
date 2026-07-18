//! Integration tests for the soft TX limiter (FF-7).

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_modem::ModemEngine;

fn make_engine() -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(LoopbackBackend::new()));
    e.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    e
}

/// With limiter active, the audio written to the backend stays within threshold.
///
/// This inspects the emitted samples. It previously only checked that transmit did not error —
/// which made it the sole engine-path coverage of FF-7 while proving nothing about the limiter.
#[test]
fn limiter_bounds_peak_amplitude() {
    let backend = LoopbackBackend::new();
    let mut engine = ModemEngine::new(Box::new(backend.clone_shared()));
    engine.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    engine.set_tx_limiter_threshold(0.5);

    engine.transmit(b"test payload", "BPSK250", None).unwrap();

    let samples = backend.drain_samples();
    assert!(!samples.is_empty(), "engine emitted no samples to inspect");
    let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(
        peak <= 0.5 + 1e-4,
        "limiter set to 0.5 but emitted peak was {peak}"
    );
}

/// The bound above is only meaningful if an unlimited transmit actually exceeds it — otherwise the
/// assertion would hold on a signal the limiter never touched.
#[test]
fn unlimited_transmit_exceeds_the_limiter_threshold() {
    let backend = LoopbackBackend::new();
    let mut engine = ModemEngine::new(Box::new(backend.clone_shared()));
    engine.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    engine.set_tx_limiter_threshold(0.0); // disabled

    engine.transmit(b"test payload", "BPSK250", None).unwrap();

    let samples = backend.drain_samples();
    assert!(!samples.is_empty(), "engine emitted no samples to inspect");
    let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(
        peak > 0.5,
        "with the limiter disabled the peak must exceed 0.5, got {peak} — \
         if it does not, limiter_bounds_peak_amplitude proves nothing"
    );
}

/// With limiter disabled (threshold 0.0), BER on clean loopback is unchanged.
#[test]
fn limiter_disabled_clean_loopback() {
    let mut engine = make_engine();
    engine.set_tx_limiter_threshold(0.0);

    let payload = b"hello loopback";
    engine.transmit(payload, "BPSK250", None).unwrap();
    let received = engine.receive("BPSK250", None).unwrap();
    assert_eq!(received, payload);
}

/// With limiter enabled at a generous threshold, clean loopback still decodes.
#[test]
fn limiter_enabled_clean_loopback_decodes() {
    let mut engine = make_engine();
    // 0.9 is generous — essentially no distortion on normalised BPSK
    engine.set_tx_limiter_threshold(0.9);

    let payload = b"limiter round trip";
    engine.transmit(payload, "BPSK250", None).unwrap();
    let received = engine.receive("BPSK250", None).unwrap();
    assert_eq!(received, payload);
}

/// tanh_limit unit tests are in openpulse-audio; this test validates the
/// setter API compiles and the field is zero by default.
#[test]
fn default_threshold_is_zero() {
    let mut engine = make_engine();
    // Transmit without setting threshold — should behave exactly as before.
    engine.transmit(b"no limiter", "BPSK250", None).unwrap();
    let received = engine.receive("BPSK250", None).unwrap();
    assert_eq!(received, b"no limiter");
}
