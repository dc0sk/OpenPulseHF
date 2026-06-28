//! Receiver-side streaming-AGC loopback integration tests.
//!
//! The AGC normalises the captured level before demodulation so the PSK/QAM ladder sees a
//! consistent amplitude despite QSB fading and inter-station level spread. It is opt-in (default
//! off), active-span gated (gain adapts only on carrier-present blocks, frozen through silence),
//! and lives at the single `PipelineStage::InputCapture` seam so every capture path gets it.

use openpulse_audio::LoopbackBackend;
use openpulse_modem::ModemEngine;
use qpsk_plugin::QpskPlugin;

const MODE: &str = "QPSK500";

fn engine() -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(LoopbackBackend::new()));
    e.register_plugin(Box::new(QpskPlugin::new())).unwrap();
    e
}

#[test]
fn agc_off_by_default_and_toggles() {
    let mut e = engine();
    assert!(!e.is_agc_enabled());
    e.enable_agc();
    assert!(e.is_agc_enabled());
    e.disable_agc();
    assert!(!e.is_agc_enabled());
}

#[test]
fn agc_runs_on_the_daemon_streaming_capture_path() {
    // Same tripwire guard as the notch: the daemon's streaming path (`accumulate_capture` →
    // `accumulate_routed`) must reach the front-end seam, not just the `receive()` family.
    let mut e = engine();
    let carrier: Vec<f32> = (0..4096)
        .map(|i| 0.5 * (2.0 * std::f32::consts::PI * 1500.0 * i as f32 / 8000.0).sin())
        .collect();

    // Disabled: the seam is a no-op, counter stays at 0.
    let _ = e.accumulate_capture(Some(MODE), carrier.clone());
    assert_eq!(e.agc_blocks_processed(), 0);

    // Enabled: every block on the daemon path runs the AGC.
    e.enable_agc();
    for _ in 0..3 {
        let _ = e.accumulate_capture(Some(MODE), carrier.clone());
    }
    assert!(
        e.agc_blocks_processed() >= 3,
        "AGC must run on the accumulate_capture (daemon) path, counter = {}",
        e.agc_blocks_processed()
    );
}

#[test]
fn active_span_gating_holds_gain_through_silence_then_adapts_on_a_burst() {
    // The crux of "with active-span gating": a long leading silence must NOT ramp the gain to its
    // boost clamp (the failure mode that bars running an AGC on the raw capture buffer); the gain
    // only climbs once a carrier-present (but low-level) block arrives.
    let mut e = engine();
    e.configure_agc(0.3, 0.05, 40.0);
    e.enable_agc();

    // Below the DCD squelch → silence: gain stays frozen near unity (0 dB).
    let silence = vec![0.0f32; 4096];
    for _ in 0..20 {
        let _ = e.accumulate_capture(Some(MODE), silence.clone());
    }
    assert!(
        e.agc_gain_db().abs() < 1.0,
        "gain must stay ~0 dB through silence, got {:.1} dB",
        e.agc_gain_db()
    );

    // A low-level carrier (well above squelch) → the gain boosts toward the target.
    let weak: Vec<f32> = (0..4096)
        .map(|i| 0.02 * (2.0 * std::f32::consts::PI * 1500.0 * i as f32 / 8000.0).sin())
        .collect();
    for _ in 0..40 {
        let _ = e.accumulate_capture(Some(MODE), weak.clone());
    }
    assert!(
        e.agc_gain_db() > 6.0,
        "gain should boost a weak carrier, got {:.1} dB",
        e.agc_gain_db()
    );
}

#[test]
fn agc_preserves_decode_on_a_low_level_signal() {
    let payload = b"OpenPulseHF streaming AGC loopback gate";

    // Modulate one frame to a loopback backend and drain its samples.
    let tx_backend = LoopbackBackend::new();
    let tx_handle = tx_backend.clone_shared();
    let mut tx = ModemEngine::new(Box::new(tx_backend));
    tx.register_plugin(Box::new(QpskPlugin::new())).unwrap();
    tx.transmit(payload, MODE, None).unwrap();
    let mut samples = tx_handle.drain_samples();
    assert!(!samples.is_empty());

    // Attenuate ~30 dB (×0.03) — a quiet station the ladder would otherwise mis-scale.
    for s in &mut samples {
        *s *= 0.03;
    }

    // Receive with the AGC on; the level is normalised at the front end before demod.
    let rx_backend = LoopbackBackend::new();
    let rx_handle = rx_backend.clone_shared();
    let mut rx = ModemEngine::new(Box::new(rx_backend));
    rx.register_plugin(Box::new(QpskPlugin::new())).unwrap();
    rx.enable_agc();
    rx_handle.fill_samples(&samples);
    let decoded = rx.receive(MODE, None).unwrap_or_default();

    assert_eq!(decoded, payload, "AGC-on decode of a low-level frame must match");
    assert!(
        rx.agc_blocks_processed() > 0,
        "the AGC must have run on the receive path"
    );
}
