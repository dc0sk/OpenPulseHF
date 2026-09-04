//! The RX burst cap must cover what the peer may actually SEND, not just the mode this station is
//! configured with (#1249).
//!
//! `accumulate_routed` sizes its runaway cap from a mode. The daemon passes its *configured* mode
//! (`server.rs` rx ticker → `accumulate_capture(Some(&active_mode), …)`), while under an OTA session
//! the peer transmits at whatever rung the ladder picked. With the shipped defaults — `mode =
//! "BPSK250"` (cap 37.3 s) and profile `hpx_hf`, whose `initial_level` is SL2 = BPSK31+Rs (66.6 s at
//! one RS block, 131.8 s at two) — every entry-rung frame was force-flushed mid-frame.
//!
//! The pre-existing gate (`burst_cap_frame_length.rs`) cannot see this: it queries the cap with the
//! slow rung *itself*, an equality the daemon never has. These tests keep the cap mode and the
//! transmitted mode DIFFERENT, which is the only configuration in which the defect exists.

use bpsk_plugin::BpskPlugin;
use fsk4_plugin::Fsk4Plugin;
use mfsk16_plugin::Mfsk16Plugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::fec::FecMode;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::engine::ModemEngine;

/// The operator's configured mode — fast, so its cap is short. The daemon default.
const CONFIGURED_MODE: &str = "BPSK250";
/// Two RS blocks, so the frame is the 131.8 s case rather than the 66.6 s one.
const TWO_BLOCK_PAYLOAD: usize = 213;
/// Daemon default `receive_tick_ms = 50` at 8 kHz.
const TICK_SAMPLES: usize = 400;

fn engine() -> (ModemEngine, LoopbackBackend) {
    let backend = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(backend.clone_shared()));
    e.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    e.register_plugin(Box::new(Mfsk16Plugin::new())).unwrap();
    e.register_plugin(Box::new(Fsk4Plugin::new())).unwrap();
    (e, backend)
}

/// The entry-rung frame as it actually goes on the wire.
fn entry_rung_frame() -> Vec<f32> {
    let (mut tx, bk) = engine();
    tx.transmit_with_fec_mode(
        &vec![0x5Au8; TWO_BLOCK_PAYLOAD],
        "BPSK31",
        FecMode::Rs,
        None,
    )
    .expect("transmit SL2 entry-rung frame");
    bk.drain_samples()
}

/// Feed `frame` in tick-sized chunks then digital silence, counting flushes. Amplitude is left as
/// transmitted (well above the 0.01 default DCD threshold) so the verdict does not depend on the
/// noise-floor tracker warming — that is #1254's question, not this one.
fn flushes(e: &mut ModemEngine, frame: &[f32]) -> Vec<usize> {
    let mut out = Vec::new();
    for chunk in frame.chunks(TICK_SAMPLES) {
        if let Ok(Some(b)) = e.accumulate_capture(Some(CONFIGURED_MODE), chunk.to_vec()) {
            out.push(b.samples.len());
        }
    }
    for _ in 0..8 {
        if let Ok(Some(b)) = e.accumulate_capture(Some(CONFIGURED_MODE), vec![0.0; TICK_SAMPLES]) {
            out.push(b.samples.len());
        }
    }
    out
}

/// The defect: under an OTA session the entry rung must survive as ONE burst.
#[test]
fn an_ota_entry_rung_frame_is_not_split_by_a_cap_sized_for_the_configured_mode() {
    let frame = entry_rung_frame();
    let (mut e, _bk) = engine();
    e.start_ota_session(SessionProfile::hpx_hf());

    let bursts = flushes(&mut e, &frame);
    assert_eq!(
        bursts.len(),
        1,
        "an SL2 BPSK31+Rs frame ({} samples) arrived as {} bursts while configured for {CONFIGURED_MODE}; \
         the cap must cover the OTA candidate rungs, not just the configured mode",
        frame.len(),
        bursts.len()
    );
    assert!(
        bursts[0] >= frame.len(),
        "the single burst ({}) is shorter than the frame ({})",
        bursts[0],
        frame.len()
    );
}

/// Positive control, in the same file: WITHOUT an OTA session the identical feed still splits.
///
/// This is what stops the gate above going vacuous. If someone later raises `BURST_MIN_CAP_SAMPLES`
/// so that even BPSK250's cap covers the frame, this test fails and says so — instead of the gate
/// above passing for a reason unrelated to the fix.
#[test]
fn without_an_ota_session_the_configured_mode_still_bounds_the_burst() {
    let frame = entry_rung_frame();
    let (mut e, _bk) = engine(); // no start_ota_session

    let bursts = flushes(&mut e, &frame);
    assert!(
        bursts.len() > 1,
        "control: with no OTA session the {CONFIGURED_MODE} cap must still split a {}-sample frame — \
         it arrived as {} burst(s), so the sibling gate above proves nothing",
        frame.len(),
        bursts.len()
    );
}

/// The cap NARROWS again when the ladder is pinned to a fast rung, so the runaway guard still exists.
#[test]
fn locking_the_ladder_to_a_fast_rung_narrows_the_cap_again() {
    let (mut e, _bk) = engine();
    e.start_ota_session(SessionProfile::hpx_hf());
    let entry_cap = e.burst_cap_samples(Some("BPSK31"));

    // Locked to a fast rung, the candidate set no longer contains a slow mode.
    e.ota_lock_level(SpeedLevel::Sl9);
    let frame = entry_rung_frame();
    let bursts = flushes(&mut e, &frame);
    assert!(
        bursts.len() > 1,
        "with the ladder locked to SL9 the entry-rung frame must split again (cap back to the \
         configured mode's {} samples, not the entry rung's {entry_cap})",
        e.burst_cap_samples(Some(CONFIGURED_MODE))
    );
}
