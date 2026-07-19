use openpulse_core::profile::SessionProfile;
use openpulse_modem::ModemEngine;

use crate::matrix::{TestCase, TestResult, UseCase};

pub mod adaptive;
pub mod ardop;
pub mod b2f;
pub mod kiss;
pub mod raw_modem;

/// Register all modulation plugins on an engine.
pub fn register_all(engine: &mut ModemEngine) {
    use bpsk_plugin::BpskPlugin;
    use fsk4_plugin::Fsk4Plugin;
    use mfsk16_plugin::Mfsk16Plugin;
    use ofdm_plugin::OfdmPlugin;
    use pilot_plugin::PilotPlugin;
    use psk8_plugin::Psk8Plugin;
    use qam64_plugin::Qam64Plugin;
    use qpsk_plugin::QpskPlugin;
    use scfdma_plugin::ScFdmaPlugin;
    engine
        .register_plugin(Box::new(BpskPlugin::new()))
        .expect("register BpskPlugin");
    engine
        .register_plugin(Box::new(QpskPlugin::new()))
        .expect("register QpskPlugin");
    engine
        .register_plugin(Box::new(Psk8Plugin::new()))
        .expect("register Psk8Plugin");
    engine
        .register_plugin(Box::new(Qam64Plugin::new()))
        .expect("register Qam64Plugin");
    engine
        .register_plugin(Box::new(Fsk4Plugin::new()))
        .expect("register Fsk4Plugin");
    engine
        .register_plugin(Box::new(OfdmPlugin::new()))
        .expect("register OfdmPlugin");
    engine
        .register_plugin(Box::new(ScFdmaPlugin::new()))
        .expect("register ScFdmaPlugin");
    engine
        .register_plugin(Box::new(PilotPlugin::new()))
        .expect("register PilotPlugin");
    // hpx_hf's SL1 sub-floor rung. Omitting it silently produced fade reports for a ladder
    // missing its weakest rung (audit 2026-07-19, #13).
    engine
        .register_plugin(Box::new(Mfsk16Plugin::new()))
        .expect("register Mfsk16Plugin");
}

pub fn run_case(case: &TestCase) -> TestResult {
    match &case.use_case {
        UseCase::RawModem => raw_modem::run(case),
        UseCase::AdaptiveHpx500 => adaptive::run(case, SessionProfile::hpx500()),
        UseCase::AdaptiveHpxHf => adaptive::run(case, SessionProfile::hpx_hf()),
        UseCase::AdaptiveHpxWideband => adaptive::run(case, SessionProfile::hpx_wideband()),
        UseCase::AdaptiveHpxOfdmHf => adaptive::run(case, SessionProfile::hpx_ofdm_hf()),
        UseCase::Ardop => ardop::run(case),
        UseCase::Kiss => kiss::run(case),
        UseCase::B2f => b2f::run(case),
    }
}
