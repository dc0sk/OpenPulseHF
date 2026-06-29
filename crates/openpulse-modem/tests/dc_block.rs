//! Receiver DC-block (REQ-PHY-02) integration tests.
//!
//! The capture path removes the DC component of each burst (per-burst mean subtraction) at the
//! single `PipelineStage::InputCapture` seam, before the notch/AGC and before the DCD energy gate.
//! The heterodyne PSK/QAM demods already reject a 0 Hz offset, so this never changes a decode; its
//! value is keeping a soundcard/SSB DC offset out of the mean-square energy the DCD/CSMA gate uses.

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_modem::ModemEngine;
use std::f32::consts::PI;

const MODE: &str = "BPSK250";

fn engine_with_handle() -> (ModemEngine, LoopbackBackend) {
    let lb = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(lb.clone_shared()));
    e.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    (e, lb)
}

fn tone(amp: f32, n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| amp * (2.0 * PI * 1500.0 * i as f32 / 8000.0).sin())
        .collect()
}

#[test]
fn dc_block_runs_on_the_daemon_streaming_capture_path() {
    // Tripwire (same as notch/AGC): the daemon's `accumulate_capture` path must reach the seam.
    let (mut e, _lb) = engine_with_handle();
    assert_eq!(e.dc_blocks_processed(), 0);
    let _ = e.accumulate_capture(Some(MODE), tone(0.3, 4096));
    assert!(
        e.dc_blocks_processed() > 0,
        "DC block must run on the accumulate_capture path"
    );
}

#[test]
fn dc_offset_capture_still_decodes() {
    // A large DC offset on the captured audio must not affect the decode — the seam removes it,
    // and the demod is DC-immune regardless. Also confirms the block ran on the receive path.
    let (mut tx, tx_lb) = engine_with_handle();
    let (mut rx, rx_lb) = engine_with_handle();
    let payload = b"dc-block decode probe 0123456789".to_vec();

    tx.transmit(&payload, MODE, None).unwrap();
    let mut samples = tx_lb.drain_samples();
    for s in samples.iter_mut() {
        *s += 0.25; // soundcard-style DC offset on a ~unit-amplitude signal
    }
    rx_lb.fill_samples(&samples);

    let out = rx.receive(MODE, None).unwrap_or_default();
    assert_eq!(out, payload, "DC-offset capture must still decode");
    assert!(rx.dc_blocks_processed() > 0);
}

#[test]
fn pure_dc_does_not_trip_dcd_but_a_real_tone_does() {
    // The point of REQ-PHY-02 for this codebase: a DC offset must not inflate the DCD/CSMA
    // mean-square energy gate. After the seam removes it, a pure-DC burst reads idle...
    let (mut e_dc, lb_dc) = engine_with_handle();
    lb_dc.fill_samples(&vec![0.1f32; 4096]); // pure DC, no AC content
    let _ = e_dc.receive(MODE, None);
    assert!(
        !e_dc.is_channel_busy(),
        "a pure DC offset must not mark the channel busy (energy {})",
        e_dc.dcd_energy()
    );

    // ...while a genuine carrier tone of comparable level still registers as channel energy.
    let (mut e_ac, lb_ac) = engine_with_handle();
    lb_ac.fill_samples(&tone(0.1, 4096));
    let _ = e_ac.receive(MODE, None);
    assert!(
        e_ac.is_channel_busy(),
        "a real carrier tone must register on the DCD (energy {})",
        e_ac.dcd_energy()
    );
}
