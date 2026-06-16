//! Regression: 8PSK frames preceded by leading silence, delivered incrementally
//! through `receive_with_timeout`, must decode.
//!
//! 8PSK acquisition failed even after the QPSK500 onset fix: with the onset
//! correctly located, `afc_mini_settle` still returned a spurious sub-Hz
//! correction (~0.7 Hz from the short data-aided estimate on a zero-offset
//! frame). 8PSK's `carrier_phase_correct` enters a fragile drift-fit branch at
//! ≥0.5 Hz, so that spurious correction corrupted the decode at the correct
//! onset. The engine now applies an AFC deadband (AFC_SETTLE_DEADBAND_HZ): a
//! settled correction below the estimator noise floor is snapped to zero. QPSK's
//! M-th-power path tolerated the spurious correction, which is why it was 8PSK-
//! specific.

use openpulse_audio::LoopbackBackend;
use openpulse_modem::ModemEngine;
use psk8_plugin::Psk8Plugin;
use std::time::Duration;

fn acquires_through_silence(mode: &str) {
    let loopback = LoopbackBackend::new();
    let shared = loopback.clone_shared();
    let mut engine = ModemEngine::new(Box::new(loopback));
    engine.register_plugin(Box::new(Psk8Plugin::new())).unwrap();

    let payload = b"8psk-acquisition-regression-0123456789-abcdefghij-0123456789";
    engine.transmit(payload, mode, None).unwrap();
    let frame = shared.drain_samples();
    assert!(!frame.is_empty(), "transmit must produce samples");

    shared.push_frame(&vec![0.0f32; 40000]);
    for chunk in frame.chunks(8000) {
        shared.push_frame(chunk);
    }

    let got = engine
        .receive_with_timeout(mode, None, Duration::from_secs(10))
        .unwrap_or_else(|e| panic!("{mode} frame with leading silence must decode: {e}"));
    assert_eq!(&got[..payload.len()], payload, "{mode} payload mismatch");
}

#[test]
fn psk8_500_frame_with_leading_silence_decodes() {
    acquires_through_silence("8PSK500");
}

#[test]
fn psk8_1000_frame_with_leading_silence_decodes() {
    acquires_through_silence("8PSK1000");
}
