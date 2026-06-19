//! Regression: the 2000-baud RRC modes acquire and decode through a real carrier
//! offset.
//!
//! QPSK2000-RRC and 8PSK2000-RRC previously failed through an offset (QPSK2000-RRC
//! decoded only within ±~10 Hz; 8PSK2000-RRC was erratic and failed even at 0 Hz).
//! Root cause: `afc_estimate_hz` demodulated the preamble through the passband Hann
//! filter, which is badly mismatched at the RRC modes' low oversampling (4 sps),
//! so the CFO estimate under-/mis-corrected (QPSK ~60% of the true offset; 8PSK a
//! spurious +25 Hz lock at zero offset) — outside the demod's tolerance. Fix: the
//! RRC branch of `afc_estimate_hz` now estimates CFO on the *matched-filtered*
//! baseband preamble (the same downmix + RRC front-end the demod uses). The full
//! ±50 Hz sweep is in the (ignored) carrier_offset_matrix characterization.

use openpulse_audio::LoopbackBackend;
use openpulse_modem::ModemEngine;
use std::time::Duration;

fn decodes_through_offset(mode: &str, offset_hz: f32) -> bool {
    let payload = b"rrc-carrier-offset-0123456789-abcdefghij-0123456789-abcdefghij";

    let mk = || {
        let lb = LoopbackBackend::new();
        let shared = lb.clone_shared();
        let mut e = ModemEngine::new(Box::new(lb));
        e.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))
            .unwrap();
        e.register_plugin(Box::new(psk8_plugin::Psk8Plugin::new()))
            .unwrap();
        (e, shared)
    };

    let (mut tx, tx_shared) = mk();
    tx.set_center_frequency(1500.0 + offset_hz);
    tx.transmit(payload, mode, None).unwrap();
    let frame = tx_shared.drain_samples();
    assert!(!frame.is_empty(), "{mode}: transmit must produce samples");

    let (mut rx, rx_shared) = mk();
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
fn qpsk2000_rrc_decodes_through_offset() {
    for offset in [25.0f32, 50.0, -25.0] {
        assert!(
            decodes_through_offset("QPSK2000-RRC", offset),
            "QPSK2000-RRC must decode through a {offset} Hz carrier offset"
        );
    }
}

#[test]
fn psk8_2000_rrc_decodes_through_offset() {
    for offset in [25.0f32, 50.0, -25.0] {
        assert!(
            decodes_through_offset("8PSK2000-RRC", offset),
            "8PSK2000-RRC must decode through a {offset} Hz carrier offset"
        );
    }
}
