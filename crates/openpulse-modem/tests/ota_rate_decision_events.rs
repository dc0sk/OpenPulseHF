//! The OTA rate controller's decisions must be observable, including the ones that change nothing.
//!
//! **The gap this pins (#1081).** OTA level *state* was already observable: the daemon emits a
//! `ControlEvent::OtaStatus` snapshot periodically (~1 Hz while a session is active) and after
//! `apply_ota_ack`. What no event carried was the *decision*:
//!
//! - a **failed decode** that leaves the level where it was is invisible in a state snapshot, by
//!   construction — the snapshot reports state and the state did not move. The controller's whole
//!   failure path was dark.
//! - **which branch fired** — fast-downshift, NACK hysteresis, climb-on-SNR, climb-on-evidence —
//!   was recorded nowhere. That is the field that separates "the controller is working" from "the
//!   SNR estimate is broken and the controller is compensating", which is the #934 failure mode.
//! - the **SNR the decision acted on** was passed to `on_rx_frame` and then discarded.
//!
//! `EngineEvent::OtaRateDecision` carries all three. The daemon already forwards every
//! `EngineEvent` into `ControlEvent::EngineEvent`, so one engine-level event reaches the CLI
//! `monitor` NDJSON, the TUI, the panel, and the on-disk audit `events.ndjson` that 1.0 exit
//! criterion A2 is scored from.

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::ota_rate::RateDecision;
use openpulse_core::profile::SessionProfile;
use openpulse_modem::engine::ModemEngine;
use openpulse_modem::pipeline::AudioSamples;
use openpulse_modem::EngineEvent;

const SESSION: &str = "sess-decision";

fn engine_with_ota_session() -> ModemEngine {
    let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
    engine
        .register_plugin(Box::new(BpskPlugin::new()))
        .expect("register bpsk");
    engine.start_ota_session(SessionProfile::hpx500());
    engine
}

/// Drain the decision events currently queued on a subscriber.
fn drain_decisions(
    rx: &mut tokio::sync::broadcast::Receiver<EngineEvent>,
) -> Vec<(Option<String>, f32, RateDecision)> {
    let mut out = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        if let EngineEvent::OtaRateDecision {
            decoded_level,
            snr_db,
            decision,
            ..
        } = ev
        {
            out.push((decoded_level.map(|l| l.name()), snr_db, decision));
        }
    }
    out
}

/// THE GATE: a burst that does not decode must still emit a decision event.
///
/// This is the case a state snapshot structurally cannot show. If this regresses, an on-air session
/// becomes undebuggable after the fact in exactly the regime that matters — the one where frames are
/// failing.
#[test]
fn a_failed_decode_emits_a_decision_event() {
    let mut engine = engine_with_ota_session();
    let mut rx = engine.subscribe();

    // Noise: no candidate mode can decode it, so the controller takes its failure path.
    let noise: Vec<f32> = (0..8_000)
        .map(|i| ((i as f32 * 12.9898).sin() * 43_758.547).fract() * 0.05)
        .collect();
    let burst = AudioSamples { samples: noise };
    let _ = engine.ota_decode_burst(&burst, SESSION);

    let decisions = drain_decisions(&mut rx);
    assert!(
        !decisions.is_empty(),
        "a failed decode must emit an OtaRateDecision — this is the case the periodic OtaStatus \
         snapshot cannot show, because the level did not change"
    );
    let (decoded_level, _snr, decision) = &decisions[0];
    assert_eq!(
        *decoded_level, None,
        "the burst did not decode, so decoded_level must report that"
    );
    assert!(
        decision.is_failure_path(),
        "a failed decode must report a failure-path branch, got {decision:?}"
    );
}

/// The event must carry the SNR the decision acted on, not a placeholder. Without it the reading
/// that drove the decision is unrecoverable, and a decision cannot be second-guessed after the fact.
#[test]
fn the_decision_event_carries_the_snr_it_acted_on() {
    let mut engine = engine_with_ota_session();
    let mut rx = engine.subscribe();

    let noise: Vec<f32> = (0..8_000)
        .map(|i| ((i as f32 * 7.233).sin() * 21_312.9).fract() * 0.05)
        .collect();
    let burst = AudioSamples { samples: noise };
    let _ = engine.ota_decode_burst(&burst, SESSION);

    let decisions = drain_decisions(&mut rx);
    assert!(!decisions.is_empty(), "expected a decision event");
    assert!(
        decisions.iter().all(|(_, snr, _)| snr.is_finite()),
        "snr_db must be a real reading, not NaN/inf: {decisions:?}"
    );
}

/// Every decision is reported, not only transitions. A controller that emitted solely on a level
/// change would reproduce the original defect one layer up: the frames that failed without moving
/// the level — the interesting ones — would still be missing.
#[test]
fn repeated_failures_each_emit_rather_than_only_the_transition() {
    let mut engine = engine_with_ota_session();
    let mut rx = engine.subscribe();

    let noise: Vec<f32> = (0..8_000)
        .map(|i| ((i as f32 * 3.77).sin() * 9_133.1).fract() * 0.05)
        .collect();
    let burst = AudioSamples { samples: noise };
    for _ in 0..3 {
        let _ = engine.ota_decode_burst(&burst, SESSION);
    }

    let decisions = drain_decisions(&mut rx);
    assert_eq!(
        decisions.len(),
        3,
        "one event per decision, including holds — got {decisions:?}"
    );
}
