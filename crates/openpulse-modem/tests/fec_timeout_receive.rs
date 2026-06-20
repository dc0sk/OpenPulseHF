//! `receive_with_fec_mode_timeout`: timeout-scanning reception of FEC-protected
//! frames (the path the CLI/loopback uses), validated through the channel sim.
use std::time::Duration;

use bpsk_plugin::BpskPlugin;
use openpulse_channel::awgn::AwgnChannel;
use openpulse_channel::AwgnConfig;
use openpulse_core::fec::FecMode;
use openpulse_modem::channel_sim::ChannelSimHarness;
use pilot_plugin::PilotPlugin;
use scfdma_plugin::ScFdmaPlugin;

fn harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    for eng in [&mut h.tx_engine, &mut h.rx_engine] {
        eng.register_plugin(Box::new(BpskPlugin::new())).unwrap();
        eng.register_plugin(Box::new(ScFdmaPlugin::new())).unwrap();
        eng.register_plugin(Box::new(PilotPlugin::new())).unwrap();
    }
    h
}

fn roundtrip(mode: &str, fec: FecMode, snr: f32, payload: &[u8]) -> bool {
    let mut h = harness();
    h.tx_engine
        .transmit_with_fec_mode(payload, mode, fec, None)
        .unwrap();
    let mut ch = AwgnChannel::new(AwgnConfig::new(snr, Some(7))).unwrap();
    h.route(&mut ch);
    matches!(
        h.rx_engine
            .receive_with_fec_mode_timeout(mode, fec, None, Duration::from_millis(4000)),
        Ok(rx) if rx == payload
    )
}

fn roundtrip_sro(mode: &str, fec: FecMode, ppm: f32, payload: &[u8]) -> bool {
    let mut h = harness();
    h.tx_engine
        .transmit_with_fec_mode(payload, mode, fec, None)
        .unwrap();
    h.route_with_sro(ppm);
    matches!(
        h.rx_engine
            .receive_with_fec_mode_timeout(mode, fec, None, Duration::from_millis(6000)),
        Ok(rx) if rx == payload
    )
}

#[test]
fn bpsk_rs_interleaved_timeout() {
    let payload = b"fec timeout receive: rs-interleaved over BPSK250";
    assert!(roundtrip("BPSK250", FecMode::RsInterleaved, 15.0, payload));
}

#[test]
fn scfdma_hom_soft_concatenated_timeout() {
    // The realistic HOM config: soft LLRs + RS+soft-Viterbi, decoded via the
    // timeout-scanning path. 18 dB is comfortably above its threshold.
    let payload: Vec<u8> = (0..64).map(|i| (i * 53 + 7) as u8).collect();
    assert!(roundtrip(
        "SCFDMA52-16QAM",
        FecMode::SoftConcatenated,
        18.0,
        &payload
    ));
}

#[test]
fn pilot_hom_soft_concatenated_timeout() {
    // The pilot dense rungs are structurally compatible with RS+soft-Viterbi:
    // the demod emits genuine LLRs that round-trip through the byte-exact
    // soft-concatenated path via the timeout scanner. (This documents that the
    // combination is valid in sim across AWGN and SRO; on the dual-clock
    // hardware cable the convolutional inner code loses resync and LDPC is the
    // recommended pilot soft FEC -- see docs/dev/dualcard-loopback.md.)
    let payload: Vec<u8> = (0..64).map(|i| (i * 37 + 11) as u8).collect();
    assert!(roundtrip(
        "PILOT-16QAM500",
        FecMode::SoftConcatenated,
        18.0,
        &payload
    ));
}

#[test]
fn pilot_hom_soft_concatenated_tolerates_sro() {
    // Pure sample-rate offset (the dual-clock effect) up to a realistic
    // two-soundcard 200 ppm: the pilot soft-concatenated path round-trips, so
    // the combination is not geometry-incompatible.
    let payload: Vec<u8> = (0..64).map(|i| (i * 53 + 7) as u8).collect();
    assert!(roundtrip_sro(
        "PILOT-8PSK500",
        FecMode::SoftConcatenated,
        200.0,
        &payload
    ));
}

#[test]
fn none_path_unchanged() {
    let payload = b"no-fec timeout path still works";
    assert!(roundtrip("BPSK250", FecMode::None, 20.0, payload));
}

#[test]
fn concatenated_timeout() {
    // Concatenated (Conv½ + RS, hard) now works through the scanning timeout path.
    let payload = b"fec timeout receive: concatenated over BPSK250";
    assert!(roundtrip("BPSK250", FecMode::Concatenated, 15.0, payload));
}

#[test]
fn rs_strong_timeout() {
    let payload = b"fec timeout receive: rs-strong over BPSK250";
    assert!(roundtrip("BPSK250", FecMode::RsStrong, 15.0, payload));
}

#[test]
fn turbo_timeout_does_not_decode() {
    // Turbo is a fixed-block code (QPP block = llrs.len()/3), so the scanning
    // receive can't feed it the exact LLR count — it's single-shot only. (The
    // prior bug was wasting a soft demodulation on it before failing; it is now
    // excluded from the soft set and rejected by the dispatch.)
    let payload = b"turbo single-shot only";
    assert!(!roundtrip("BPSK250", FecMode::Turbo, 20.0, payload));
}
