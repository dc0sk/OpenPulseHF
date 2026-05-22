//! Multi-mode channel-simulation loopback integration tests (QPSK, OFDM, SCFDMA).

use ofdm_plugin::OfdmPlugin;
use openpulse_channel::{
    awgn::AwgnChannel, watterson::WattersonChannel, AwgnConfig, WattersonConfig,
};
use openpulse_modem::channel_sim::ChannelSimHarness;
use qpsk_plugin::QpskPlugin;
use scfdma_plugin::ScFdmaPlugin;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_harness_qpsk() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    h.tx_engine
        .register_plugin(Box::new(QpskPlugin::new()))
        .expect("tx QPSK registration");
    h.rx_engine
        .register_plugin(Box::new(QpskPlugin::new()))
        .expect("rx QPSK registration");
    h
}

fn make_harness_ofdm() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    h.tx_engine
        .register_plugin(Box::new(OfdmPlugin::new()))
        .expect("tx OFDM registration");
    h.rx_engine
        .register_plugin(Box::new(OfdmPlugin::new()))
        .expect("rx OFDM registration");
    h
}

fn make_harness_scfdma() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    h.tx_engine
        .register_plugin(Box::new(ScFdmaPlugin::new()))
        .expect("tx SCFDMA registration");
    h.rx_engine
        .register_plugin(Box::new(ScFdmaPlugin::new()))
        .expect("rx SCFDMA registration");
    h
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// QPSK500 over AWGN at 20 dB SNR: byte recovery expected.
#[test]
fn qpsk500_awgn_20db() {
    let mut h = make_harness_qpsk();
    let payload = b"qpsk500 awgn test";
    let mut channel = AwgnChannel::new(AwgnConfig::new(20.0, Some(10))).unwrap();
    h.tx_engine.transmit(payload, "QPSK500", None).unwrap();
    h.route(&mut channel);
    let rx = h.rx_engine.receive("QPSK500", None).unwrap();
    assert_eq!(rx, payload);
}

/// QPSK500 over Watterson Good F1 WITHOUT FEC: degrades — ISI from 0.5 ms delay spread
/// exceeds what RS can correct; QPSK500 requires equalization for multipath channels.
#[test]
fn qpsk500_watterson_good_f1_no_fec_degrades() {
    let mut h = make_harness_qpsk();
    let payload = b"qpsk500 watterson f1";
    let mut channel = WattersonChannel::new(WattersonConfig::good_f1(Some(11))).unwrap();
    h.tx_engine.transmit(payload, "QPSK500", None).unwrap();
    h.route(&mut channel);
    let rx = h.rx_engine.receive("QPSK500", None);
    assert!(
        rx.map_or(true, |data| data != payload.to_vec()),
        "Watterson Good F1 should degrade raw QPSK500 (no equalizer)"
    );
}

/// OFDM52 over AWGN at 20 dB SNR: byte recovery expected.
#[test]
fn ofdm52_awgn_20db() {
    let mut h = make_harness_ofdm();
    let payload = b"ofdm52 awgn 20db test payload";
    let mut channel = AwgnChannel::new(AwgnConfig::new(20.0, Some(20))).unwrap();
    h.tx_engine.transmit(payload, "OFDM52", None).unwrap();
    h.route(&mut channel);
    let rx = h.rx_engine.receive("OFDM52", None).unwrap();
    assert_eq!(rx, payload);
}

/// OFDM52 over Watterson Good F1: byte recovery expected at mild Doppler.
#[test]
fn ofdm52_watterson_good_f1() {
    let mut h = make_harness_ofdm();
    let payload = b"ofdm52 watterson f1 test";
    let mut channel = WattersonChannel::new(WattersonConfig::good_f1(Some(21))).unwrap();
    h.tx_engine.transmit(payload, "OFDM52", None).unwrap();
    h.route(&mut channel);
    let rx = h.rx_engine.receive("OFDM52", None).unwrap();
    assert_eq!(rx, payload);
}

/// SCFDMA52-16QAM over AWGN at 20 dB SNR: byte recovery expected with pilot-aided channel estimation.
#[test]
fn scfdma52_16qam_awgn_20db() {
    let mut h = make_harness_scfdma();
    let payload = b"scfdma 16qam test";
    let mut channel = AwgnChannel::new(AwgnConfig::new(20.0, Some(30))).unwrap();
    h.tx_engine
        .transmit(payload, "SCFDMA52-16QAM", None)
        .unwrap();
    h.route(&mut channel);
    let rx = h.rx_engine.receive("SCFDMA52-16QAM", None).unwrap();
    assert_eq!(rx, payload);
}

/// SCFDMA52-64QAM over AWGN at 25 dB SNR: byte recovery expected with MMSE equalization.
#[test]
fn scfdma52_64qam_awgn_25db() {
    let mut h = make_harness_scfdma();
    let payload = b"scfdma 64qam awgn";
    let mut channel = AwgnChannel::new(AwgnConfig::new(25.0, Some(31))).unwrap();
    h.tx_engine
        .transmit(payload, "SCFDMA52-64QAM", None)
        .unwrap();
    h.route(&mut channel);
    let rx = h.rx_engine.receive("SCFDMA52-64QAM", None).unwrap();
    assert_eq!(rx, payload);
}
