//! An idle floor above the energy gate's ceiling breaks acquisition, in process.
//!
//! **The defect this pins (on-air 2026-07-28, issue #1020).** `EnergyGate` sets its threshold to
//! `clamp(idle_floor * 3, 0.0001, 0.0032)` and only hands audio to the demodulator above it. If the
//! IDLE floor itself exceeds the 0.0032 ceiling, the threshold clamps BELOW the noise: the gate can
//! never shut, it fires on noise, the receiver settles AFC on that noise, and a perfectly aligned
//! frame decodes to garbage. Measured on the IC-9700 receive path: idle mean-square **0.0154** at
//! PipeWire source volume 1.00 — 4.7x over the ceiling — and `BPSK250|none|64` failed on air. At
//! source volume 0.55 the idle floor read 0.00042 and the identical case passed.
//!
//! **Why no in-process test could see it.** Every harness route runs at a normalised level, and
//! `route_embedded` pads *silence*, so the idle floor is exactly zero and the absolute ceiling is
//! unreachable by construction. The capture level is a property of the rig and host mixer, not of
//! the channel, so no amount of AWGN or fading would have produced it.
//!
//! [`ChannelSimHarness::route_embedded_at_level`] sets the idle floor directly, in the same
//! mean-square units the gate compares against.

use std::time::Duration;

use bpsk_plugin::BpskPlugin;
use openpulse_modem::channel_sim::ChannelSimHarness;

/// The engine's `EnergyGate::MAX_THRESHOLD`. An idle floor above a third of this cannot be
/// discriminated, because the adaptive `floor * 3` rule clamps here.
const GATE_CEILING_MEAN_SQ: f32 = 0.0032;

/// Measured on air at PipeWire source volume 1.00 — the level that failed.
const HOT_IDLE_MEAN_SQ: f32 = 0.0154;
/// Measured on air at source volume 0.55 — the level that passed.
const GOOD_IDLE_MEAN_SQ: f32 = 0.00042;

/// The hot case fails by design and burns its whole window on every trial, so keep it short: this
/// is well past the ~6 s the healthy case needs to acquire, and the assertion is about the gate,
/// not about patience.
const RECEIVE_TIMEOUT_MS: u64 = 7_000;

const LEAD: usize = 40_000;
const TRAIL: usize = 40_000;

fn harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    for eng in [&mut h.tx_engine, &mut h.rx_engine] {
        eng.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    }
    h
}

fn round_trip_at_idle_level(idle_mean_sq: f32, seed: u64) -> Result<Vec<u8>, String> {
    let payload = b"capture level probe".to_vec();
    let mut h = harness();
    h.tx_engine
        .transmit(&payload, "BPSK250", None)
        .map_err(|e| format!("transmit: {e}"))?;
    let n = h.route_embedded_at_level(LEAD, TRAIL, idle_mean_sq, 1.0, seed);
    assert!(
        n > 0,
        "nothing was transmitted — the test would prove nothing"
    );
    h.rx_engine
        .receive_with_timeout("BPSK250", None, Duration::from_millis(RECEIVE_TIMEOUT_MS))
        .map_err(|e| format!("{e}"))
}

/// Sanity on the arithmetic the whole failure turns on, so the numbers below are not folklore.
///
/// The comparisons are constant by construction — that is the point. They exist so that editing any
/// one of these constants without re-deriving the others fails here rather than silently turning the
/// tests below into no-ops.
#[allow(clippy::assertions_on_constants)]
#[test]
fn the_hot_floor_really_is_above_the_gate_ceiling() {
    assert!(
        HOT_IDLE_MEAN_SQ > GATE_CEILING_MEAN_SQ,
        "the 'hot' level must exceed the gate ceiling or this file tests nothing"
    );
    assert!(
        GOOD_IDLE_MEAN_SQ * 3.0 < GATE_CEILING_MEAN_SQ,
        "the 'good' level must leave the adaptive threshold unclamped"
    );
}

/// THE CONTROL: at the level that passed on air, the frame must decode. If this ever fails, the
/// reproduction is not modelling the on-air situation and the failing case below proves nothing.
#[test]
fn at_a_healthy_idle_floor_the_frame_decodes() {
    let mut last = String::new();
    for seed in 1..=3u64 {
        match round_trip_at_idle_level(GOOD_IDLE_MEAN_SQ, seed) {
            Ok(got) => {
                assert_eq!(got, b"capture level probe".to_vec());
                return;
            }
            Err(e) => last = e,
        }
    }
    panic!(
        "BPSK250 failed to decode at the idle floor that PASSED on air (mean_sq \
         {GOOD_IDLE_MEAN_SQ}); last error: {last}"
    );
}

/// THE DEFECT: at the level measured on air, acquisition is degraded — the gate cannot
/// discriminate, so the receiver settles on noise instead of the frame.
///
/// Deliberately asserted as "does not reliably decode" rather than "always errors": the gate being
/// saturated open does not make a decode impossible, it makes it a coin flip on where the scan
/// happens to land. Requiring a hard error would encode luck as a requirement.
#[test]
fn a_hot_idle_floor_degrades_acquisition() {
    let mut successes = 0;
    let trials = 3;
    for seed in 1..=trials {
        if round_trip_at_idle_level(HOT_IDLE_MEAN_SQ, seed).is_ok() {
            successes += 1;
        }
    }
    assert!(
        successes < trials,
        "all {trials} trials decoded at an idle floor of {HOT_IDLE_MEAN_SQ} mean-square, which is \
         {:.1}x the energy gate's {GATE_CEILING_MEAN_SQ} ceiling. Either the gate no longer clamps \
         (in which case #1020's mechanism is gone and this test should be re-derived) or the \
         harness is not actually applying the level.",
        HOT_IDLE_MEAN_SQ / GATE_CEILING_MEAN_SQ
    );
}
