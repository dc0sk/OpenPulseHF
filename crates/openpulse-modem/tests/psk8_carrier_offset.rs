//! 8PSK acquisition through a real carrier offset.
//!
//! 8PSK500/1000 acquired and decoded at *zero* carrier offset but FAILED through a
//! realistic ~25 Hz offset, while BPSK/QPSK/64QAM all succeeded.
//!
//! Root cause (corrected — see memory `8psk-carrier-offset-gap`): the earlier
//! diagnosis blamed AFC-estimate precision, but a swept-AFC experiment showed the
//! demod failed to decode the 25 Hz frame *even when the applied AFC correction was
//! exactly right*.  The real bug was in `carrier_phase_correct`: when the engine
//! signalled an RF offset (`afc_correction_hz` ≥ 0.5) it fit a per-symbol phase drift
//! from the two 8-symbol preamble halves and extrapolated it across the whole frame.
//! Over an 8-symbol baseline that slope is dominated by per-half ISI, not true drift,
//! so it rotated the dense 45° constellation off its decision grid.  Removing that
//! branch (static phase + Costas only) plus replacing the single-pass Costas with a
//! two-pass decision-directed loop (pass 1 *acquires* the residual frequency, pass 2
//! *tracks* it seeded — the structure 64QAM already uses) closes the characterized gap.
//!
//! A follow-up then improved the 8PSK1000 n=8 settle: at 8 samples/symbol the
//! consecutive-symbol data-aided AFC estimate is erratically ISI-biased (−1…+5 Hz vs
//! offset), so it is now kept only as the wide-range anchor and small residuals are
//! refined with a debiased half-split (ISI-robust vector-sum preamble halves) — see
//! `afc_estimate_hz`.  This made the estimator accurate (sub-Hz at the fixed point)
//! and lifted 8PSK1000 from ~3/9 to ~8/9 offsets; 8PSK500 became fully robust (all
//! offsets, both payloads).  8PSK1000 stays MARGINAL at the edges though — decode
//! there is payload-sensitive because the engine settle dynamics at n=8 leave a thin
//! margin — so its gate below pins only the established +25 Hz case.

use openpulse_audio::LoopbackBackend;
use openpulse_channel::{awgn::AwgnChannel, AwgnConfig, ChannelModel};
use openpulse_modem::ModemEngine;
use psk8_plugin::Psk8Plugin;
use std::time::Duration;

/// Like [`decodes_through_offset`] but adds AWGN to the frame — validates that carrier acquisition
/// (the coarse-CFO grid search) survives noise, not just clean loopback.
fn decodes_through_offset_awgn(mode: &str, offset_hz: f32, snr_db: f32) -> bool {
    let payload = b"8psk-carrier-offset-awgn-0123456789-abcdefghij-0123456789-abc";
    let tx_lb = LoopbackBackend::new();
    let tx_shared = tx_lb.clone_shared();
    let mut tx = ModemEngine::new(Box::new(tx_lb));
    tx.register_plugin(Box::new(Psk8Plugin::new())).unwrap();
    tx.set_center_frequency(1500.0 + offset_hz);
    tx.transmit(payload, mode, None).unwrap();
    let mut frame = tx_shared.drain_samples();
    let mut ch = AwgnChannel::new(AwgnConfig::new(snr_db, Some(7))).unwrap();
    frame = ch.apply(&frame);

    let rx_lb = LoopbackBackend::new();
    let rx_shared = rx_lb.clone_shared();
    let mut rx = ModemEngine::new(Box::new(rx_lb));
    rx.register_plugin(Box::new(Psk8Plugin::new())).unwrap();
    rx_shared.push_frame(&vec![0.0f32; 40000]);
    for chunk in frame.chunks(8000) {
        rx_shared.push_frame(chunk);
    }
    match rx.receive_with_timeout(mode, None, Duration::from_secs(10)) {
        Ok(got) => got.len() >= payload.len() && &got[..payload.len()] == payload,
        Err(_) => false,
    }
}

fn decodes_through_offset(mode: &str, offset_hz: f32) -> bool {
    let payload = b"8psk-carrier-offset-0123456789-abcdefghij-0123456789-abcdefghij";

    let tx_lb = LoopbackBackend::new();
    let tx_shared = tx_lb.clone_shared();
    let mut tx = ModemEngine::new(Box::new(tx_lb));
    tx.register_plugin(Box::new(Psk8Plugin::new())).unwrap();
    tx.set_center_frequency(1500.0 + offset_hz);
    tx.transmit(payload, mode, None).unwrap();
    let frame = tx_shared.drain_samples();
    assert!(!frame.is_empty(), "{mode}: transmit must produce samples");

    let rx_lb = LoopbackBackend::new();
    let rx_shared = rx_lb.clone_shared();
    let mut rx = ModemEngine::new(Box::new(rx_lb));
    rx.register_plugin(Box::new(Psk8Plugin::new())).unwrap();
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
fn psk8_500_decodes_through_offset() {
    for offset in [25.0f32, -25.0, 50.0, -50.0] {
        assert!(
            decodes_through_offset("8PSK500", offset),
            "8PSK500 must decode through a {offset} Hz carrier offset"
        );
    }
}

#[test]
fn psk8_1000_decodes_through_offsets() {
    // The FFT/grid-search coarse-CFO anchor (replacing the erratic n=8 data-aided anchor) fixed the
    // +40 Hz spurious AFC fixed point, lifting 8PSK1000 to 8/9 offsets. Pin the established +25 Hz
    // case and the newly-fixed +40 Hz case. (−10 Hz stays a separate onset/timing gap; matrix map.)
    for offset in [25.0f32, 40.0] {
        assert!(
            decodes_through_offset("8PSK1000", offset),
            "8PSK1000 must decode through a {offset} Hz carrier offset"
        );
    }
}

#[test]
fn psk8_1000_plus40hz_offset_acquires_under_awgn() {
    // The grid-search coarse-CFO fix targeted +40 Hz; validate it acquires UNDER NOISE, not just on
    // clean loopback (the concern a clean offset matrix can't see). 8PSK1000 no-FEC needs ~30 dB, so
    // test at 30 dB — where the acquisition, not the SNR, is what's under test.
    assert!(
        decodes_through_offset_awgn("8PSK1000", 40.0, 30.0),
        "8PSK1000 must acquire + decode a +40 Hz carrier offset through AWGN 30 dB"
    );
}
