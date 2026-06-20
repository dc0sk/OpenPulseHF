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

const MODES: [&str; 4] = [
    "PILOT-QPSK500",
    "PILOT-8PSK500",
    "PILOT-16QAM500",
    "PILOT-32APSK500",
];

fn decodes(mode: &str, offset_hz: f32) -> bool {
    let (mut tx, tx_shared) = engine();
    tx.set_center_frequency(1500.0 + offset_hz);
    tx.transmit(PAYLOAD, mode, None).unwrap();
    let frame = tx_shared.drain_samples();
    assert!(!frame.is_empty(), "transmit must produce samples");

    let (mut rx, rx_shared) = engine();
    rx.set_center_frequency(1500.0);
    rx_shared.push_frame(&vec![0.0f32; 40000]);
    for chunk in frame.chunks(8000) {
        rx_shared.push_frame(chunk);
    }
    match rx.receive_with_timeout(mode, None, Duration::from_secs(10)) {
        Ok(got) => got.len() >= PAYLOAD.len() && &got[..PAYLOAD.len()] == PAYLOAD,
        Err(_) => false,
    }
}

#[test]
fn engine_loopback_clean() {
    for mode in MODES {
        assert!(
            decodes(mode, 0.0),
            "{mode} must decode through the engine at zero offset"
        );
    }
}

#[test]
fn engine_loopback_through_carrier_offset() {
    for mode in MODES {
        for offset in [25.0f32, -25.0] {
            assert!(
                decodes(mode, offset),
                "{mode} must decode through a {offset} Hz carrier offset (engine AFC)"
            );
        }
    }
}

const RRC_MODES: [&str; 4] = [
    "PILOT-QPSK500-RRC",
    "PILOT-8PSK500-RRC",
    "PILOT-16QAM500-RRC",
    "PILOT-32APSK500-RRC",
];

#[test]
fn engine_loopback_rrc_clean() {
    for mode in RRC_MODES {
        assert!(
            decodes(mode, 0.0),
            "{mode} must decode through the engine at zero offset"
        );
    }
}

#[test]
fn engine_loopback_rrc_through_carrier_offset() {
    for mode in RRC_MODES {
        for offset in [25.0f32, -25.0] {
            assert!(
                decodes(mode, offset),
                "{mode} must decode through {offset} Hz (engine AFC)"
            );
        }
    }
}

const BAUD1000_MODES: [&str; 8] = [
    "PILOT-QPSK1000",
    "PILOT-8PSK1000",
    "PILOT-16QAM1000",
    "PILOT-32APSK1000",
    "PILOT-QPSK1000-RRC",
    "PILOT-8PSK1000-RRC",
    "PILOT-16QAM1000-RRC",
    "PILOT-32APSK1000-RRC",
];

#[test]
fn engine_loopback_baud1000() {
    // 1000-baud rungs: 8 samples/symbol at 8 kHz. Clean + a carrier offset through
    // the engine AFC chain, both rectangular and RRC.
    for mode in BAUD1000_MODES {
        assert!(
            decodes(mode, 0.0),
            "{mode} must decode through the engine (clean)"
        );
        assert!(
            decodes(mode, 25.0),
            "{mode} must decode through +25 Hz (engine AFC)"
        );
    }
}

const BAUD2000_RRC_MODES: [&str; 4] = [
    "PILOT-QPSK2000-RRC",
    "PILOT-8PSK2000-RRC",
    "PILOT-16QAM2000-RRC",
    "PILOT-32APSK2000-RRC",
];

#[test]
fn engine_loopback_baud2000_rrc() {
    // 2000-baud rungs: RRC only (4 samples/symbol at 8 kHz; rectangular 2000 would
    // alias). Clean + a carrier offset through the engine AFC chain.
    for mode in BAUD2000_RRC_MODES {
        assert!(
            decodes(mode, 0.0),
            "{mode} must decode through the engine (clean)"
        );
        assert!(
            decodes(mode, 25.0),
            "{mode} must decode through +25 Hz (engine AFC)"
        );
    }
}
