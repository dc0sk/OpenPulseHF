//! A quiet (low-level) station with a small sub-deadband carrier offset must still decode with AGC off.
//!
//! The PSK carrier loops' phase-error magnitude scales with the symbol amplitude, so without level
//! normalisation the loop gain drops with the receive level and the loop cannot acquire even a ~1 Hz
//! residual over a short frame (a residual the AGC would otherwise mask). `normalize_stream_rms` before
//! each loop restores a level-invariant gain. Verified pre-fix: QPSK500 weak+offset failed AGC-off.

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::plugin::ModulationPlugin;
use openpulse_modem::ModemEngine;
use psk8_plugin::Psk8Plugin;
use qpsk_plugin::QpskPlugin;

/// Transmit `payload` on `mode` at 1500 Hz, attenuate by `scale`, receive with a `+offset` Hz carrier
/// mismatch (sub-deadband → never AFC-corrected) and AGC off. Returns whether it decoded.
fn decodes(
    plugin: impl Fn() -> Box<dyn ModulationPlugin>,
    mode: &str,
    scale: f32,
    offset: f32,
    payload: &[u8],
) -> bool {
    let tx_backend = LoopbackBackend::new();
    let tx_handle = tx_backend.clone_shared();
    let mut tx = ModemEngine::new(Box::new(tx_backend));
    tx.register_plugin(plugin()).unwrap();
    tx.set_center_frequency(1500.0);
    tx.transmit(payload, mode, None).unwrap();
    let mut samples = tx_handle.drain_samples();
    for s in &mut samples {
        *s *= scale;
    }

    let rx_backend = LoopbackBackend::new();
    let rx_handle = rx_backend.clone_shared();
    let mut rx = ModemEngine::new(Box::new(rx_backend));
    rx.register_plugin(plugin()).unwrap();
    rx.set_center_frequency(1500.0 + offset); // sub-deadband residual the Costas must track; AGC left off
    rx_handle.fill_samples(&samples);
    let decoded = rx.receive(mode, None).unwrap_or_default();
    decoded.len() >= payload.len() && &decoded[..payload.len()] == payload
}

#[test]
fn quiet_station_with_small_offset_decodes_without_agc() {
    let payload: Vec<u8> = (0u8..64).collect();
    let cases = [
        ("QPSK500", 1.5f32),
        ("8PSK500", 1.0f32),
        ("BPSK250", 1.5f32),
    ];
    for (mode, offset) in cases {
        let make: Box<dyn Fn() -> Box<dyn ModulationPlugin>> = match mode {
            "QPSK500" => Box::new(|| Box::new(QpskPlugin::new()) as Box<dyn ModulationPlugin>),
            "8PSK500" => Box::new(|| Box::new(Psk8Plugin::new()) as Box<dyn ModulationPlugin>),
            _ => Box::new(|| Box::new(BpskPlugin::new()) as Box<dyn ModulationPlugin>),
        };
        // Control: full amplitude decodes (isolates the level coupling, not the offset itself).
        assert!(
            decodes(&make, mode, 1.0, offset, &payload),
            "{mode}: full-amplitude + {offset} Hz offset should decode"
        );
        // The fix: a quiet (×0.05) station with the same offset must also decode, AGC off.
        assert!(
            decodes(&make, mode, 0.05, offset, &payload),
            "{mode}: quiet (×0.05) + {offset} Hz offset must decode with level normalisation"
        );
    }
}
