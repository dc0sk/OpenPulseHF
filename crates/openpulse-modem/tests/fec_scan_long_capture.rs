//! The scanning FEC receive must find a frame inside a capture longer than the frame.
//!
//! **The defect this pins.** Every decode attempt slices a *fixed-length* window
//! (`start + max_frame_samples`, clamped only by the buffer end), and a demodulator's output byte
//! count is a function of that slice length — not of the frame. `FecCodec::decode` requires an exact
//! multiple of the 255-byte block, so once the capture outlasted the frame the length gate rejected
//! every attempt before Reed–Solomon ever ran.
//!
//! Measured on the dual-card hardware rig, 2026-07-19, everything else held constant:
//!
//! | capture window | `QPSK250 + rs` |
//! |---|---|
//! | ~7 s (buffer ≈ frame) | PASS |
//! | 45 s (default listen) | FAIL — `FEC data length 872 is not a non-zero multiple of 255` |
//!
//! **Why the suite could not have caught it.** `ChannelSimHarness::route*` fills the RX loopback with
//! a buffer that *is* the frame — the receiver's easiest possible case. A real receiver listens for
//! seconds and the frame sits somewhere inside. These tests use
//! [`ChannelSimHarness::route_embedded`], which pads silence around the frame so the receiver has to
//! locate it.
//!
//! A test here that passes without exercising a long buffer is worthless; the length assertions
//! below exist to keep that honest.

use std::time::Duration;

use bpsk_plugin::BpskPlugin;
use openpulse_core::fec::FecMode;
use openpulse_modem::channel_sim::ChannelSimHarness;
use qpsk_plugin::QpskPlugin;

fn harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    for eng in [&mut h.tx_engine, &mut h.rx_engine] {
        eng.register_plugin(Box::new(BpskPlugin::new())).unwrap();
        eng.register_plugin(Box::new(QpskPlugin::new())).unwrap();
    }
    h
}

/// Send `payload` in `mode`+`fec` with `lead`/`trail` silence samples around it, and try to receive.
fn round_trip_embedded(
    mode: &str,
    fec: FecMode,
    payload: &[u8],
    lead: usize,
    trail: usize,
) -> Result<Vec<u8>, String> {
    let mut h = harness();
    h.tx_engine
        .transmit_with_fec_mode(payload, mode, fec, None)
        .map_err(|e| format!("transmit: {e}"))?;
    let frame_samples = h.route_embedded(lead, trail);
    assert!(
        frame_samples > 0,
        "nothing was transmitted — the test would prove nothing"
    );
    h.rx_engine
        .receive_with_fec_mode_timeout(mode, fec, None, Duration::from_millis(6000))
        .map_err(|e| format!("{e}"))
}

/// THE GATE: a frame embedded in a much longer capture must still decode.
///
/// Before the fix this failed with `FEC data length N is not a non-zero multiple of 255`.
#[test]
fn rs_frame_decodes_when_embedded_in_a_long_capture() {
    let payload: Vec<u8> = (0..64u8).collect();
    // A 255-byte RS frame at QPSK250 is ~32 640 samples; pad well past it on both sides so the
    // receiver genuinely has to search, and the fixed-length slice cannot coincide with the frame.
    let lead = 40_000;
    let trail = 120_000;

    let got = round_trip_embedded("QPSK250", FecMode::Rs, &payload, lead, trail)
        .expect("a frame embedded in a long capture must decode");
    assert_eq!(
        got, payload,
        "decoded payload differs from what was transmitted"
    );
}

/// The same frame with only a little padding — the case that already worked, kept as the control so
/// a regression tells you *which* of the two broke.
#[test]
fn rs_frame_decodes_in_a_tight_capture() {
    let payload: Vec<u8> = (0..64u8).collect();
    let got = round_trip_embedded("QPSK250", FecMode::Rs, &payload, 2_000, 2_000)
        .expect("a tightly-captured frame must decode");
    assert_eq!(got, payload);
}

/// `RsStrong` shares the same wire→RS path and the same fix; pin it too.
#[test]
fn rs_strong_frame_decodes_when_embedded_in_a_long_capture() {
    let payload: Vec<u8> = (0..48u8).collect();
    let got = round_trip_embedded("QPSK250", FecMode::RsStrong, &payload, 40_000, 120_000)
        .expect("an RsStrong frame embedded in a long capture must decode");
    assert_eq!(got, payload);
}

/// Anti-vacuity: prove the padding is actually there and large relative to the frame. Without this,
/// a future edit that quietly shrank the padding would leave the gate green while testing the easy
/// case again — exactly how the original defect stayed hidden.
#[test]
fn the_embedded_capture_is_genuinely_longer_than_the_frame() {
    let payload: Vec<u8> = (0..64u8).collect();
    let mut h = harness();
    h.tx_engine
        .transmit_with_fec_mode(&payload, "QPSK250", FecMode::Rs, None)
        .expect("transmit");
    let lead = 40_000;
    let trail = 120_000;
    let frame_samples = h.route_embedded(lead, trail);

    assert!(
        frame_samples > 20_000,
        "an RS-coded QPSK250 frame should be tens of thousands of samples, got {frame_samples}"
    );
    let total = lead + frame_samples + trail;
    assert!(
        total > frame_samples * 4,
        "the capture ({total}) must be several times the frame ({frame_samples}) for this suite to \
         be testing frame LOCATION rather than the easy buffer-is-the-frame case"
    );
}
