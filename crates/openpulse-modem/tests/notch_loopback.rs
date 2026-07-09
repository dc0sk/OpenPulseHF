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
    // Amplitude 8.0: the QPSK crossfade-ISI cancellation (see `qpsk-plugin`'s `crossfade_isi` test)
    // made the receiver strong enough to decode through amp 4.0, so the "baseline is corrupted"
    // precondition needs a harsher tone.
    let (tone_hz, amp) = (2600.0, 8.0);

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

#[test]
fn notch_runs_on_the_daemon_streaming_capture_path() {
    // Regression guard for the gap where the notch was wired only into `stage_capture_input`
    // (the `receive()` family) and never ran on the daemon's streaming path
    // (`accumulate_capture` → `accumulate_routed`). Both now funnel through the single
    // `route_audio_stage(InputCapture)` seam; this drives the daemon's exact call and asserts
    // the front end actually executed via the tripwire counter.
    use openpulse_audio::LoopbackBackend;
    use openpulse_modem::ModemEngine;

    let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
    engine.register_plugin(Box::new(QpskPlugin::new())).unwrap();

    // Notch disabled: the seam is a no-op, counter stays at 0.
    let samples: Vec<f32> = (0..4096).map(|i| (i as f32 * 0.01).sin()).collect();
    let _ = engine.accumulate_capture(Some(MODE), samples.clone());
    assert_eq!(engine.notch_blocks_processed(), 0);

    // Notch enabled: the daemon path must reach the front-end seam every block.
    engine.enable_notch();
    for _ in 0..3 {
        let _ = engine.accumulate_capture(Some(MODE), samples.clone());
    }
    assert!(
        engine.notch_blocks_processed() >= 3,
        "notch must run on the accumulate_capture (daemon) path, counter = {}",
        engine.notch_blocks_processed()
    );
}

#[test]
fn persistence_surfaces_in_band_interferer_for_qsy() {
    use openpulse_audio::LoopbackBackend;
    use openpulse_modem::ModemEngine;

    let backend = LoopbackBackend::new();
    let handle = backend.clone_shared();
    let mut engine = ModemEngine::new(Box::new(backend));
    engine.register_plugin(Box::new(QpskPlugin::new())).unwrap();
    engine.enable_notch();
    engine.set_notch_persistence(3);

    // A lone in-band CW tone (1500 Hz = QPSK500 centre) with no own signal present: each capture
    // is observed as silence, so after enough blocks it is confirmed as an in-band interferer —
    // which a notch can't remove, so it is surfaced for QSY.
    let tone: Vec<f32> = (0..8192)
        .map(|i| 0.2 * (2.0 * std::f32::consts::PI * 1500.0 * i as f32 / 8000.0).sin())
        .collect();
    for _ in 0..4 {
        handle.fill_samples(&tone);
        let _ = engine.receive("QPSK500", None); // demod fails on a pure tone; the capture is observed
    }

    assert!(
        engine
            .in_band_interferers()
            .iter()
            .any(|&f| (f - 1500.0).abs() < 15.0),
        "a persistent in-band tone should be flagged for QSY, got {:?}",
        engine.in_band_interferers()
    );
}
