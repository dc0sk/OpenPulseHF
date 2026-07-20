//! A coded frame must be classified by its CODED length, not its raw geometry.
//!
//! **The defect this pins.** `receive_with_fec_mode_timeout` skips a full-buffer retry for
//! "long-frame" modes, because that retry re-scans the whole buffer every 2 s and for a long frame
//! outruns the capture read cadence — the frame then never finishes buffering. The classification was
//! computed from the **raw** geometry and only *then* was the slice widened 3× for FEC, so three modes
//! whose coded frames run ~28 s were treated as short and kept the starving retry:
//!
//! | mode | raw samples | coded (×3) | classified (before) | actual need |
//! |---|---|---|---|---|
//! | `BPSK250` | 74 400 | 223 200 | short | long |
//! | `BPSK250-RRC` | 74 400 | 223 200 | short | long |
//! | `QPSK125` | 75 200 | 225 600 | short | long |
//!
//! Measured on the virtual loopback rung, 2026-07-20: `BPSK250-RRC` reached at most **152 of the 255
//! bytes** it needs across all 1042 scan positions, and `QPSK125` at most 82 — the capture was being
//! starved mid-frame. All three pass with the classification moved after the FEC widening.
//!
//! **Why only the audio rungs saw it.** In-process there is no read cadence to starve: the buffer is
//! already full when the scan begins, so `ChannelSimHarness` passes these modes either way. It took a
//! real streaming capture to expose it, which is exactly what the virtual rung is for.
//!
//! These three are the only modes the reclassification moves — verified against the whole registry.

use openpulse_core::fec::FecMode;
use openpulse_modem::engine::{frame_plan, LONG_FRAME_SAMPLES};

// Exercises the ENGINE's own function, not a copy of the rule. The widening and the classification
// live inside `frame_plan` together, so a change that classified on the raw geometry again would have
// to alter the function these tests call — which is precisely how the bug got in when they were two
// separate steps.

/// THE GATE: the three modes whose coded frames exceed the threshold must classify as long.
///
/// Computing this on the raw geometry (the bug) puts all three on the wrong side.
#[test]
fn coded_frames_over_the_threshold_classify_as_long() {
    for (mode, raw) in [
        ("BPSK250", 74_400),
        ("BPSK250-RRC", 74_400),
        ("QPSK125", 75_200),
    ] {
        let (coded, long) = frame_plan(raw, FecMode::Rs);
        assert!(
            long,
            "{mode}: coded frame is {coded} samples and must classify as long-frame; classifying on \
             the raw {raw} would keep the retry that starves the capture read loop"
        );
    }
}

/// The same modes UNCODED are genuinely short — the widening is what moves them, not the mode.
#[test]
fn the_same_modes_uncoded_stay_short() {
    for (mode, raw) in [
        ("BPSK250", 74_400),
        ("BPSK250-RRC", 74_400),
        ("QPSK125", 75_200),
    ] {
        let (plain, long) = frame_plan(raw, FecMode::None);
        assert!(
            !long,
            "{mode}: uncoded frame is {plain} samples and must stay short — the FEC widening is the \
             only reason the coded frame crosses the threshold"
        );
    }
}

/// Control: the wideband modes must NOT be reclassified. The comment in the engine is explicit that
/// SCFDMA/OFDM "depend on the retry's per-position re-acquisition", so sweeping them into long-frame
/// would trade this bug for a worse one.
#[test]
fn the_wideband_modes_keep_their_retry() {
    // SCFDMA52 / OFDM52 raw geometries are well under 40 000 samples, so even x3 they stay short.
    for (mode, raw) in [
        ("SCFDMA52", 33_000),
        ("OFDM52", 30_000),
        ("SCFDMA52-8PSK", 24_000),
    ] {
        let (coded, long) = frame_plan(raw, FecMode::Rs);
        assert!(
            !long,
            "{mode}: coded frame is {coded} samples — it must stay short-frame and keep the retry it \
             depends on for acquisition"
        );
    }
}

/// The threshold is a boundary, so pin both sides of it.
#[test]
fn the_threshold_is_pinned_on_both_sides() {
    let (_, at) = frame_plan(LONG_FRAME_SAMPLES, FecMode::None);
    let (_, over) = frame_plan(LONG_FRAME_SAMPLES + 1, FecMode::None);
    assert!(!at, "exactly at the threshold is not long");
    assert!(over, "one sample over the threshold is long");
}

/// The widened length is returned too, and the caller must use it as the slice bound.
#[test]
fn the_returned_length_is_the_widened_one() {
    let (coded, _) = frame_plan(74_400, FecMode::Rs);
    assert_eq!(coded, 223_200, "a coded frame is 3x the raw geometry");
    let (plain, _) = frame_plan(74_400, FecMode::None);
    assert_eq!(plain, 74_400, "an uncoded frame is not widened");
}
