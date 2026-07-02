//! SC-FDMA acquisition through a carrier (dial) offset, end-to-end via the engine.
//!
//! Regression for the bug where SC-FDMA decoded only at *exactly* 1500 Hz: the plugin rejected the
//! engine's AFC-corrected centre (`center_frequency != 1500`), and its `estimate_afc_hz` returned the
//! absolute offset every pass (ignoring the applied correction), so the settle diverged. Fix: the
//! demod mixes the AFC-corrected centre back to nominal, and `estimate_afc_hz` reports the residual —
//! so the engine's iterative settle converges and SC-FDMA acquires routine ±10 Hz dial error.

use openpulse_audio::LoopbackBackend;
use openpulse_dsp::acquisition::quadrature;
use openpulse_modem::ModemEngine;
use std::time::Duration;

fn engine() -> (ModemEngine, LoopbackBackend) {
    let lb = LoopbackBackend::new();
    let shared = lb.clone_shared();
    let mut e = ModemEngine::new(Box::new(lb));
    e.register_plugin(Box::new(scfdma_plugin::ScFdmaPlugin::new()))
        .unwrap();
    (e, shared)
}

/// Frequency-shift a real passband signal by `delta` Hz (analytic-signal mix) — simulates a dial
/// offset between the TX and RX radios.
fn shift(samples: &[f32], delta: f32) -> Vec<f32> {
    let h = quadrature(samples);
    samples
        .iter()
        .zip(h.iter())
        .enumerate()
        .map(|(n, (&s, &hs))| {
            let ph = std::f32::consts::TAU * delta * n as f32 / 8000.0;
            s * ph.cos() - hs * ph.sin()
        })
        .collect()
}

fn decodes_through_offset(mode: &str, offset_hz: f32) -> bool {
    let payload = b"scfdma carrier-offset gate 0123456789 abcdefghij 0123456789";
    let (mut tx, tx_shared) = engine();
    if tx.transmit(payload, mode, None).is_err() {
        return false;
    }
    let frame = shift(&tx_shared.drain_samples(), offset_hz);
    let (mut rx, rx_shared) = engine();
    rx.set_center_frequency(1500.0);
    rx_shared.push_frame(&vec![0.0f32; 40000]);
    for chunk in frame.chunks(8000) {
        rx_shared.push_frame(chunk);
    }
    matches!(
        rx.receive_with_timeout(mode, None, Duration::from_secs(10)),
        Ok(got) if got.len() >= payload.len() && &got[..payload.len()] == payload
    )
}

#[test]
fn scfdma52_acquires_dial_offsets() {
    // Previously only 0 Hz decoded; now routine ±8 Hz dial error acquires end-to-end.
    for offset in [0.0f32, 8.0, -8.0] {
        assert!(
            decodes_through_offset("SCFDMA52", offset),
            "SCFDMA52 must acquire a {offset} Hz dial offset through the engine"
        );
    }
}

#[test]
fn scfdma52_16qam_acquires_dial_offset() {
    assert!(
        decodes_through_offset("SCFDMA52-16QAM", 8.0),
        "SCFDMA52-16QAM must acquire an +8 Hz dial offset through the engine"
    );
}
