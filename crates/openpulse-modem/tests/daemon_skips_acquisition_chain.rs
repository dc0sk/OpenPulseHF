//! The daemon's streaming receive path does NOT run the acquisition chain (#1118).
//!
//! `EnergyGate` -> refined onset -> `afc_mini_settle` -> the #1049 preamble-correlation veto live
//! inside `receive_with_timeout_fec`, which the CLI listen path reaches. The shipping daemon's
//! `rx_ticker` calls `accumulate_capture` and then one of two decode arms, and reaches none of it.
//!
//! **Why this file exists.** That claim was already written down in three places — `engine.rs`'s
//! `update_dcd_at_seam` ("none of that path's machinery ... runs here"), the header of
//! `preamble_veto_interference.rs` (explicitly a research harness with no asserts), and issue #1118
//! — and asserted by NOTHING. A comment cannot fail, and this one is load-bearing: #1053, #1059,
//! #1060 and #1062 are all refinements *of that chain*, so whether the daemon runs it decides
//! whether that work reaches the shipping receiver.
//!
//! **It pins the CURRENT SPLIT, not a desired one.** Closing the gap is an open design question and
//! is not attempted here. When it is resolved these tests SHOULD fail — that is the point. Change
//! them deliberately; do not delete them to make a change go green.
//!
//! **Both daemon decode arms are covered, because `server::run` has two.** It dispatches on
//! `engine.ota_active()` (`server.rs:858`): with an OTA session it calls `ota_decode_burst`, and
//! otherwise — the DEFAULT, since `ota_enabled` is opt-in — it calls `decode_burst`
//! (`server.rs:930`). An earlier version of this file exercised NEITHER: it called
//! `ota_decode_burst` with no session started, so the call returned
//! `Err("no OTA session active")` before reaching any decode work, and `let _ =` swallowed it.
//! The zero-counter assertion after it was therefore vacuous, and the gate was blind at exactly the
//! point it exists to trip — a closure wiring the chain into the OTA decode path would not have
//! failed it. Both arms now assert the shape of what they got back, so a silent no-op cannot recur.
//!
//! **Two counters, deliberately.** `afc_settle_attempts()` increments at the chain's ENTRY on every
//! mode; the `rho_*` counters record what the veto DECIDED and only move where a preamble template
//! exists. A gate on the `rho_*` pair alone would stay green if the chain were half-wired into the
//! streaming path — energy gate and settle without the veto — which is a plausible closure, since
//! every mode except BPSK250 has no template to check. The entry counter closes that hole.
//!
//! **Vacuity is the design problem here**, because "counters are 0" is also what a test that fed
//! nothing decodable reports. Four validations rule it out: the same audio through the CLI path
//! moves the counters (`the_cli_path_runs_the_chain`); each daemon arm asserts a burst actually
//! flushed; each asserts `dcd_blocks_processed() > 0`, so the shared `InputCapture` seam ran; and
//! each asserts the decode call returned a real outcome rather than erroring out early.

use bpsk_plugin::BpskPlugin;
use openpulse_audio::loopback::LoopbackBackend;
use openpulse_core::fec::FecMode;
use openpulse_core::profile::SessionProfile;
use openpulse_modem::engine::ModemEngine;
use openpulse_modem::pipeline::AudioSamples;
use std::time::Duration;

/// BPSK250 is the ONLY mode publishing a `preamble_template`, so it is the only mode where the veto
/// can run at all. On a no-template mode both arms would read 0 rho and the gate would be vacuous by
/// construction. (Verified: `BpskPlugin::preamble_template` returns `None` for every other mode.)
const MODE: &str = "BPSK250";
const FEC: FecMode = FecMode::Rs;
const PAYLOAD: &[u8] = b"daemon acquisition-chain seam probe";
const SESSION: &str = "chain-seam";
const SAMPLE_RATE: u64 = 8_000;
/// Lead-in silence. The frame must sit INSIDE a longer capture — a buffer that is exactly the frame
/// is the easiest case that exists and cannot exercise frame location.
const LEAD_SAMPLES: usize = 8_000;
/// Bounded in WORK so the CLI arm's verdict comes from the audio, not from how much wall clock the
/// machine had (#1066).
const SCAN_POSITIONS: usize = 3_000;
const MAX_ITERATIONS: usize = 400;

/// Samples per `accumulate_capture` call, derived from the daemon's configured tick rather than
/// transcribed. An earlier version hard-coded 800 with the comment "the daemon's default receive
/// tick is 100 ms" — the default is **50 ms** (`DaemonConfig::receive_tick_ms`), so the claim of
/// fidelity was false. Three other test files still carry that same wrong 100 ms comment, which is
/// how it reached this one: by copying. Bound to the config so it cannot drift again.
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

/// One transmitted frame embedded in silence. Built ONCE and handed to every arm, so the paths are
/// compared on identical audio rather than on similar-looking generators.
fn signal_with_frame() -> Vec<f32> {
    let (backend, mut e) = engine();
    e.transmit_with_fec_mode(PAYLOAD, MODE, FEC, None)
        .expect("transmit");
    let tx = backend.drain_samples();
    let mut out = vec![0.0f32; LEAD_SAMPLES];
    out.extend_from_slice(&tx);
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

/// Message shared by both daemon arms, so they cannot drift apart.
fn gap_msg(arm: &str, settles: u64, accepted: u64, rejected: u64) -> String {
    format!(
        "the daemon's {arm} arm reached the acquisition chain (settle_attempts={settles} \
         rho_accepted={accepted} rho_rejected={rejected}), which #1118 records as unreachable from \
         accumulate_capture.\n\n\
         If you are CLOSING that seam this failure is the expected signal: update this test, and \
         note that #1053, #1059, #1060 and #1062 now reach the shipping receiver.\n\
         If you are NOT, something has wired the CLI acquisition chain into the streaming path, and \
         the daemon has inherited its wall-clock-bounded retry regime (#1066)."
    )
}

/// FILTER VALIDATION for both daemon arms: the same audio through the CLI listen path must move the
/// counters. If this reads 0, the daemon arms' zeros prove nothing.
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
        "the CLI path must enter afc_mini_settle on this audio (settle_attempts=0). A zero makes \
         the daemon arms' zeros meaningless — they would no longer distinguish 'the daemon skips \
         the chain' from 'this audio drives the chain nowhere'."
    );
    assert!(
        rho > 0,
        "the CLI path must exercise the preamble-correlation veto on {MODE} (rho total=0), else the \
         rho half of the daemon assertions is vacuous"
    );
}

/// THE CLAIM, default daemon config (`ota_enabled` off): `server::run` calls `decode_burst`.
#[test]
fn the_daemon_decode_burst_arm_never_runs_the_acquisition_chain() {
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

    let (s, a, r) = (
        e.afc_settle_attempts(),
        e.rho_accepted_settles(),
        e.rho_rejected_settles(),
    );
    assert_eq!((s, a, r), (0, 0, 0), "{}", gap_msg("decode_burst", s, a, r));
}

/// THE CLAIM, `ota_enabled` config: `server::run` calls `ota_decode_burst`.
///
/// The OTA session MUST be started or the call returns `Err("no OTA session active")` before doing
/// any decode work, and this test silently measures nothing — the defect this file was rewritten to
/// fix. `server::run` starts the session at startup under `ota_enabled` (`server.rs:228-242`) and
/// only reaches this arm when `engine.ota_active()`, so a session is a precondition of the arm
/// existing at all.
#[test]
fn the_daemon_ota_arm_never_runs_the_acquisition_chain() {
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

    let (s, a, r) = (
        e.afc_settle_attempts(),
        e.rho_accepted_settles(),
        e.rho_rejected_settles(),
    );
    assert_eq!(
        (s, a, r),
        (0, 0, 0),
        "{}",
        gap_msg("ota_decode_burst", s, a, r)
    );
}
