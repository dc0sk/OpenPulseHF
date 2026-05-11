//! LDPC engine dispatch integration tests.
//!
//! Verifies that `transmit_with_ldpc` / `receive_with_ldpc` round-trip
//! correctly through the `LoopbackBackend` on BPSK250 (clean channel).

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_modem::engine::ModemEngine;

fn ldpc_engine() -> ModemEngine {
    let audio = Box::new(LoopbackBackend::new());
    let mut engine = ModemEngine::new(audio);
    engine
        .register_plugin(Box::new(BpskPlugin::new()))
        .expect("BPSK registration");
    engine
}

#[test]
fn ldpc_bpsk250_clean_loopback() {
    let mut engine = ldpc_engine();
    let payload = b"LDPC engine round-trip";
    engine
        .transmit_with_ldpc(payload, "BPSK250", None)
        .expect("transmit_with_ldpc");
    let recovered = engine
        .receive_with_ldpc("BPSK250", None)
        .expect("receive_with_ldpc");
    assert_eq!(&recovered[..payload.len()], payload);
}

#[test]
fn ldpc_rejects_frame_larger_than_one_block() {
    let mut engine = ldpc_engine();
    // A payload large enough that stage_encode_frame output exceeds 128 bytes.
    // Frame overhead (header + CRC) is ~8 bytes, so 125 bytes of user data overflows.
    let payload = vec![0xABu8; 125];
    let result = engine.transmit_with_ldpc(&payload, "BPSK250", None);
    assert!(
        result.is_err(),
        "expected error for payload exceeding one LDPC block"
    );
}
