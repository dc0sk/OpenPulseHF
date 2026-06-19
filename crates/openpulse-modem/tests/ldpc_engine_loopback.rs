//! LDPC engine dispatch integration tests.
//!
//! Verifies that `transmit_with_ldpc` / `receive_with_ldpc` (rate-1/2) and the
//! high-rate (rate ≈8/9) variants round-trip correctly through the
//! `LoopbackBackend`, and that the HARQ policy selects high-rate LDPC for a
//! soft-capable dense rung on a strong channel.

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::fec::FecMode;
use openpulse_modem::engine::ModemEngine;
use psk8_plugin::Psk8Plugin;

fn ldpc_engine() -> ModemEngine {
    let audio = Box::new(LoopbackBackend::new());
    let mut engine = ModemEngine::new(audio);
    engine
        .register_plugin(Box::new(BpskPlugin::new()))
        .expect("BPSK registration");
    engine
        .register_plugin(Box::new(Psk8Plugin::new()))
        .expect("8PSK registration");
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

#[test]
fn ldpc_high_rate_bpsk250_clean_loopback() {
    let mut engine = ldpc_engine();
    let payload = b"high-rate LDPC round-trip";
    engine
        .transmit_with_ldpc_high_rate(payload, "BPSK250", None)
        .expect("transmit_with_ldpc_high_rate");
    let recovered = engine
        .receive_with_ldpc_high_rate("BPSK250", None)
        .expect("receive_with_ldpc_high_rate");
    assert_eq!(&recovered[..payload.len()], payload);
}

#[test]
fn ldpc_high_rate_8psk500_dense_loopback() {
    // The intended use: a dense, soft-capable rung carrying high-rate LDPC.
    let mut engine = ldpc_engine();
    let payload = b"dense rung high-rate LDPC";
    engine
        .transmit_with_ldpc_high_rate(payload, "8PSK500", None)
        .expect("transmit_with_ldpc_high_rate on 8PSK500");
    let recovered = engine
        .receive_with_ldpc_high_rate("8PSK500", None)
        .expect("receive_with_ldpc_high_rate on 8PSK500");
    assert_eq!(&recovered[..payload.len()], payload);
}

#[test]
fn ldpc_high_rate_shares_one_block_limit() {
    // High-rate LDPC has the same 128-byte info block as the rate-1/2 codec,
    // so the single-block gate must reject the same oversized frame.
    let mut engine = ldpc_engine();
    let payload = vec![0xABu8; 125];
    let result = engine.transmit_with_ldpc_high_rate(&payload, "BPSK250", None);
    assert!(
        result.is_err(),
        "high-rate LDPC must reject a frame exceeding one block"
    );
}

#[test]
fn harq_selects_high_rate_ldpc_for_dense_soft_mode() {
    let engine = ldpc_engine();

    // 8PSK500 is soft-capable: on a strong, low-fade channel the HARQ policy
    // upgrades the first attempt from RS to high-rate LDPC.
    let dense = engine.select_harq_decision_for_mode("8PSK500", 28.0, 1.0, 0);
    assert_eq!(dense.fec_mode, FecMode::LdpcHighRate);

    // Below the high-rate floor, the same dense mode stays on the RS ladder.
    let dense_low = engine.select_harq_decision_for_mode("8PSK500", 24.0, 1.0, 0);
    assert_ne!(dense_low.fec_mode, FecMode::LdpcHighRate);

    // A failed first attempt drops back to more-protective coding.
    let dense_retry = engine.select_harq_decision_for_mode("8PSK500", 28.0, 1.0, 1);
    assert!(dense_retry.fec_mode.strength() > FecMode::LdpcHighRate.strength());
}
