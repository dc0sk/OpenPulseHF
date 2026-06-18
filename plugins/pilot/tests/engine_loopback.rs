//! Engine-level loopback for PILOT-QPSK500: full `receive_with_timeout`
//! acquisition path (energy gate → onset refine → AFC settle → demodulate →
//! frame/CRC), proving the mode integrates with the modem engine and that the
//! data-aided AFC hook recovers a realistic carrier offset.

use openpulse_audio::LoopbackBackend;
use openpulse_modem::ModemEngine;
use pilot_plugin::PilotPlugin;
use std::time::Duration;

const PAYLOAD: &[u8] = b"PILOT-QPSK500 engine loopback 0123456789 abcdefghij the quick brown fox";

fn engine() -> (ModemEngine, LoopbackBackend) {
    let lb = LoopbackBackend::new();
    let shared = lb.clone_shared();
    let mut e = ModemEngine::new(Box::new(lb));
    e.register_plugin(Box::new(PilotPlugin::new())).unwrap();
    (e, shared)
}

fn decodes_through_offset(offset_hz: f32) -> bool {
    let (mut tx, tx_shared) = engine();
    tx.set_center_frequency(1500.0 + offset_hz);
    tx.transmit(PAYLOAD, "PILOT-QPSK500", None).unwrap();
    let frame = tx_shared.drain_samples();
    assert!(!frame.is_empty(), "transmit must produce samples");

    let (mut rx, rx_shared) = engine();
    rx.set_center_frequency(1500.0);
    rx_shared.push_frame(&vec![0.0f32; 40000]);
    for chunk in frame.chunks(8000) {
        rx_shared.push_frame(chunk);
    }
    match rx.receive_with_timeout("PILOT-QPSK500", None, Duration::from_secs(10)) {
        Ok(got) => got.len() >= PAYLOAD.len() && &got[..PAYLOAD.len()] == PAYLOAD,
        Err(_) => false,
    }
}

#[test]
fn engine_loopback_clean() {
    assert!(
        decodes_through_offset(0.0),
        "PILOT-QPSK500 must decode through the engine at zero offset"
    );
}

#[test]
fn engine_loopback_through_carrier_offset() {
    for offset in [25.0f32, -25.0] {
        assert!(
            decodes_through_offset(offset),
            "PILOT-QPSK500 must decode through a {offset} Hz carrier offset (engine AFC)"
        );
    }
}
