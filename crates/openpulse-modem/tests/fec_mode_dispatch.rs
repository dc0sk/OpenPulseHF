//! `transmit_with_fec_mode` / `receive_with_fec_mode` dispatch tests.
//!
//! Verifies that every `FecMode` variant routes to the correct codec path via
//! the generic dispatch methods and round-trips cleanly on a loopback engine.

use bpsk_plugin::BpskPlugin;
use fsk4_plugin::Fsk4Plugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::fec::FecMode;
use openpulse_modem::engine::ModemEngine;
use qpsk_plugin::QpskPlugin;

fn engine() -> ModemEngine {
    let audio = Box::new(LoopbackBackend::new());
    let mut e = ModemEngine::new(audio);
    e.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    e.register_plugin(Box::new(QpskPlugin::new())).unwrap();
    e.register_plugin(Box::new(Fsk4Plugin::new())).unwrap();
    e
}

fn roundtrip(fec: FecMode, mode: &str) {
    let mut e = engine();
    let payload = b"fec mode dispatch test";
    e.transmit_with_fec_mode(payload, mode, fec, None)
        .unwrap_or_else(|err| panic!("{fec:?} TX on {mode}: {err}"));
    let got = e
        .receive_with_fec_mode(mode, fec, None)
        .unwrap_or_else(|err| panic!("{fec:?} RX on {mode}: {err}"));
    assert_eq!(&got[..payload.len()], payload, "{fec:?} payload mismatch");
}

#[test]
fn dispatch_none() {
    roundtrip(FecMode::None, "BPSK100");
}

#[test]
fn dispatch_rs() {
    roundtrip(FecMode::Rs, "BPSK100");
}

#[test]
fn dispatch_rs_interleaved() {
    roundtrip(FecMode::RsInterleaved, "BPSK100");
}

#[test]
fn dispatch_concatenated() {
    roundtrip(FecMode::Concatenated, "BPSK100");
}

#[test]
fn dispatch_rs_strong() {
    roundtrip(FecMode::RsStrong, "BPSK100");
}

#[test]
fn dispatch_soft_concatenated() {
    roundtrip(FecMode::SoftConcatenated, "BPSK100");
}

#[test]
fn dispatch_ldpc() {
    roundtrip(FecMode::Ldpc, "BPSK250");
}

#[test]
fn dispatch_turbo() {
    roundtrip(FecMode::Turbo, "BPSK250");
}

#[test]
fn dispatch_short_rs_data_frame_roundtrip() {
    // ShortRs now dispatches to the data-frame path; verify a small payload
    // round-trips through the generic dispatch.
    let mut e = engine();
    let payload = b"short-rs dispatch";
    e.transmit_with_fec_mode(payload, "BPSK250", FecMode::ShortRs, None)
        .expect("ShortRs dispatch transmit");
    let received = e
        .receive_with_fec_mode("BPSK250", FecMode::ShortRs, None)
        .expect("ShortRs dispatch receive");
    assert_eq!(&received, payload);
}

#[test]
fn dispatch_short_rs_rejects_oversized_payload() {
    let mut e = engine();
    let oversized = vec![0u8; 214];
    assert!(
        e.transmit_with_fec_mode(&oversized, "BPSK250", FecMode::ShortRs, None)
            .is_err(),
        "ShortRs must reject payloads > 213 bytes"
    );
}
