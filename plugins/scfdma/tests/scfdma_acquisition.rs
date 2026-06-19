//! SC-FDMA acquisition test.
//!
//! Verifies that the demodulator can recover a payload when the received
//! buffer is preceded by arbitrary leading samples, using the transmitted sync
//! preamble to locate the payload start.

use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use scfdma_plugin::ScFdmaPlugin;

fn mod_config(mode: &str) -> ModulationConfig {
    ModulationConfig {
        mode: mode.to_string(),
        center_frequency: 1500.0,
        sample_rate: 8000,
        ..ModulationConfig::default()
    }
}

#[test]
fn scfdma52_acquires_payload_after_leading_offset() {
    let plugin = ScFdmaPlugin::new();
    let payload = b"SC-FDMA acquisition payload";
    let samples = plugin.modulate(payload, &mod_config("SCFDMA52")).unwrap();

    // Prefix a non-symbol-aligned amount of garbage to force acquisition.
    let mut shifted = vec![0.0f32; 97];
    shifted.extend_from_slice(&samples);

    let rx = plugin
        .demodulate(&shifted, &mod_config("SCFDMA52"))
        .unwrap();
    assert_eq!(rx.as_slice(), payload.as_ref());
}
