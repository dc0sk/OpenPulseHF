//! Integration test: AFC tracking loop converges on a frequency-offset signal.

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::plugin::ModulationConfig;
use openpulse_modem::ModemEngine;

/// Build AFC samples at `fc_actual` Hz using the BPSK modulator directly.
fn make_samples(fc_actual: f32) -> Vec<f32> {
    let cfg = ModulationConfig {
        mode: "BPSK100".to_string(),
        sample_rate: 8000,
        center_frequency: fc_actual,
        ..ModulationConfig::default()
    };
    bpsk_plugin::modulate::bpsk_modulate(b"AFC test payload 0123456789", &cfg).unwrap()
}

fn make_engine() -> (ModemEngine, openpulse_audio::LoopbackBackend) {
    let loopback = LoopbackBackend::new();
    let shared = loopback.clone_shared();
    let mut engine = ModemEngine::new(Box::new(loopback));
    engine.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    (engine, shared)
}

#[test]
fn afc_converges_within_25_frames() {
    let fc_nominal = 1500.0_f32;
    let fc_actual = 1515.0_f32; // 15 Hz offset

    let (mut engine, shared) = make_engine();
    // Engine demodulates at fc_nominal; signal arrives at fc_actual.
    engine.set_center_frequency(fc_nominal);

    for _ in 0..25 {
        // Inject one frame of samples at the offset frequency.
        let samples = make_samples(fc_actual);
        shared.fill_samples(&samples);
        // receive() will call update_afc_estimate and accumulate correction.
        // Ignore decode errors — carrier offset may prevent clean decode.
        let _ = engine.receive("BPSK100", None);
    }

    let correction = engine.afc_correction_hz();
    let residual = (fc_actual - fc_nominal) - correction;
    assert!(
        residual.abs() < 2.0,
        "residual {residual:.2} Hz after 25 frames (correction={correction:.2} Hz)"
    );
}

#[test]
fn afc_disabled_correction_stays_zero() {
    let fc_actual = 1520.0_f32;

    let (mut engine, shared) = make_engine();
    engine.set_center_frequency(1500.0);
    engine.disable_afc();

    for _ in 0..10 {
        let samples = make_samples(fc_actual);
        shared.fill_samples(&samples);
        let _ = engine.receive("BPSK100", None);
    }

    assert_eq!(engine.afc_correction_hz(), 0.0);
}

#[test]
fn reset_afc_clears_state() {
    let fc_actual = 1510.0_f32;

    let (mut engine, shared) = make_engine();
    engine.set_center_frequency(1500.0);

    for _ in 0..5 {
        let samples = make_samples(fc_actual);
        shared.fill_samples(&samples);
        let _ = engine.receive("BPSK100", None);
    }

    assert!(
        engine.afc_correction_hz() != 0.0,
        "expected nonzero correction after 5 frames"
    );
    engine.reset_afc();
    assert_eq!(engine.afc_correction_hz(), 0.0);
    assert!(engine.last_afc_offset_hz().is_none());
}
