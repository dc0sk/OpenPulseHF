//! Full selective Window-ARQ engine path integration tests.

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::fec::{ByteRange, FecCodec, WindowArqFeedback};
use openpulse_core::frame::Frame;
use openpulse_modem::ModemEngine;

fn setup_engine() -> ModemEngine {
    let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
    engine.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    engine
}

#[test]
fn selective_window_retransmit_recovers_corrupted_protected_frame() {
    let mut engine = setup_engine();

    let payload = b"window-arq selective full-path payload".to_vec();
    let frame = Frame::new(7, payload.clone()).unwrap();
    let wire = frame.encode();
    let protected = FecCodec::new().encode(&wire);

    let feedback = WindowArqFeedback::new(vec![
        ByteRange { start: 8, len: 16 },
        ByteRange { start: 80, len: 24 },
    ])
    .unwrap();

    let mut corrupted = protected.clone();
    for i in 8..24 {
        corrupted[i] ^= 0x5A;
    }
    for i in 80..104 {
        corrupted[i] ^= 0xA5;
    }

    engine
        .transmit_window_retransmit_packet(&protected, &feedback, "BPSK250", None)
        .unwrap();

    let recovered = engine
        .receive_with_window_arq_selective("BPSK250", None, &mut corrupted, 1)
        .unwrap();

    assert_eq!(recovered, payload);
}

#[test]
fn selective_window_retransmit_rejects_zero_packets() {
    let mut engine = setup_engine();
    let mut protected = vec![0u8; 255];

    let err = engine
        .receive_with_window_arq_selective("BPSK250", None, &mut protected, 0)
        .unwrap_err();

    let msg = format!("{err}");
    assert!(msg.contains("n_packets must be >= 1"));
}
