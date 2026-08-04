//! Which modes have a preamble-correlation veto is pinned, not just how the current ones behave.
//!
//! The #1049 veto is gated by two things of very different kinds: whether the plugin publishes a
//! template (a correctness choice — the ρ threshold and grid must have been *derived for that
//! mode*), and whether the template fits the engine's correlation budget (a cost limit).
//!
//! Those were entangled, and the entanglement was invisible. `bpsk` published the same constants
//! for every BPSK mode, and `MAX_PREAMBLE_CORRELATION_SAMPLES` discarded the slow rungs before the
//! engine could use them — so a **cost** limit was enforcing a **correctness** property as a side
//! effect. Turning that cap into a post-decimation budget (phase 0 of #1062) removed the accident
//! and would have silently activated the veto on BPSK31/63/100 with BPSK250's threshold, and with a
//! grid that reaches their own first spectral line: measured ρ 0.636–0.701 against a 0.40 threshold
//! for a steady tone at any frequency in the first-order band.
//!
//! Every gate at the time was green, because they all exercise BPSK250.
//!
//! **The general rule this encodes: when a capability is gated by a resource limit, pin the
//! membership set, not only the behaviour of current members.** A limit change grows the set
//! silently, and each new member arrives wearing constants that were derived for someone else.

use openpulse_core::plugin::ModulationPlugin;
use openpulse_modem::engine::ModemEngine;
use std::collections::BTreeSet;

/// The single-carrier PSK family — the modes this veto could apply to at all.
fn plugins() -> Vec<Box<dyn ModulationPlugin>> {
    vec![
        Box::new(bpsk_plugin::BpskPlugin::new()),
        Box::new(qpsk_plugin::QpskPlugin::new()),
        Box::new(psk8_plugin::Psk8Plugin::new()),
    ]
}

fn engine_with_all_plugins() -> ModemEngine {
    let backend = openpulse_audio::loopback::LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(backend.clone_shared()));
    for p in plugins() {
        e.register_plugin(p).expect("register");
    }
    e
}

/// Every mode the registered plugins claim, read from the plugins themselves so the sweep cannot
/// miss one by being written down separately.
fn all_modes() -> BTreeSet<String> {
    plugins()
        .iter()
        .flat_map(|p| p.info().supported_modes.iter().cloned())
        .collect()
}

/// Modes that currently have an active veto. Changing this set is a deliberate act.
///
/// To add one: derive **its own** ρ threshold (noise column across receive bandwidths, decode
/// column on the channel that rung exists for) and **its own** grid half-width (bounded above by
/// `baud/4`, below by the settle residual, measured at ≤ 0.3 Hz), publish them from the plugin,
/// then list the mode here. Adding it here alone does nothing; adding it in the plugin alone fails
/// this test, which is the point.
const MODES_WITH_VETO: [&str; 1] = ["BPSK250"];

#[test]
fn only_the_pinned_modes_have_a_preamble_veto() {
    let e = engine_with_all_plugins();

    let all_modes = all_modes();
    assert!(
        all_modes.len() > 10,
        "only {} modes discovered — the sweep is not covering the registered plugins, so a mode \
         could gain a veto without this test noticing",
        all_modes.len()
    );

    let active: BTreeSet<&str> = all_modes
        .iter()
        .filter(|m| e.preamble_veto_active(m))
        .map(|m| m.as_str())
        .collect();
    let expected: BTreeSet<&str> = MODES_WITH_VETO.into_iter().collect();

    assert_eq!(
        active, expected,
        "the set of modes with an active preamble veto has changed.\n\
         If this grew: a mode is now correlating with constants that may have been derived for a \
         different template — ρ is normalised, so a threshold and grid measured on one preamble do \
         not transfer to another, and the failure is silent (a veto that corroborates noise, not \
         an error).\n\
         If this shrank: a mode lost its veto and fell back to the energy-only settle.\n\
         Either way, update MODES_WITH_VETO deliberately and say why."
    );
}

/// The pin above is only meaningful if the veto it counts is real.
#[test]
fn the_pinned_mode_actually_builds_a_veto() {
    let e = engine_with_all_plugins();
    assert!(
        e.preamble_veto_active("BPSK250"),
        "BPSK250 has no active veto, so the membership pin is counting an empty set and would \
         pass no matter what the other modes did"
    );
}
