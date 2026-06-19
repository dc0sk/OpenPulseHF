//! Passband loopback for the pilot-framed QPSK plugin: modulate → (channel) →
//! demodulate, validating the full audio chain plus pilot-aided recovery.

use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use pilot_plugin::PilotPlugin;

fn cfg(center: f32) -> ModulationConfig {
    ModulationConfig {
        center_frequency: center,
        sample_rate: 8000,
        mode: "PILOT-QPSK500".to_string(),
        ..Default::default()
    }
}

const PAYLOAD: &[u8] = b"pilot-framed QPSK passband loopback 0123456789 abcdefghij KN4xyz";

#[test]
fn clean_loopback_with_leadin() {
    let plugin = PilotPlugin::new();
    let audio = plugin.modulate(PAYLOAD, &cfg(1500.0)).unwrap();
    assert!(!audio.is_empty());

    // Prepend lead-in silence so the demodulator must locate the onset.
    let mut buf = vec![0.0f32; 640];
    buf.extend_from_slice(&audio);

    let out = plugin.demodulate(&buf, &cfg(1500.0)).unwrap();
    assert!(
        out.len() >= PAYLOAD.len() && &out[..PAYLOAD.len()] == PAYLOAD,
        "clean loopback must recover the payload (got {} bytes)",
        out.len()
    );
}

#[test]
fn loopback_through_carrier_offset() {
    // TX carrier 2 Hz above the RX's nominal: the downconverter leaves a residual
    // that the symbol-level pilot tracker removes. The offset that the *passband
    // POC* tolerates is bounded here by onset precision, not by the tracker: with
    // rectangular pulses and integer-sample onset (no timing recovery yet) a
    // larger offset shifts the coherent preamble-correlation peak by a whole
    // sample, straddling symbol boundaries. Sub-sample timing recovery (Gardner)
    // plus a coarse-CFO stage — and, in normal use, the engine's AFC chain — lift
    // this in the integration step; the symbol-level codec itself already tracks
    // far larger offsets (see frame.rs `round_trip_through_carrier_frequency_offset`).
    let plugin = PilotPlugin::new();
    let audio = plugin.modulate(PAYLOAD, &cfg(1502.0)).unwrap();
    let out = plugin.demodulate(&audio, &cfg(1500.0)).unwrap();
    assert!(
        out.len() >= PAYLOAD.len() && &out[..PAYLOAD.len()] == PAYLOAD,
        "pilot tracking must recover the payload through a carrier offset (got {} bytes)",
        out.len()
    );
}
