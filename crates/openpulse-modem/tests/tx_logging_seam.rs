//! Regression: regulatory TX logging lives at the single `stage_emit_output` seam, so EVERY
//! transmit path records a frame — not only the plain `transmit()` path (the audit found FEC /
//! ACK / LDPC / turbo / retransmit frames were never logged, the TX mirror of the notch gap).

use bpsk_plugin::BpskPlugin;
use fsk4_plugin::Fsk4Plugin;
use openpulse_audio::LoopbackBackend;
use openpulse_modem::ModemEngine;

fn engine() -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(LoopbackBackend::new()));
    e.register_plugin(Box::new(BpskPlugin::new()))
        .expect("register BPSK");
    e.register_plugin(Box::new(Fsk4Plugin::new()))
        .expect("register FSK4");
    e
}

#[test]
fn plain_transmit_still_logs() {
    let mut e = engine();
    assert_eq!(e.tx_session_log().frame_count(), 0);
    e.transmit(b"plain path", "BPSK250", None).unwrap();
    assert!(e.tx_session_log().frame_count() >= 1);
}

#[test]
fn fec_transmit_path_logs_at_the_emit_seam() {
    let mut e = engine();
    e.transmit_with_fec(b"regulatory log via the FEC path", "BPSK250", None)
        .unwrap();
    assert!(
        e.tx_session_log().frame_count() >= 1,
        "a non-transmit() path must still be logged for regulatory compliance"
    );
}

#[test]
fn ack_path_logs_at_the_emit_seam() {
    use openpulse_core::ack::{AckFrame, AckType};
    let mut e = engine();
    let ack = AckFrame::new(AckType::AckOk, "TESTSESS");
    e.transmit_ack_with_short_fec(&ack, None).unwrap();
    assert!(
        e.tx_session_log().frame_count() >= 1,
        "ACK transmissions must be logged at the emit seam too"
    );
}
