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

/// A frame larger than one 128-byte LDPC block is split across blocks, not rejected. The largest
/// possible frame is 265 bytes (`Frame`'s payload length is a `u8`), i.e. three blocks.
#[test]
fn ldpc_round_trips_a_frame_spanning_several_blocks() {
    for len in [117usize, 118, 125, 200, 255] {
        let mut engine = ldpc_engine();
        let payload: Vec<u8> = (0..len)
            .map(|i| (i.wrapping_mul(37) & 0xff) as u8)
            .collect();
        engine
            .transmit_with_ldpc(&payload, "BPSK250", None)
            .unwrap_or_else(|e| panic!("transmit {len} B: {e}"));
        let recovered = engine
            .receive_with_ldpc("BPSK250", None)
            .unwrap_or_else(|e| panic!("receive {len} B: {e}"));
        assert_eq!(recovered, payload, "{len}-byte payload");
    }
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

/// High-rate LDPC shares the 128-byte info block, so it splits at the same boundaries.
#[test]
fn ldpc_high_rate_round_trips_a_frame_spanning_several_blocks() {
    for len in [117usize, 118, 125, 255] {
        let mut engine = ldpc_engine();
        let payload: Vec<u8> = (0..len)
            .map(|i| (i.wrapping_mul(53) & 0xff) as u8)
            .collect();
        engine
            .transmit_with_ldpc_high_rate(&payload, "BPSK250", None)
            .unwrap_or_else(|e| panic!("transmit {len} B: {e}"));
        let recovered = engine
            .receive_with_ldpc_high_rate("BPSK250", None)
            .unwrap_or_else(|e| panic!("receive {len} B: {e}"));
        assert_eq!(recovered, payload, "{len}-byte payload");
    }
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
