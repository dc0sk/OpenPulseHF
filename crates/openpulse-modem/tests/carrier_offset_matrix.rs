//! Characterization (ignored): carrier-offset decode matrix across the single-carrier
//! PSK/QAM modes, through the REAL engine path (`receive_with_timeout` — energy gate →
//! refine_onset → afc_mini_settle → decode → carrier tracker).
//!
//! Maps exactly which (mode, offset) cells decode so a targeted carrier-recovery fix
//! is grounded in data. Run with:
//!   cargo test -p openpulse-modem --no-default-features --test carrier_offset_matrix -- --ignored --nocapture

use openpulse_audio::LoopbackBackend;
use openpulse_modem::ModemEngine;
use std::time::Duration;

const OFFSETS: [f32; 9] = [-50.0, -40.0, -25.0, -10.0, 0.0, 10.0, 25.0, 40.0, 50.0];

// Single-carrier modes that go through the engine AFC + carrier-recovery path, all at
// 8 kHz (the 9600-baud modes need ≥48 kHz and are out of scope for this 8 kHz sweep).
const MODES: [&str; 17] = [
    "BPSK31",
    "BPSK63",
    "BPSK100",
    "BPSK250", // control: wide margin, should pass
    "QPSK125",
    "QPSK250",
    "QPSK500",
    "QPSK1000",
    "QPSK2000", // rectangular 4 sps — fails even at 0 (known timing issue, not AFC)
    "QPSK2000-RRC",
    "8PSK500",
    "8PSK1000",
    "8PSK2000", // rectangular 4 sps — fails even at 0 (known timing issue, not AFC)
    "8PSK2000-RRC",
    "64QAM500",
    "64QAM1000",
    "64QAM2000-RRC",
];

fn engine() -> (ModemEngine, LoopbackBackend) {
    let lb = LoopbackBackend::new();
    let shared = lb.clone_shared();
    let mut e = ModemEngine::new(Box::new(lb));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(psk8_plugin::Psk8Plugin::new()))
        .unwrap();
    e.register_plugin(Box::new(qam64_plugin::Qam64Plugin::new()))
        .unwrap();
    (e, shared)
}

fn decodes(mode: &str, offset_hz: f32) -> bool {
    let payload = b"carrier-offset-matrix-0123456789-abcdefghij-0123456789";

    let (mut tx, tx_shared) = engine();
    tx.set_center_frequency(1500.0 + offset_hz);
    if tx.transmit(payload, mode, None).is_err() {
        return false;
    }
    let frame = tx_shared.drain_samples();
    if frame.is_empty() {
        return false;
    }

    let (mut rx, rx_shared) = engine();
    rx.set_center_frequency(1500.0);
    rx_shared.push_frame(&vec![0.0f32; 40000]);
    for chunk in frame.chunks(8000) {
        rx_shared.push_frame(chunk);
    }
    match rx.receive_with_timeout(mode, None, Duration::from_secs(10)) {
        Ok(got) => got.len() >= payload.len() && &got[..payload.len()] == payload,
        Err(_) => false,
    }
}

#[test]
#[ignore = "characterization: prints the carrier-offset decode matrix; not a gate"]
fn carrier_offset_decode_matrix() {
    // Header.
    eprint!("\n{:<16}", "mode \\ Δf(Hz)");
    for o in OFFSETS {
        eprint!("{:>6}", o as i32);
    }
    eprintln!();
    for mode in MODES {
        eprint!("{mode:<16}");
        for o in OFFSETS {
            eprint!("{:>6}", if decodes(mode, o) { "ok" } else { "·" });
        }
        eprintln!();
    }
    eprintln!("\n(ok = decoded payload exactly; · = failed/garbled)");
}
