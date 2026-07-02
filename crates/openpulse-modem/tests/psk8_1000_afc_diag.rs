//! Diagnostic (ignored): localize the 8PSK1000 carrier-offset gap per the DSP playbook's
//! swept-applied-AFC experiment. For each offset, compare the REAL engine path (RX@1500, AFC on)
//! against PERFECT AFC (RX centre = TX centre, AFC disabled). If perfect-AFC decodes where the real
//! path fails, the demod/onset/tracker are innocent and the bug is the AFC settle/estimate.
//!
//!   cargo test -p openpulse-modem --no-default-features --test psk8_1000_afc_diag -- --ignored --nocapture
//!
//! FINDINGS (2026-07-02 spike). 8PSK1000 = 7/9; the 2 failures have TWO distinct causes:
//!  1. **+40 / −50 Hz (some payloads): AFC settle locks a SPURIOUS fixed point.** `perfect_afc`
//!     decodes; the real path's `afc_mini_settle` overshoots (e.g. true +40 → settled_afc +82.3,
//!     residual −133). The `afc_fixed_point_sweep` (psk8-plugin) shows `afc_estimate_hz` is accurate
//!     in ≈[−45,+30] Hz but ERRATIC beyond (±10–14 Hz), so the settle iterates onto a false zero.
//!     At +40 BOTH sub-estimators are unusable: the data-aided one from the n=8 crossfade ISI bias,
//!     AND the half-split one because the ~0.25 rad/symbol ramp is ~4 rad over each 16-symbol half,
//!     collapsing its vector sum — so widening the half-split gate (`rough < baud/64`) CANNOT fix it.
//!  2. **−10 Hz (payload 0): demod fails EVEN WITH PERFECT AFC** → an onset/timing issue at n=8,
//!     payload-sensitive (payload 1 decodes at −10), independent of the AFC.
//! CONCLUSION: no safe parameter fix (the ACQUIRE_BW lever regressed 7/9→5/9; the half-split-gate
//! lever is disproven above). The real fix is a dedicated **FFT-based coarse frequency-acquisition
//! stage** for n=8 (the #1 reference-mining gap), replacing the erratic data-aided anchor with an
//! unambiguous wide-range estimate — a redesign, not a tweak. These probes ground that work.

use openpulse_audio::LoopbackBackend;
use openpulse_modem::ModemEngine;
use std::time::Duration;

fn engine() -> (ModemEngine, LoopbackBackend) {
    let lb = LoopbackBackend::new();
    let shared = lb.clone_shared();
    let mut e = ModemEngine::new(Box::new(lb));
    e.register_plugin(Box::new(psk8_plugin::Psk8Plugin::new()))
        .unwrap();
    (e, shared)
}

fn decode(mode: &str, offset_hz: f32, rx_center: f32, afc: bool, payload: &[u8]) -> bool {
    let (mut tx, tx_shared) = engine();
    tx.set_center_frequency(1500.0 + offset_hz);
    if tx.transmit(payload, mode, None).is_err() {
        return false;
    }
    let frame = tx_shared.drain_samples();
    if frame.is_empty() {
        return false;
    }
    let (mut rx, rx_shared) = engine();
    rx.set_center_frequency(rx_center);
    if !afc {
        rx.disable_afc();
    }
    rx_shared.push_frame(&vec![0.0f32; 40000]);
    for chunk in frame.chunks(8000) {
        rx_shared.push_frame(chunk);
    }
    match rx.receive_with_timeout(mode, None, Duration::from_secs(10)) {
        Ok(got) => got.len() >= payload.len() && &got[..payload.len()] == payload,
        Err(_) => false,
    }
}

/// Run the real path and report where the AFC settled (regardless of decode).
fn settled_afc(mode: &str, offset_hz: f32, payload: &[u8]) -> (bool, f32, f32) {
    let (mut tx, tx_shared) = engine();
    tx.set_center_frequency(1500.0 + offset_hz);
    tx.transmit(payload, mode, None).unwrap();
    let frame = tx_shared.drain_samples();
    let (mut rx, rx_shared) = engine();
    rx.set_center_frequency(1500.0);
    rx_shared.push_frame(&vec![0.0f32; 40000]);
    for chunk in frame.chunks(8000) {
        rx_shared.push_frame(chunk);
    }
    let ok = matches!(
        rx.receive_with_timeout(mode, None, Duration::from_secs(10)),
        Ok(got) if got.len() >= payload.len() && &got[..payload.len()] == payload
    );
    (
        ok,
        rx.afc_correction_hz(),
        rx.last_afc_offset_hz().unwrap_or(f32::NAN),
    )
}

#[test]
#[ignore = "diagnostic; run with --ignored --nocapture"]
fn psk8_1000_settled() {
    let payload: &[u8] = b"carrier-offset-matrix-0123456789-abcdefghij-0123456789";
    for offset in [25.0f32, 40.0, 50.0, -25.0, -40.0, -50.0] {
        let (ok, afc, resid) = settled_afc("8PSK1000", offset, payload);
        // A correct settle should leave afc_correction ≈ offset and residual ≈ 0.
        println!(
            "offset={offset:>6}: decoded={ok:<5} settled_afc={afc:>7.1} residual={resid:>6.1}"
        );
    }
}

#[test]
#[ignore = "diagnostic; run with --ignored --nocapture"]
fn psk8_1000_localize() {
    let mode = "8PSK1000";
    // Two payloads: the matrix payload + a second to surface payload sensitivity.
    let payloads: [&[u8]; 2] = [
        b"carrier-offset-matrix-0123456789-abcdefghij-0123456789",
        b"the quick brown fox jumps over the lazy dog 9876543210 QWERTY",
    ];
    for (pi, payload) in payloads.iter().enumerate() {
        for offset in [-10.0f32, 40.0, -25.0, -50.0, 0.0, 25.0] {
            let real = decode(mode, offset, 1500.0, true, payload);
            let perfect = decode(mode, offset, 1500.0 + offset, false, payload);
            println!(
                "p{pi} offset={offset:>6}: real_path={:<5} perfect_afc={}",
                real, perfect
            );
        }
    }
}
