use openpulse_core::profile::SessionProfile;
use openpulse_modem::ModemEngine;

use crate::matrix::{TestCase, TestResult, UseCase};

pub mod adaptive;
pub mod ardop;
pub mod b2f;
pub mod kiss;
pub mod raw_modem;

/// Register all plugins on an engine.
pub fn register_all(engine: &mut ModemEngine) {
    use bpsk_plugin::BpskPlugin;
    use fsk4_plugin::Fsk4Plugin;
    use psk8_plugin::Psk8Plugin;
    use qpsk_plugin::QpskPlugin;
    let _ = engine.register_plugin(Box::new(BpskPlugin::new()));
    let _ = engine.register_plugin(Box::new(QpskPlugin::new()));
    let _ = engine.register_plugin(Box::new(Psk8Plugin::new()));
    let _ = engine.register_plugin(Box::new(Fsk4Plugin::new()));
}

pub fn run_case(case: &TestCase) -> TestResult {
    match &case.use_case {
        UseCase::RawModem => raw_modem::run(case),
        UseCase::AdaptiveHpx500 => adaptive::run(case, SessionProfile::hpx500()),
        UseCase::AdaptiveHpx2300 => adaptive::run(case, SessionProfile::hpx2300()),
        UseCase::Ardop => ardop::run(case),
        UseCase::Kiss => kiss::run(case),
        UseCase::B2f => b2f::run(case),
    }
}
