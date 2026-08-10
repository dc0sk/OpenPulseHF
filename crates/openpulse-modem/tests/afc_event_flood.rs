//! A multi-attempt scan must not narrate its discarded hypotheses to the event stream.
//!
//! Every scanning loop — `decode_burst_inner`, the OTA candidate/HARQ loops, and
//! `receive_with_timeout_fec`'s retry loop — restores `afc_correction_hz` after a failed attempt.
//! An `AfcUpdate` emitted from inside one of those attempts therefore reports a correction the
//! engine immediately throws away. It is not state, and at scan volume it is destructive: a BPSK250
//! noise scan makes ~129 attempts against a 64-slot broadcast ring, so the ring overflows and
//! EVICTS genuine events. That is how a failed OTA burst lost its own `OtaRateDecision` — the event
//! #1081 exists to guarantee.
//!
//! The rule under test is **commit-gating, not failure-gating**: a single-window receive keeps its
//! estimate even when the decode fails, so those emissions are real state and must keep flowing
//! (that is what the TUI's AFC meter renders during acquisition). Only rolled-back attempts are
//! silent.
//!
//! The assertion is the semantic property itself — ZERO events from a fully-rolled-back scan — not
//! a tuned events-per-burst bound that would drift with the scan geometry.

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::engine::ModemEngine;
use openpulse_modem::pipeline::AudioSamples;
use openpulse_modem::EngineEvent;

const MODE: &str = "BPSK250";
const SESSION: &str = "flood";

fn engine() -> (ModemEngine, LoopbackBackend) {
    let backend = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(backend.clone_shared()));
    e.register_plugin(Box::new(BpskPlugin::new()))
        .expect("register");
    (e, backend)
}

/// Noise long enough to make the onset scan run its full course.
fn noise(n: usize) -> AudioSamples {
    AudioSamples {
        samples: (0..n)
            .map(|i| ((i as f32 * 12.9898).sin() * 43_758.547).fract() * 0.05)
            .collect(),
    }
}

/// Drain every event, distinguishing a `Lagged` (ring overflow) from a clean empty channel.
/// A `while let Ok(..)` loop cannot: it stops on `Lagged` and looks like "no more events".
fn drain(rx: &mut tokio::sync::broadcast::Receiver<EngineEvent>) -> (Vec<EngineEvent>, bool) {
    let mut out = Vec::new();
    let mut lagged = false;
    loop {
        match rx.try_recv() {
            Ok(ev) => out.push(ev),
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => lagged = true,
            Err(_) => break,
        }
    }
    (out, lagged)
}

fn afc_count(evs: &[EngineEvent]) -> usize {
    evs.iter()
        .filter(|e| matches!(e, EngineEvent::AfcUpdate { .. }))
        .count()
}

/// Arm 1 — the non-OTA daemon arm (`decode_burst`).
#[test]
fn a_failed_decode_burst_scan_emits_no_afc_events() {
    let (mut e, _b) = engine();
    let mut rx = e.subscribe();
    assert!(e.decode_burst(MODE, &noise(8_000)).is_err(), "noise must not decode");
    let (evs, lagged) = drain(&mut rx);
    assert!(!lagged, "the scan overflowed the event ring");
    assert_eq!(
        afc_count(&evs),
        0,
        "a fully rolled-back scan must emit no AfcUpdate; got {:#?}",
        evs
    );
}

/// Arm 2 — the OTA arm (candidate loop + uncoded fallback + HARQ), and the #1081 survival property.
#[test]
fn a_failed_ota_burst_emits_no_afc_events_and_keeps_its_rate_decision() {
    let (mut e, _b) = engine();
    e.start_ota_session(SessionProfile::hpx_hf());
    e.ota_lock_level(SpeedLevel::Sl5);
    let mut rx = e.subscribe();

    let res = e
        .ota_decode_burst(&noise(8_000), SESSION, Some(MODE))
        .expect("must not error");
    assert!(res.payload.is_none(), "noise must not decode");

    let (evs, lagged) = drain(&mut rx);
    assert!(!lagged, "the scan overflowed the event ring");
    assert_eq!(afc_count(&evs), 0, "rolled-back attempts must be silent");
    assert!(
        evs.iter()
            .any(|e| matches!(e, EngineEvent::OtaRateDecision { .. })),
        "the failed burst's OtaRateDecision must survive the scan (#1081); got {evs:#?}"
    );
}

/// Arm 3 — the CLI scanning receiver (`receive_with_timeout_fec`).
#[test]
fn a_failed_timeout_receive_scan_emits_no_afc_events() {
    let (mut e, backend) = engine();
    backend.fill_samples(&noise(8_000).samples);
    let mut rx = e.subscribe();
    let _ = e.receive_with_timeout(MODE, None, std::time::Duration::from_millis(300));
    let (evs, lagged) = drain(&mut rx);
    assert!(!lagged, "the scan overflowed the event ring");
    assert_eq!(afc_count(&evs), 0, "rolled-back attempts must be silent");
}

/// Vacuity control: suppression must not be a blanket mute.
///
/// Without this, an implementation that silenced `AfcUpdate` unconditionally would pass all three
/// gates above — the failure mode a zero-assertion invites.
#[test]
fn a_successful_scan_still_emits_exactly_one_afc_update() {
    let (mut tx, tx_backend) = engine();
    tx.transmit(b"committed afc", MODE, None).expect("transmit");
    let frame = AudioSamples {
        samples: tx_backend.drain_samples(),
    };

    let (mut e, _b) = engine();
    let mut rx = e.subscribe();
    let got = e.decode_burst(MODE, &frame).expect("the frame must decode");
    assert_eq!(&got[..b"committed afc".len()], b"committed afc");

    let (evs, lagged) = drain(&mut rx);
    assert!(!lagged, "a successful decode must not overflow the ring");
    assert_eq!(
        afc_count(&evs),
        1,
        "a scan that COMMITS its correction must report it, exactly once; got {evs:#?}"
    );
}
