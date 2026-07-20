//! A mode must not advertise a soft-LLR capability it refuses at call time.
//!
//! `qpsk_demodulate_soft` errors on the differential (`-D`) modes by deliberate design (#923): a
//! differential detector has no calibrated coherent LLR, and emitting miscalibrated ones would be
//! worse than refusing. But `supports_soft_demod()` returned `true` for the whole plugin, so the
//! engine selected the soft path for `QPSK250-D` and only discovered the refusal at demodulation.
//!
//! On the dual-card rig that surfaced as `QPSK250-D` + `ldpc` failing with
//! `differential QPSK has no soft-LLR path` — correct behaviour, badly surfaced. The advertisement
//! was the bug, not the refusal.
//!
//! The invariant this pins is general: **`supports_soft_demod(mode)` and `demodulate_soft(mode)`
//! must agree**, for every mode the plugin claims.

use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use qpsk_plugin::QpskPlugin;

fn config(mode: &str) -> ModulationConfig {
    ModulationConfig {
        mode: mode.to_string(),
        center_frequency: 1500.0,
        sample_rate: 8000,
        ..ModulationConfig::default()
    }
}

/// THE GATE: the advertisement matches reality for every mode the plugin offers.
///
/// Before the fix, `QPSK250-D` and `QPSK500-D` advertised `true` and then errored.
#[test]
fn every_mode_advertises_the_soft_capability_it_actually_has() {
    let plugin = QpskPlugin::new();
    let payload: Vec<u8> = (0..32u8).collect();

    let mut checked = 0;
    let modes = plugin.info().supported_modes.clone();
    for mode in &modes {
        let cfg = config(mode);
        let Ok(tx) = plugin.modulate(&payload, &cfg) else {
            continue; // a mode this build cannot modulate proves nothing either way
        };
        let advertised = plugin.supports_soft_demod(mode);
        let actual = plugin.demodulate_soft(&tx, &cfg).is_ok();

        assert_eq!(
            advertised, actual,
            "{mode}: supports_soft_demod() says {advertised} but demodulate_soft() {} — a mode must \
             not advertise a capability it refuses at call time",
            if actual { "succeeded" } else { "errored" }
        );
        checked += 1;
    }

    assert!(
        checked >= 4,
        "only {checked} modes were exercised — the loop is not covering the plugin, so this gate \
         would pass vacuously"
    );
}

/// The specific case from the hardware rig, pinned by name so a regression is unambiguous.
#[test]
fn differential_modes_do_not_advertise_soft_demod() {
    let plugin = QpskPlugin::new();
    for mode in ["QPSK250-D", "QPSK500-D"] {
        assert!(
            !plugin.supports_soft_demod(mode),
            "{mode} is differential and has no soft-LLR path, so it must not advertise one"
        );
    }
}

/// Control: the coherent modes still advertise soft demod, so the fix did not disable the soft path
/// wholesale — which would silently cost every soft-FEC mode its iteration gain.
#[test]
fn coherent_modes_still_advertise_soft_demod() {
    let plugin = QpskPlugin::new();
    for mode in ["QPSK250", "QPSK500", "QPSK1000"] {
        assert!(
            plugin.supports_soft_demod(mode),
            "{mode} is coherent and soft-capable; it must keep advertising soft demod"
        );
    }
}
