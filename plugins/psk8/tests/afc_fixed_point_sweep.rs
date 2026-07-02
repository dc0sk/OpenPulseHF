//! Diagnostic (ignored): map `afc_estimate_hz` vs applied CFO for 8PSK1000 to find spurious
//! fixed points (residuals where the estimate crosses ~0, which the engine settle locks onto).
//!   cargo test -p psk8-plugin --test afc_fixed_point_sweep -- --ignored --nocapture

use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use psk8_plugin::demodulate::afc_estimate_hz;
use psk8_plugin::Psk8Plugin;

fn cfg(mode: &str, center: f32) -> ModulationConfig {
    ModulationConfig {
        mode: mode.into(),
        center_frequency: center,
        sample_rate: 8000,
        ..Default::default()
    }
}

#[test]
#[ignore = "diagnostic"]
fn sweep() {
    let plugin = Psk8Plugin::new();
    let payload = b"afc fixed-point sweep 8psk1000 preamble probe payload data";
    for mode in ["8PSK1000", "8PSK500"] {
        println!("== {mode} : applied_cfo -> afc_estimate_hz ==");
        for r in (-60..=60).step_by(5) {
            let r = r as f32;
            // Modulate at centre 1500+r, estimate with nominal centre 1500 → estimate should read r.
            let audio = plugin.modulate(payload, &cfg(mode, 1500.0 + r)).unwrap();
            let est = afc_estimate_hz(&audio, &cfg(mode, 1500.0)).unwrap_or(f32::NAN);
            println!("  cfo={r:>6.0}  est={est:>7.1}  err={:>7.1}", est - r);
        }
    }
}
