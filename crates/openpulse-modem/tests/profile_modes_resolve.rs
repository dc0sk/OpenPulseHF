//! Audit H3: every mode string in every shipped `SessionProfile` ladder must resolve to a registered
//! modulation plugin — a typo in an untested rung would otherwise only surface at runtime.

use openpulse_audio::LoopbackBackend;
use openpulse_core::error::ModemError;
use openpulse_core::profile::SessionProfile;
use openpulse_modem::ModemEngine;

fn engine_with_all_plugins() -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(LoopbackBackend::new()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(psk8_plugin::Psk8Plugin::new()))
        .unwrap();
    e.register_plugin(Box::new(qam64_plugin::Qam64Plugin::new()))
        .unwrap();
    e.register_plugin(Box::new(ofdm_plugin::OfdmPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(scfdma_plugin::ScFdmaPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(pilot_plugin::PilotPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(fsk4_plugin::Fsk4Plugin::new()))
        .unwrap();
    e.register_plugin(Box::new(mfsk16_plugin::Mfsk16Plugin::new()))
        .unwrap();
    e
}

/// The names accepted by `SessionProfile::by_name` (the operator-selectable OTA profiles).
const PROFILE_NAMES: &[&str] = &[
    "hpx500",
    "hpx_modcod",
    "hpx_pilot",
    "hpx_pilot_rrc",
    "hpx_pilot_fast",
    "hpx_pilot_fast_rrc",
    "hpx_hf",
    "hpx_ofdm_hf",
    "hpx_wideband",
    "hpx_wideband_hd",
    "hpx_narrowband",
    "hpx_narrowband_hd",
];

#[test]
fn every_profile_mode_string_resolves_to_a_registered_plugin() {
    let mut engine = engine_with_all_plugins();

    for name in PROFILE_NAMES {
        let profile = SessionProfile::by_name(name)
            .unwrap_or_else(|| panic!("by_name should know profile {name}"));

        // The initial level must itself be a defined rung.
        assert!(
            profile.mode_for(profile.initial_level).is_some(),
            "{name}: initial_level {:?} has no mode string",
            profile.initial_level
        );

        for level in profile.defined_levels() {
            let mode = profile
                .mode_for(level)
                .unwrap_or_else(|| panic!("{name}: defined level {level:?} has no mode string"));

            // A registered mode transmits (or fails for some non-registry reason); only
            // PluginNotFound proves the ladder references a mode no plugin provides (a typo).
            if let Err(ModemError::PluginNotFound(m)) = engine.transmit(b"x", mode, None) {
                panic!("{name}: level {level:?} mode {m:?} is not a registered plugin (typo?)");
            }
        }
    }
}
