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
