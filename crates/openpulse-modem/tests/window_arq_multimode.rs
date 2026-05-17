//! Engine-level Window-ARQ integration across representative mode families.
//!
//! Verifies that `receive_with_window_arq()` decodes correctly for multiple
//! registered modulation plugins using the same range-limited LLR combine path.

use bpsk_plugin::BpskPlugin;
use ofdm_plugin::OfdmPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::fec::{ByteRange, WindowArqFeedback};
use openpulse_modem::ModemEngine;
use psk8_plugin::Psk8Plugin;
use qam64_plugin::Qam64Plugin;
use qpsk_plugin::QpskPlugin;
use scfdma_plugin::ScFdmaPlugin;

fn window_feedback_for_rs_block() -> WindowArqFeedback {
    WindowArqFeedback::new(vec![
        ByteRange { start: 0, len: 32 },
        ByteRange { start: 96, len: 32 },
    ])
    .expect("valid feedback")
}

fn run_mode_case(mode: &str) {
    let backend = LoopbackBackend::new();
    let shared = backend.clone_shared();

    let mut engine = ModemEngine::new(Box::new(backend));
    engine.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    engine.register_plugin(Box::new(QpskPlugin::new())).unwrap();
    engine.register_plugin(Box::new(Psk8Plugin::new())).unwrap();
    engine
        .register_plugin(Box::new(Qam64Plugin::new()))
        .unwrap();
    engine
        .register_plugin(Box::new(ScFdmaPlugin::new()))
        .unwrap();
    engine.register_plugin(Box::new(OfdmPlugin::new())).unwrap();

    let payload = format!("window-arq multimode payload for {mode}").into_bytes();

    // Two RS-protected attempts are queued as separate input frames so the
    // engine captures them as distinct retries.
    engine.transmit_with_fec(&payload, mode, None).unwrap();
    let attempt0 = shared.drain_samples();
    assert!(!attempt0.is_empty(), "attempt0 empty for mode {mode}");

    engine.transmit_with_fec(&payload, mode, None).unwrap();
    let attempt1 = shared.drain_samples();
    assert!(!attempt1.is_empty(), "attempt1 empty for mode {mode}");

    shared.push_frame(&attempt0);
    shared.push_frame(&attempt1);

    let decoded = engine
        .receive_with_window_arq(mode, None, 2, &window_feedback_for_rs_block())
        .unwrap();

    assert_eq!(
        decoded, payload,
        "window-arq decode mismatch for mode {mode}"
    );
}

#[test]
fn window_arq_engine_path_across_mode_families() {
    for mode in [
        "BPSK250", "QPSK500", "8PSK500", "64QAM500", "SCFDMA16", "OFDM16",
    ] {
        run_mode_case(mode);
    }
}
