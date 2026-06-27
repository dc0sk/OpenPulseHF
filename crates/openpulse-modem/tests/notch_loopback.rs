//! Receiver-side automatic-notch loopback integration test.
//!
//! Routes a single-carrier frame through a QRM channel (a strong CW tone just outside the
//! signal's occupied band) and checks that the engine's receiver notch — which protects the
//! active mode's own band — recovers a decode that fails without it.

use openpulse_channel::{qrm::QrmChannel, QrmConfig, ToneConfig};
use openpulse_modem::channel_sim::ChannelSimHarness;
use qpsk_plugin::QpskPlugin;

const MODE: &str = "QPSK500";

fn qrm_channel(tone_hz: f32, amp: f32, seed: u64) -> QrmChannel {
    QrmChannel::new(QrmConfig {
        tones: vec![ToneConfig {
            frequency_hz: tone_hz,
            amplitude: amp,
        }],
        noise_floor_snr_db: Some(20.0),
        sample_rate: 8000,
        seed: Some(seed),
    })
    .expect("qrm channel")
}

fn harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    h.tx_engine
        .register_plugin(Box::new(QpskPlugin::new()))
        .expect("tx reg");
    h.rx_engine
        .register_plugin(Box::new(QpskPlugin::new()))
        .expect("rx reg");
    h
}

/// One trial: transmit `payload` through the QRM channel and decode, optionally with the
/// receiver notch enabled. Returns the decoded bytes (or an empty vec on decode failure).
fn trial(payload: &[u8], notch: bool, tone_hz: f32, amp: f32) -> Vec<u8> {
    let mut h = harness();
    if notch {
        h.rx_engine.enable_notch();
    }
    let mut ch = qrm_channel(tone_hz, amp, 0xBEEF);
    h.tx_engine.transmit(payload, MODE, None).unwrap();
    let _ = h.route_tapped(&mut ch);
    h.rx_engine.receive(MODE, None).unwrap_or_default()
}

#[test]
fn notch_recovers_decode_against_out_of_band_qrm() {
    let payload = b"OpenPulseHF receiver notch loopback gate";
    // A strong CW tone at 2600 Hz — outside QPSK500's protected band (1500 +/- 500 = 1000..2000).
    let (tone_hz, amp) = (2600.0, 4.0);

    let off = trial(payload, false, tone_hz, amp);
    let on = trial(payload, true, tone_hz, amp);

    assert_ne!(
        off,
        payload.to_vec(),
        "baseline should be corrupted by the strong out-of-band tone (got a clean decode \
         — pick a harsher tone)"
    );
    assert_eq!(
        on,
        payload.to_vec(),
        "the receiver notch should recover the decode by removing the out-of-band tone"
    );
}

#[test]
fn notch_is_off_by_default_and_toggles() {
    let mut h = harness();
    assert!(!h.rx_engine.is_notch_enabled());
    h.rx_engine.enable_notch();
    assert!(h.rx_engine.is_notch_enabled());
    h.rx_engine.disable_notch();
    assert!(!h.rx_engine.is_notch_enabled());
}
