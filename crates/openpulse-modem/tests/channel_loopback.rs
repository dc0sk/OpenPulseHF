//! Channel-simulation loopback integration tests.
//!
//! These tests substitute for on-air validation by routing TX samples through
//! `openpulse-channel` models (AWGN, Watterson, Gilbert-Elliott) before the RX
//! engine demodulates them.  They serve as the CI gate for Phase 1.6 loopback
//! correctness.
//!
//! All tests use `ChannelSimHarness` from `openpulse_modem::channel_sim`.

use bpsk_plugin::BpskPlugin;
use openpulse_channel::{
    awgn::AwgnChannel, gilbert_elliott::GilbertElliottChannel, watterson::WattersonChannel,
    AwgnConfig, GilbertElliottConfig, WattersonConfig,
};
use openpulse_modem::channel_sim::ChannelSimHarness;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    h.tx_engine
        .register_plugin(Box::new(BpskPlugin::new()))
        .expect("tx BPSK registration");
    h.rx_engine
        .register_plugin(Box::new(BpskPlugin::new()))
        .expect("rx BPSK registration");
    h
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Baseline: no channel distortion; samples passed through unchanged.
#[test]
fn clean_loopback_bpsk250() {
    let mut h = make_harness();
    let payload = b"clean loopback test payload";
    h.tx_engine.transmit(payload, "BPSK250", None).unwrap();
    h.route_clean();
    let rx = h.rx_engine.receive("BPSK250", None).unwrap();
    assert_eq!(rx, payload);
}

/// AWGN at 20 dB SNR: high SNR; byte recovery expected.
#[test]
fn awgn_bpsk31_snr20db() {
    let mut h = make_harness();
    let payload = b"awgn test payload";
    let mut channel = AwgnChannel::new(AwgnConfig::new(20.0, Some(42))).unwrap();
    h.tx_engine.transmit(payload, "BPSK31", None).unwrap();
    h.route(&mut channel);
    let rx = h.rx_engine.receive("BPSK31", None).unwrap();
    assert_eq!(rx, payload);
}

/// Watterson Good F1 (0.1 Hz Doppler, 0.5 ms delay spread, 20 dB SNR).
#[test]
fn watterson_good_f1_bpsk250() {
    let mut h = make_harness();
    let payload = b"watterson good f1 payload";
    let mut channel = WattersonChannel::new(WattersonConfig::good_f1(Some(1))).unwrap();
    h.tx_engine.transmit(payload, "BPSK250", None).unwrap();
    h.route(&mut channel);
    let rx = h.rx_engine.receive("BPSK250", None).unwrap();
    assert_eq!(rx, payload);
}

/// Watterson Good F2 (0.5 Hz Doppler, 1.0 ms delay spread, 15 dB SNR) WITHOUT FEC.
///
/// F2 conditions are more severe than F1; with seed 2 the channel introduces enough
/// errors to prevent exact recovery of raw BPSK250, confirming the model degrades signal.
#[test]
fn watterson_good_f2_bpsk250_no_fec_degrades() {
    let mut h = make_harness();
    let payload = b"watterson f2 payload";
    let mut channel = WattersonChannel::new(WattersonConfig::good_f2(Some(2))).unwrap();
    h.tx_engine.transmit(payload, "BPSK250", None).unwrap();
    h.route(&mut channel);
    let rx = h.rx_engine.receive("BPSK250", None);
    // Seeded RNG is deterministic: seed 2 + F2 conditions cause enough errors that
    // exact byte recovery must not occur.  If this assertion fails after a refactor,
    // switch to a seed or profile that reliably produces degradation.
    assert!(
        rx.map_or(true, |data| data != payload.to_vec()),
        "Watterson F2 seed 2 should degrade raw BPSK250; got exact recovery"
    );
}

/// Gilbert-Elliott light burst channel with FEC+interleaver: recovery expected.
#[test]
fn gilbert_elliott_light_burst_with_fec() {
    let mut h = make_harness();
    let payload = b"gilbert-elliott fec payload";
    let mut channel = GilbertElliottChannel::new(GilbertElliottConfig::light(Some(3))).unwrap();
    h.tx_engine
        .transmit_with_fec_interleaved(payload, "BPSK250", None, 5)
        .unwrap();
    h.route(&mut channel);
    let rx = h
        .rx_engine
        .receive_with_fec_interleaved("BPSK250", None, 5)
        .unwrap();
    assert_eq!(rx, payload);
}

/// Gilbert-Elliott burst channel WITHOUT FEC: demodulation should
/// either fail or produce corrupted output — confirms FEC is load-bearing.
#[test]
fn gilbert_elliott_moderate_burst_no_fec_degrades() {
    let mut h = make_harness();
    let payload = b"no fec payload";
    // Custom destructive burst profile: p_gb=0.1 (burst every ~10 samples), snr_bad=-30 dB
    // (~31× noise amplitude during bursts). The matched filter cannot average out noise this
    // large — symbol errors occur whenever a burst spans a symbol period.
    let mut channel = GilbertElliottChannel::new(GilbertElliottConfig {
        p_gb: 0.1,
        p_bg: 0.05,
        snr_good_db: 20.0,
        snr_bad_db: -30.0,
        seed: Some(99),
    })
    .unwrap();
    h.tx_engine.transmit(payload, "BPSK250", None).unwrap();
    h.route(&mut channel);
    let rx = h.rx_engine.receive("BPSK250", None);
    assert!(
        rx.map_or(true, |data| data != payload.to_vec()),
        "destructive G-E burst should degrade raw BPSK250; got exact recovery"
    );
}
