//! A coded frame must still decode when the receiver settled AFC on noise before it arrived.
//!
//! **The defect this pins (issue #1021).** On a real 2 m link, `BPSK250|none` decoded reliably while
//! `BPSK250|rs` and `BPSK250|soft_concatenated` never decoded at all — same session, same rigs,
//! strong signal, carrier alignment measured at +1.2 Hz. The mechanism is a two-part trap:
//!
//! 1. `EnergyGate::threshold()` returns the bare `ABS_THRESHOLD` (1e-4) until it has 32 windows of
//!    history. A real idle floor above that (measured: 4.2e-4) trips the gate on NOISE within the
//!    first few scan positions, and `afc_mini_settle` returns a confident, plausible correction from
//!    it (measured: −49.8 Hz against a true +1.2 Hz), settling ~40 000 samples before the frame.
//! 2. `ScanPlanner::note_settled` is permanent — there is no un-settle and no re-anchor. The
//!    first-energy micro-sweep then re-decodes forever from that noise position, and its reach is
//!    only `fep + 8 × (step/2)` — four symbols. It can never walk to the real frame.
//!
//! Uncoded survives this because the full-buffer retry rescues it (measured: 115 retry attempts
//! after an equally bogus 136.7 Hz settle). Coded does not, because `frame_plan` widens the frame
//! ×3 for FEC, which pushes `BPSK250` across `LONG_FRAME_SAMPLES` (74 624 → 223 872 vs a 120 000
//! threshold) and `long_frame && is_settled()` disables that very retry — the one the code's own
//! comments call "the fallback for a bad settle".
//!
//! **Why nothing in-process caught it.** `route_embedded` pads *silence*, so the idle level is
//! exactly zero and the cold-start gate can never fire early — the noise settle is structurally
//! invisible. And `fec_scan_long_capture`, the one scanning-coded long-capture test, uses QPSK250,
//! whose coded frame (114 048) stays *under* the threshold — so it is `long_frame = false` and
//! exercises the retry-driven branch, never the settled-long-frame branch that coded BPSK250 takes.
//! The failing configuration had zero coverage.
//!
//! These tests use [`ChannelSimHarness::route_embedded_noisy`] to reproduce the on-air idle floor.

use std::time::Duration;

use bpsk_plugin::BpskPlugin;
use openpulse_core::fec::FecMode;
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use openpulse_modem::channel_sim::ChannelSimHarness;
use openpulse_modem::engine::{frame_arrival_samples, frame_plan, LONG_FRAME_SAMPLES};

/// The measured on-air idle floor: mean-square 4.2e-4 => RMS ~0.0205. Above the energy gate's
/// 1e-4 cold-start floor, which is exactly what makes the gate fire before the frame.
const ONAIR_IDLE_RMS: f32 = 0.0205;

/// Lead padding long enough that a noise settle lands far outside the micro-sweep's reach,
/// mirroring the on-air 5 s transmitter start delay.
const LEAD: usize = 40_000;

/// Must comfortably exceed `ScanPlanner::RETRY_START_SECS` (12 s), or the full-buffer retry — the
/// mechanism that rescues the uncoded control on air — never fires and the control fails for a
/// reason that has nothing to do with the defect under test. The on-air listen window was 155 s.
const RECEIVE_TIMEOUT_MS: u64 = 25_000;

/// Trailing padding that makes the total capture ~269 000 samples, matching the on-air run
/// (268 000 accumulated). This is load-bearing, not cosmetic: a receiver cannot judge a settled
/// position undecodable until a whole frame's worth of audio exists past it, so a shorter capture
/// leaves the anchor forever "not yet fully buffered" and the recovery can never engage — which is
/// exactly how this test failed before the length was matched to the real capture.
///
/// The threshold is the RAW frame geometry (74 624 samples for BPSK250), not the widened FEC slice
/// reserve. It was the reserve until 2026-07-29, and the figure quoted here — "widened x3, 223 872
/// samples" — was stale twice over: a same-week commit had already changed the factor to x2
/// (149 248), and the reserve was the wrong quantity to measure arrival with at all. Sizing arrival
/// from the reserve made this recovery unreachable for BPSK31/BPSK63 in every configured harness
/// (archetype scan 2026-07-29, findings 4 and 16).
const TRAIL: usize = 160_000;

fn harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    for eng in [&mut h.tx_engine, &mut h.rx_engine] {
        eng.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    }
    h
}

fn round_trip_in_noise(fec: FecMode, payload: &[u8], seed: u64) -> Result<Vec<u8>, String> {
    let mut h = harness();
    h.tx_engine
        .transmit_with_fec_mode(payload, "BPSK250", fec, None)
        .map_err(|e| format!("transmit: {e}"))?;
    let frame_samples = h.route_embedded_noisy(LEAD, TRAIL, ONAIR_IDLE_RMS, seed);
    assert!(
        frame_samples > 0,
        "nothing was transmitted — the test would prove nothing"
    );
    h.rx_engine
        .receive_with_fec_mode_timeout(
            "BPSK250",
            fec,
            None,
            Duration::from_millis(RECEIVE_TIMEOUT_MS),
        )
        .map_err(|e| format!("{e}"))
}

/// The classification that flips the failing configuration across the retry gate. If this ever
/// stops holding, the reproduction below stops reproducing anything and must be re-derived.
#[test]
fn coded_bpsk250_is_long_frame_while_uncoded_is_not() {
    // BPSK250 @ 8 kHz raw geometry, taken from the plugin rather than hardcoded.
    let raw = BpskPlugin::new()
        .frame_geometry(&ModulationConfig {
            mode: "BPSK250".to_string(),
            sample_rate: 8_000,
            ..ModulationConfig::default()
        })
        .expect("BPSK250 geometry")
        .max_frame_samples;

    let (_, uncoded_long) = frame_plan(raw, FecMode::None);
    let (coded_len, coded_long) = frame_plan(raw, FecMode::Rs);

    assert!(
        !uncoded_long,
        "uncoded BPSK250 ({raw} samples) must stay under LONG_FRAME_SAMPLES ({LONG_FRAME_SAMPLES}) \
         — it is the control that decodes on air via the full-buffer retry"
    );
    assert!(
        coded_long,
        "coded BPSK250 ({coded_len} samples) must exceed LONG_FRAME_SAMPLES ({LONG_FRAME_SAMPLES}) \
         — this is the classification that disables the retry and causes issue #1021"
    );
}

/// The control: uncoded decodes through a noise settle, exactly as measured on air.
#[test]
fn uncoded_bpsk250_recovers_from_a_noise_settle() {
    let payload = b"uncoded control payload".to_vec();
    let mut last = String::new();
    for seed in 1..=2u64 {
        match round_trip_in_noise(FecMode::None, &payload, seed) {
            Ok(got) => {
                assert_eq!(got, payload, "uncoded round trip must return the payload");
                return;
            }
            Err(e) => last = e,
        }
    }
    panic!(
        "uncoded BPSK250 failed to decode through a noise-padded capture in 2 seeds \
         (last error: {last}). On air this is the case that PASSES; if it fails here the \
         reproduction is not modelling the on-air situation and the coded assertion below \
         would prove nothing."
    );
}

/// The defect: coded must decode through the same capture. Fails until the bad settle is
/// recoverable for long-frame modes.
#[test]
fn coded_bpsk250_recovers_from_a_noise_settle() {
    let payload = b"coded payload that must survive a noise settle".to_vec();
    let mut last = String::new();
    for seed in 1..=2u64 {
        match round_trip_in_noise(FecMode::Rs, &payload, seed) {
            Ok(got) => {
                assert_eq!(got, payload, "coded round trip must return the payload");
                return;
            }
            Err(e) => last = e,
        }
    }
    panic!(
        "coded BPSK250+Rs never decoded through a noise-padded capture (last error: {last}). \
         This is issue #1021: the noise settle latches (`note_settled` is permanent), the \
         micro-sweep can only reach ~4 symbols from it, and `long_frame && is_settled()` \
         disables the full-buffer retry that rescues the uncoded control."
    );
}

/// THE REACHABILITY GATE: the settle-recovery precondition must be satisfiable for the ladder's slow
/// rungs inside a realistic listen window.
///
/// The recovery above cannot fire at all until `accumulated.len() >= onset + arrival_samples`. Sizing
/// `arrival_samples` from the FEC slice reserve put that threshold at 149.2 s of post-onset audio for
/// BPSK31 — longer than any configured harness listens for (the dual-card rig's window is 109.6 s),
/// so the recovery was dead code on `hpx_hf`'s entry rung. The tests above could not see it: they pin
/// BPSK250 exclusively, the one mode where the old threshold *was* satisfiable.
///
/// This is an arithmetic gate on purpose — it compares the two real constants (the plugin's own frame
/// geometry, and the listen windows the on-air scripts configure) rather than re-deriving either.
#[test]
fn the_settle_recovery_threshold_is_reachable_for_the_slow_rungs() {
    const SAMPLE_RATE: usize = 8_000;
    /// The shortest listen window any configured harness uses (the dual-card rig; the on-air runner
    /// uses 130 s). Recovery must be reachable inside it.
    const SHORTEST_LISTEN_SECS: f32 = 109.6;

    let plugin = BpskPlugin::new();
    for mode in ["BPSK31", "BPSK63", "BPSK125", "BPSK250"] {
        let cfg = ModulationConfig {
            mode: mode.to_string(),
            sample_rate: SAMPLE_RATE as u32,
            ..ModulationConfig::default()
        };
        let geom = plugin
            .frame_geometry(&cfg)
            .unwrap_or_else(|| panic!("{mode} must report frame geometry"));

        // Read the engine's OWN arrival decision, not the geometry it happens to be derived from
        // today — otherwise re-sizing arrival back to the FEC reserve would leave this gate green.
        let arrival = frame_arrival_samples(geom.max_frame_samples, FecMode::Rs);
        let arrival_secs = arrival as f32 / SAMPLE_RATE as f32;
        assert!(
            arrival_secs < SHORTEST_LISTEN_SECS,
            "{mode}: the settle-recovery precondition needs {arrival_secs:.1} s of post-onset audio, \
             but the shortest configured listen window is {SHORTEST_LISTEN_SECS} s — the recovery \
             can never fire for this rung"
        );

        // And the anti-vacuity half: the FEC slice reserve — what the precondition used to be sized
        // from — must be genuinely larger, or this gate is testing nothing.
        let (reserve, _) = frame_plan(geom.max_frame_samples, FecMode::Rs);
        assert!(
            reserve > arrival,
            "{mode}: the slice reserve ({reserve}) is not larger than the arrival threshold \
             ({arrival}) — the two quantities this gate exists to keep apart have collapsed into one"
        );
    }

    // The specific rung the defect disabled: BPSK31's old threshold really was out of reach.
    let cfg = ModulationConfig {
        mode: "BPSK31".to_string(),
        sample_rate: SAMPLE_RATE as u32,
        ..ModulationConfig::default()
    };
    let geom = plugin.frame_geometry(&cfg).expect("BPSK31 geometry");
    let (old_threshold, _) = frame_plan(geom.max_frame_samples, FecMode::Rs);
    assert!(
        old_threshold as f32 / SAMPLE_RATE as f32 > SHORTEST_LISTEN_SECS,
        "the old reserve-sized threshold for BPSK31 is no longer unreachable, so this suite is \
         documenting a defect that no longer exists in the form described"
    );
}
