//! Gate 7 (`docs/dev/design/ofdm-preamble-detect.md`): the carrier detector's template IS the
//! transmitted preamble — taken by reference from the transmitter's own function, never re-typed.

use ofdm_plugin::params::SYM_LEN;
use ofdm_plugin::OfdmPlugin;
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};

fn cfg(mode: &str) -> ModulationConfig {
    ModulationConfig {
        mode: mode.into(),
        ..ModulationConfig::default()
    }
}

/// Normalised dot product.
fn ndot(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let ea: f32 = a.iter().map(|x| x * x).sum();
    let eb: f32 = b.iter().map(|x| x * x).sum();
    dot / (ea * eb).sqrt()
}

#[test]
fn the_detector_template_is_the_transmitted_preamble() {
    let plugin = OfdmPlugin::new();
    for (mode, payload) in [
        ("OFDM52", vec![0x00u8; 1]),
        ("OFDM52", (0..200u8).collect::<Vec<_>>()),
        ("OFDM52-64QAM", vec![0xA5u8; 37]),
        ("OFDM52-64QAM", (0..255u8).rev().collect::<Vec<_>>()),
    ] {
        let det = plugin
            .carrier_detector(&cfg(mode))
            .unwrap_or_else(|| panic!("{mode} publishes a detector"));
        let tx = plugin.modulate(&payload, &cfg(mode)).expect("modulate");
        let rho = ndot(det.template(), &tx[..SYM_LEN]);
        assert_eq!(det.template().len(), SYM_LEN, "{mode}");
        assert!(
            rho > 0.99995,
            "{mode}, {} B: the detector's template has ρ {rho:.6} against the transmitted \
             preamble; it must be the transmitter's own clipped preamble",
            payload.len()
        );
    }
    assert!(
        plugin.carrier_detector(&cfg("OFDM16")).is_none(),
        "OFDM16's preamble is unmeasured and must publish no detector"
    );
}
