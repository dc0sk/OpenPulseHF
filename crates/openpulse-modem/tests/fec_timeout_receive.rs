//! `receive_with_fec_mode_timeout`: timeout-scanning reception of FEC-protected
//! frames (the path the CLI/loopback uses), validated through the channel sim.
use std::time::Duration;

use bpsk_plugin::BpskPlugin;
use openpulse_channel::awgn::AwgnChannel;
use openpulse_channel::AwgnConfig;
use openpulse_core::fec::FecMode;
use openpulse_modem::channel_sim::ChannelSimHarness;
use scfdma_plugin::ScFdmaPlugin;

fn harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    for eng in [&mut h.tx_engine, &mut h.rx_engine] {
        eng.register_plugin(Box::new(BpskPlugin::new())).unwrap();
        eng.register_plugin(Box::new(ScFdmaPlugin::new())).unwrap();
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
fn none_path_unchanged() {
    let payload = b"no-fec timeout path still works";
    assert!(roundtrip("BPSK250", FecMode::None, 20.0, payload));
}
