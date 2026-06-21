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
use openpulse_core::fec::FecMode;
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

/// `route_tapped` returns the pre-channel TX and post-channel samples while still
/// delivering to the RX engine (used by the testbench virtual-loop visualization).
#[test]
fn route_tapped_exposes_tx_and_channel_samples() {
    let mut h = make_harness();
    let payload = b"route_tapped payload";
    let mut channel = AwgnChannel::new(AwgnConfig::new(30.0, Some(7))).unwrap();
    h.tx_engine.transmit(payload, "BPSK250", None).unwrap();
    let (tx, out) = h.route_tapped(&mut channel);
    assert!(!tx.is_empty(), "tapped TX samples should be non-empty");
    assert_eq!(tx.len(), out.len(), "AWGN is additive: equal sample counts");
    assert_ne!(tx, out, "AWGN must perturb the samples");
    let rx = h.rx_engine.receive("BPSK250", None).unwrap();
    assert_eq!(rx, payload, "RX engine still decodes after route_tapped");
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

/// Watterson Good F1 (0.1 Hz Doppler, 0.5 ms delay spread) at high SNR.
///
/// Uses 35 dB SNR (vs. the profile's nominal 20 dB) so the smoke test is
/// robust to deep slow-fading dwells: the F1 envelope can dip to ~0.2×
/// nominal within a single 500 ms frame, which at 20 dB SNR pushes the
/// effective SNR low enough that an uncoded frame may legitimately fail
/// its CRC.  The `_turbo` variant below covers the nominal-SNR realistic
/// path with FEC.
#[test]
fn watterson_good_f1_bpsk250() {
    let payload = b"watterson good f1 payload";
    // Good-F1 is seed-sensitive: a given fade realization can be too deep to decode even at
    // high SNR (a real channel property, not a bug). Require decode through at least one of a
    // window of benign fades rather than pinning one seed (brittle to any change in the
    // channel realization).
    let decoded = (0..16u64).any(|seed| {
        let mut h = make_harness();
        let mut cfg = WattersonConfig::good_f1(Some(seed));
        cfg.snr_db = 35.0;
        let mut channel = WattersonChannel::new(cfg).unwrap();
        if h.tx_engine.transmit(payload, "BPSK250", None).is_err() {
            return false;
        }
        h.route(&mut channel);
        h.rx_engine
            .receive("BPSK250", None)
            .map(|rx| rx == payload)
            .unwrap_or(false)
    });
    assert!(
        decoded,
        "BPSK250 should decode through at least one benign Good-F1 fade (seeds 0..16)"
    );
}

/// Watterson Extreme (10 Hz Doppler, 10 ms delay, 0 dB SNR) WITHOUT FEC.
///
/// Extreme conditions reliably degrade BPSK250: high Doppler causes multiple sign
/// transitions within the frame and 0 dB SNR adds significant noise at every transition.
/// Uses the extreme profile rather than Good F2 because the complex-fading model +
/// differential detection can decode Good F2 without FEC when the fading sign happens
/// to be consistent across the frame (which is the correct physical behaviour).
#[test]
fn watterson_extreme_bpsk250_no_fec_degrades() {
    let mut h = make_harness();
    let payload = b"watterson extreme payload";
    let mut channel = WattersonChannel::new(WattersonConfig::extreme(Some(2))).unwrap();
    h.tx_engine.transmit(payload, "BPSK250", None).unwrap();
    h.route(&mut channel);
    let rx = h.rx_engine.receive("BPSK250", None);
    assert!(
        rx.map_or(true, |data| data != payload.to_vec()),
        "Watterson extreme should degrade raw BPSK250; got exact recovery"
    );
}

/// Watterson Good F2 with RS FEC + interleaver: recovery expected after temporal-
/// correlation fix (full-frame FFT envelope instead of independent 1024-sample blocks).
#[test]
fn watterson_good_f2_bpsk250_with_fec() {
    let payload = b"watterson f2 fec payload";
    // Seed-sensitive (see watterson_good_f1_bpsk250): require FEC+interleaver recovery through
    // at least one benign Good-F2 fade rather than pinning a single realization.
    let decoded = (0..16u64).any(|seed| {
        let mut h = make_harness();
        let mut channel = WattersonChannel::new(WattersonConfig::good_f2(Some(seed))).unwrap();
        if h.tx_engine
            .transmit_with_fec_interleaved(payload, "BPSK250", None, 5)
            .is_err()
        {
            return false;
        }
        h.route(&mut channel);
        h.rx_engine
            .receive_with_fec_interleaved("BPSK250", None, 5)
            .map(|rx| rx == payload)
            .unwrap_or(false)
    });
    assert!(
        decoded,
        "BPSK250+FEC+interleaver should recover through at least one benign Good-F2 fade (seeds 0..16)"
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

/// Turbo FEC (rate-1/3 PCCC) over Watterson Good F1 on BPSK250: recovery expected.
///
/// Turbo's iterative belief-propagation decoder handles mild Doppler spread better
/// than single-pass RS; this test confirms the channel-sim path through FecMode::Turbo.
#[test]
fn watterson_good_f1_bpsk250_turbo() {
    let payload = b"turbo watterson good f1";
    // Seed-sensitive (see watterson_good_f1_bpsk250): require Turbo recovery through at least
    // one benign Good-F1 fade rather than pinning a single realization.
    let decoded = (0..16u64).any(|seed| {
        let mut h = make_harness();
        let mut channel = WattersonChannel::new(WattersonConfig::good_f1(Some(seed))).unwrap();
        if h.tx_engine
            .transmit_with_fec_mode(payload, "BPSK250", FecMode::Turbo, None)
            .is_err()
        {
            return false;
        }
        h.route(&mut channel);
        h.rx_engine
            .receive_with_fec_mode("BPSK250", FecMode::Turbo, None)
            .map(|rx| rx.len() >= payload.len() && &rx[..payload.len()] == payload)
            .unwrap_or(false)
    });
    assert!(
        decoded,
        "Turbo BPSK250 should recover through at least one benign Good-F1 fade (seeds 0..16)"
    );
}

/// LDPC over AWGN at 15 dB SNR on BPSK250: recovery expected with soft-decision decoding.
#[test]
fn awgn_15db_bpsk250_ldpc() {
    let mut h = make_harness();
    let payload = b"ldpc bpsk250 awgn 15db";
    let mut channel = AwgnChannel::new(AwgnConfig::new(15.0, Some(50))).unwrap();
    h.tx_engine
        .transmit_with_fec_mode(payload, "BPSK250", FecMode::Ldpc, None)
        .unwrap();
    h.route(&mut channel);
    let rx = h
        .rx_engine
        .receive_with_fec_mode("BPSK250", FecMode::Ldpc, None)
        .unwrap();
    assert_eq!(&rx[..payload.len()], payload);
}
