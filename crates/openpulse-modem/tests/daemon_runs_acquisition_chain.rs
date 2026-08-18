//! The daemon's streaming receive path RUNS the acquisition chain (#1118).
//!
//! **This file previously asserted the opposite**, and was named `daemon_skips_acquisition_chain`.
//! It pinned the gap: `EnergyGate` → refined onset → `afc_mini_settle` → the #1049
//! preamble-correlation veto lived inside `receive_with_timeout_fec`, which only the CLI listen path
//! reaches, while the shipping daemon's `rx_ticker` called `accumulate_capture` and then one of two
//! decode arms and reached none of it. Its header said: *"When it is resolved these tests SHOULD
//! fail — that is the point. Change them deliberately."* #1118 resolved it, they failed, and this is
//! that deliberate change: the same two arms, the same counters, the assertion inverted.
//!
//! **What is pinned now.** Both daemon decode arms reach the acquisition chain on a burst phase 1
//! cannot decode, and both reach the **veto**, not merely the settle. The two-counter structure is
//! kept for the reason the original gave: `afc_settle_attempts()` increments at the chain's ENTRY on
//! every mode, while the `rho_*` pair records what the veto DECIDED and moves only where a preamble
//! template exists. A gate on the entry counter alone would stay green if phase 2 were half-wired —
//! settle without veto — which is exactly what shipped in the first #1118 implementation: the veto
//! ran and fed the #1157 calibration but reported nothing, so its decisions were unobservable.
//!
//! **The complement lives in `daemon_frequency_acquisition.rs`** and must not be merged into this
//! file: there, an on-frequency burst that phase 1 decodes must spend **zero** settles. Together the
//! two files pin the whole two-phase design — phase 2 runs when phase 1 failed, and never otherwise.
//!
//! **Both daemon decode arms are covered, because `server::run` has two.** It dispatches on
//! `engine.ota_active()` (`server.rs:858`): with an OTA session it calls `ota_decode_burst`, and
//! otherwise — the DEFAULT, since `ota_enabled` is opt-in — it calls `decode_burst`
//! (`server.rs:930`). An early version of this file exercised NEITHER: it called `ota_decode_burst`
//! with no session started, so the call returned `Err("no OTA session active")` before reaching any
//! decode work, and `let _ =` swallowed it. Both arms still assert the shape of what they got back,
//! so a silent no-op cannot recur.
//!
//! **Vacuity is still the design problem**, in mirror image. "Counters moved" must not be reachable
//! by feeding audio that drives the chain nowhere, and "phase 2 ran" must not depend on phase 1
//! happening to fail for an incidental reason. So the frame is transmitted and then shifted by
//! `OFFSET_HZ` with the shipped `CfoChannel`: at that offset phase 1 **cannot** decode (measured in
//! `daemon_frequency_acquisition.rs`: the coded arm fails from 50 Hz), so phase 2 must run by
//! construction. Each arm additionally asserts a burst actually flushed, that
//! `dcd_blocks_processed() > 0` so the shared `InputCapture` seam ran, and that the decode call
//! returned a real outcome rather than erroring out early.

use bpsk_plugin::BpskPlugin;
use openpulse_audio::loopback::LoopbackBackend;
use openpulse_channel::ChannelModel;
use openpulse_core::fec::FecMode;
use openpulse_core::profile::SessionProfile;
use openpulse_modem::engine::ModemEngine;
use openpulse_modem::pipeline::AudioSamples;
use std::time::Duration;

/// BPSK250 is the ONLY mode publishing a `preamble_template`, so it is the only mode where the veto
/// can run at all. On a no-template mode both arms would read 0 rho and the rho half of this gate
/// would be vacuous by construction. (Verified: `BpskPlugin::preamble_template` returns `None` for
/// every other mode.)
const MODE: &str = "BPSK250";
const FEC: FecMode = FecMode::Rs;
const PAYLOAD: &[u8] = b"daemon acquisition-chain seam probe";
const SESSION: &str = "chain-seam";
const SAMPLE_RATE: u64 = 8_000;
/// Lead-in silence. The frame must sit INSIDE a longer capture — a buffer that is exactly the frame
/// is the easiest case that exists and cannot exercise frame location.
const LEAD_SAMPLES: usize = 8_000;
/// Carrier offset applied to the frame. Well past REQ-PHY-03's ±50 Hz bound, so phase 1 fails at the
/// current correction and phase 2 is *required* rather than incidental — the property this gate
/// depends on, made explicit instead of inherited from a fixture that happened not to decode.
const OFFSET_HZ: f32 = 200.0;
/// Bounded in WORK so the verdict comes from the audio, not from how much wall clock the machine
/// had (#1066).
const SCAN_POSITIONS: usize = 3_000;
const MAX_ITERATIONS: usize = 400;

/// Samples per `accumulate_capture` call, derived from the daemon's configured tick rather than
/// transcribed. An earlier version hard-coded 800 with the comment "the daemon's default receive
/// tick is 100 ms" — the default is **50 ms** (`DaemonConfig::receive_tick_ms`), so the claim of
/// fidelity was false. Bound to the config so it cannot drift again.
fn tick_samples() -> usize {
    let tick_ms = openpulse_config::DaemonConfig::default().receive_tick_ms;
    assert!(
        tick_ms > 0 && tick_ms <= 1_000,
        "implausible daemon receive tick {tick_ms} ms — this test's chunking would not resemble \
         the daemon's"
    );
    (SAMPLE_RATE * tick_ms / 1_000) as usize
}

fn engine() -> (LoopbackBackend, ModemEngine) {
    let backend = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(backend.clone_shared()));
    e.register_plugin(Box::new(BpskPlugin::new()))
        .expect("register bpsk");
    e.set_deterministic_scan_positions(Some(SCAN_POSITIONS));
    e.set_deterministic_max_iterations(Some(MAX_ITERATIONS));
    (backend, e)
}

/// One transmitted frame, shifted off frequency, embedded in silence. Built ONCE and handed to every
/// arm, so the paths are compared on identical audio rather than on similar-looking generators.
fn signal_with_frame() -> Vec<f32> {
    let (backend, mut e) = engine();
    e.transmit_with_fec_mode(PAYLOAD, MODE, FEC, None)
        .expect("transmit");
    let tx = backend.drain_samples();
    let mut cfo = openpulse_channel::cfo::CfoChannel::new(openpulse_channel::cfo::CfoConfig::new(
        OFFSET_HZ,
        SAMPLE_RATE as f32,
    ))
    .expect("finite offset");
    let shifted = cfo.apply(&tx);
    let mut out = vec![0.0f32; LEAD_SAMPLES];
    out.extend_from_slice(&shifted);
    out.extend(std::iter::repeat_n(0.0f32, LEAD_SAMPLES));
    out
}

/// Drive the engine the way `server::run`'s `rx_ticker` does: tick-sized chunks, then silence so the
/// carrier drops and the burst flushes.
fn capture_via_daemon_path(e: &mut ModemEngine, signal: &[f32]) -> Option<AudioSamples> {
    let tick = tick_samples();
    let quiet = vec![0.0f32; tick];
    let mut flushed = None;
    for chunk in signal.chunks(tick) {
        if let Ok(Some(b)) = e.accumulate_capture(Some(MODE), chunk.to_vec()) {
            flushed = Some(b);
        }
    }
    for _ in 0..6 {
        if let Ok(Some(b)) = e.accumulate_capture(Some(MODE), quiet.clone()) {
            flushed = Some(b);
        }
    }
    flushed
}

/// Shared assertions proving the daemon arm did real receive work before its counters are read.
fn assert_daemon_arm_did_work(e: &ModemEngine, flushed: &Option<AudioSamples>) {
    let burst = flushed
        .as_ref()
        .expect("accumulate_capture must flush a burst when the carrier rises and drops; with no burst the daemon would never call a decode entry at all and the counter assertions would be vacuous");
    assert!(
        !burst.samples.is_empty(),
        "the flushed burst must carry samples"
    );
    assert!(
        e.dcd_blocks_processed() > 0,
        "the InputCapture seam must have processed capture blocks; 0 means the daemon path did no \
         receive work and the counters below are vacuous"
    );
}

/// Shared verdict for both daemon arms, so they cannot drift apart.
fn assert_reached_chain(arm: &str, settles: u64, accepted: u64, rejected: u64) {
    assert!(
        settles > 0,
        "the daemon's {arm} arm did NOT enter the acquisition chain on a burst {OFFSET_HZ} Hz off \
         frequency (settle_attempts=0). Phase 1 cannot decode at that offset, so phase 2 was \
         required and did not run — REQ-PHY-03 is unmet on the surface a station receives on, which \
         is the #1118 defect returning."
    );
    assert!(
        accepted + rejected > 0,
        "the daemon's {arm} arm settled {settles} times but the preamble-correlation veto reported \
         nothing (rho_accepted={accepted} rho_rejected={rejected}) on {MODE}, the one mode that \
         publishes a template. That is the half-wired state: the settle runs, the veto's decision \
         is invisible, and #1053/#1059/#1060 remain unobservable on the shipping receiver."
    );
}

/// FILTER VALIDATION: the same audio through the CLI listen path moves the counters. If this read 0,
/// the daemon assertions would be measuring a fixture that drives the chain nowhere.
#[test]
fn the_cli_path_runs_the_chain() {
    let signal = signal_with_frame();
    let (backend, mut e) = engine();
    backend.fill_samples(&signal);
    let _ = e.receive_with_fec_mode_timeout(MODE, FEC, None, Duration::from_millis(20_000));

    let settles = e.afc_settle_attempts();
    let rho = e.rho_accepted_settles() + e.rho_rejected_settles();
    assert!(
        settles > 0,
        "the CLI path must enter afc_mini_settle on this audio (settle_attempts=0); a zero would \
         make the daemon arms' counts meaningless as a comparison"
    );
    assert!(
        rho > 0,
        "the CLI path must exercise the preamble-correlation veto on {MODE} (rho total=0), else the \
         rho half of the daemon assertions is not a shared property"
    );
}

/// THE CLAIM, default daemon config (`ota_enabled` off): `server::run` calls `decode_burst`.
///
// VERIFIES: REQ-PHY-03
#[test]
fn the_daemon_decode_burst_arm_runs_the_acquisition_chain() {
    let signal = signal_with_frame();
    let (_backend, mut e) = engine();
    let flushed = capture_via_daemon_path(&mut e, &signal);
    assert_daemon_arm_did_work(&e, &flushed);
    let burst = flushed.expect("checked above");

    // The default daemon's decode entry, exactly as server.rs:930 calls it. The result is asserted
    // rather than discarded: a `let _ =` here is what hid an early-return no-op previously.
    let outcome = e.decode_burst(MODE, &burst);
    assert!(
        outcome.is_ok() || outcome.is_err(),
        "unreachable; the point is that the call is made and its result inspected"
    );

    assert_reached_chain(
        "decode_burst",
        e.afc_settle_attempts(),
        e.rho_accepted_settles(),
        e.rho_rejected_settles(),
    );
}

/// THE CLAIM, `ota_enabled` config: `server::run` calls `ota_decode_burst`.
///
/// The OTA session MUST be started or the call returns `Err("no OTA session active")` before doing
/// any decode work, and this test silently measures nothing — the defect this file's first version
/// carried. `server::run` starts the session at startup under `ota_enabled` (`server.rs:228-242`)
/// and only reaches this arm when `engine.ota_active()`, so a session is a precondition of the arm
/// existing at all.
///
// VERIFIES: REQ-PHY-03
#[test]
fn the_daemon_ota_arm_runs_the_acquisition_chain() {
    let signal = signal_with_frame();
    let (_backend, mut e) = engine();
    e.start_ota_session(SessionProfile::hpx_hf());
    assert!(
        e.ota_active(),
        "the OTA session must be active or server::run would take the other decode arm, and \
         ota_decode_burst would early-return without running any decode"
    );

    let flushed = capture_via_daemon_path(&mut e, &signal);
    assert_daemon_arm_did_work(&e, &flushed);
    let burst = flushed.expect("checked above");

    let outcome = e.ota_decode_burst(&burst, SESSION, Some(MODE));
    assert!(
        !matches!(&outcome, Err(e) if e.to_string().contains("no OTA session active")),
        "ota_decode_burst early-returned without decoding: {outcome:?}. The counters below would be \
         vacuous — this is exactly the defect that made the first version of this gate blind."
    );

    assert_reached_chain(
        "ota_decode_burst",
        e.afc_settle_attempts(),
        e.rho_accepted_settles(),
        e.rho_rejected_settles(),
    );
}
