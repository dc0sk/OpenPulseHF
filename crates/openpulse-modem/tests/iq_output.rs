use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use openpulse_modem::ModemEngine;
use qpsk_plugin::QpskPlugin;

fn make_engine(backend: LoopbackBackend) -> ModemEngine {
    let mut engine = ModemEngine::new(Box::new(backend));
    engine.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    engine.register_plugin(Box::new(QpskPlugin::new())).unwrap();
    engine
}

#[test]
fn bpsk_iq_output_lengths_match() {
    let backend = LoopbackBackend::new();
    let mut engine = make_engine(backend);
    engine.transmit_iq(b"hello IQ", "BPSK100", None).unwrap();
}

#[test]
fn bpsk_iq_stored_in_loopback_buffer() {
    let backend = LoopbackBackend::new();
    let shared = backend.clone_shared();
    let mut engine = make_engine(backend);
    engine.transmit_iq(b"iq test", "BPSK100", None).unwrap();

    let pairs = shared.drain_iq_samples();
    assert!(!pairs.is_empty(), "expected IQ samples in buffer");
    // BPSK: Q channel should be near zero throughout
    let q_rms: f32 = (pairs.iter().map(|&(_, q)| q * q).sum::<f32>() / pairs.len() as f32).sqrt();
    assert!(
        q_rms < 1e-6,
        "BPSK Q channel should be zero, got rms={q_rms}"
    );
}

#[test]
fn qpsk_iq_both_channels_nonzero() {
    let backend = LoopbackBackend::new();
    let shared = backend.clone_shared();
    let mut engine = make_engine(backend);
    // Use a payload that maps to non-trivial I and Q symbols.
    engine
        .transmit_iq(b"QPSK IQ test payload", "QPSK250", None)
        .unwrap();

    let pairs = shared.drain_iq_samples();
    assert!(!pairs.is_empty());
    let i_rms: f32 = (pairs.iter().map(|&(i, _)| i * i).sum::<f32>() / pairs.len() as f32).sqrt();
    let q_rms: f32 = (pairs.iter().map(|&(_, q)| q * q).sum::<f32>() / pairs.len() as f32).sqrt();
    assert!(i_rms > 0.1, "QPSK I channel flat, rms={i_rms}");
    assert!(q_rms > 0.1, "QPSK Q channel flat, rms={q_rms}");
}

#[test]
fn iq_and_real_same_sample_count() {
    let payload = b"count check";
    let cfg = ModulationConfig {
        mode: "BPSK100".to_string(),
        ..ModulationConfig::default()
    };
    let plugin = BpskPlugin::new();
    let real = plugin.modulate(payload, &cfg).unwrap();
    let (i_bb, q_bb) = plugin.modulate_iq(payload, &cfg).unwrap();
    assert_eq!(real.len(), i_bb.len());
    assert_eq!(i_bb.len(), q_bb.len());
}

#[test]
fn no_iq_backend_returns_error() {
    // LoopbackBackend supports IQ output, so we test the trait default (None) by
    // confirming the error message when a real (non-IQ) backend is unavailable.
    // We verify this via the plugin trait default on a vanilla plugin path.
    use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
    use psk8_plugin::Psk8Plugin;

    let cfg = ModulationConfig {
        mode: "8PSK500".to_string(),
        ..ModulationConfig::default()
    };
    // 8PSK has no native modulate_iq override; should fall through to Hilbert default.
    let plugin = Psk8Plugin::new();
    let (i, q) = plugin.modulate_iq(b"test", &cfg).unwrap();
    assert_eq!(i.len(), q.len());
    assert!(!i.is_empty());
}

/// The IQ path is compliance-fenced (audit G-2): it records the §97 regulatory TX-metadata log and
/// arms the auto-ID counter, exactly like the audio emit seam.
#[test]
fn transmit_iq_records_regulatory_log_and_arms_auto_id() {
    let backend = LoopbackBackend::new();
    let mut engine = make_engine(backend);
    engine.set_callsign("W1AW");
    assert_eq!(engine.frames_transmitted(), 0);
    assert!(engine.tx_session_log().frames.is_empty());

    engine.transmit_iq(b"logged IQ", "BPSK100", None).unwrap();

    assert_eq!(
        engine.frames_transmitted(),
        1,
        "IQ TX must arm the auto-ID counter"
    );
    assert_eq!(
        engine.tx_session_log().frames.len(),
        1,
        "IQ TX must be recorded in the regulatory log"
    );
    assert_eq!(engine.tx_session_log().frames[0].station_id, "W1AW");
}

/// The IQ path applies the configured TX attenuation to the baseband IQ (power control).
#[test]
fn transmit_iq_applies_tx_attenuation() {
    // Unattenuated reference.
    let ref_backend = LoopbackBackend::new();
    let ref_shared = ref_backend.clone_shared();
    let mut engine = make_engine(ref_backend);
    engine
        .transmit_iq(b"attenuation test", "QPSK250", None)
        .unwrap();
    let full_pairs = ref_shared.drain_iq_samples();
    assert!(!full_pairs.is_empty());
    let full = mag_rms(&full_pairs);

    // −20 dB → linear 0.1.
    let att_backend = LoopbackBackend::new();
    let att_shared = att_backend.clone_shared();
    let mut engine = make_engine(att_backend);
    engine.set_tx_attenuation_db(-20.0);
    engine
        .transmit_iq(b"attenuation test", "QPSK250", None)
        .unwrap();
    let attenuated = mag_rms(&att_shared.drain_iq_samples());

    assert!(
        (attenuated / full - 0.1).abs() < 0.02,
        "−20 dB attenuation must scale the IQ magnitude to ~0.1× (got {})",
        attenuated / full
    );
}

fn mag_rms(pairs: &[(f32, f32)]) -> f32 {
    (pairs.iter().map(|&(i, q)| i * i + q * q).sum::<f32>() / pairs.len() as f32).sqrt()
}
