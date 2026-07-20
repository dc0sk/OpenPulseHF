//! `SCFDMA52-LP` must decode wherever the frame sits in the capture, not only at sample 0.
//!
//! **The defect this pins.** `deramp_timing` returned early for the localized (block-pilot) layout,
//! on the reasoning that it has "no evenly-spaced pilots to fit a ramp". That premise is wrong:
//! `SCFDMA52-LP`'s four pilots are **contiguous** at subcarriers 77–80, which is even spacing of 1,
//! and `pilot_spacing` is already 1 for the mode. Skipping the fit left the mode with no tolerance to
//! residual timing offset at all.
//!
//! Measured 2026-07-20 across 12 embedded frame positions:
//!
//! | mode | decoded |
//! |---|---|
//! | `SCFDMA52-LP` (before) | **1/12** — only with the frame at sample 0 |
//! | `SCFDMA52-LP` (after) | 12/12 |
//! | `SCFDMA52` (control) | 12/12 both ways |
//!
//! A **one-sample** lead offset was enough to break it. Since a real receiver never has the frame at
//! offset 0 — it listens for seconds and the frame arrives somewhere inside — the mode could not work
//! on any real capture, which is exactly how it surfaced: it failed on both audio loopback rungs while
//! passing every in-process test that used `route_clean`.
//!
//! This does **not** make `SCFDMA52-LP` generally robust. It remains a flat-channel demonstrator in no
//! adaptive profile: its single-tap CE still assumes flat gain/phase and no passband tilt. Only the
//! timing-offset fragility was a bug.

use std::time::Duration;

use openpulse_core::fec::FecMode;
use openpulse_modem::channel_sim::ChannelSimHarness;
use scfdma_plugin::ScFdmaPlugin;

/// Decode `mode` with the frame preceded by `lead` samples of silence.
fn decodes_at(mode: &str, lead: usize) -> bool {
    let mut h = ChannelSimHarness::new();
    for e in [&mut h.tx_engine, &mut h.rx_engine] {
        e.register_plugin(Box::new(ScFdmaPlugin::new()))
            .expect("register");
    }
    let payload: Vec<u8> = (0..64u8).collect();
    if h.tx_engine
        .transmit_with_fec_mode(&payload, mode, FecMode::Rs, None)
        .is_err()
    {
        return false;
    }
    h.route_embedded(lead, 120_000);
    h.rx_engine
        .receive_with_fec_mode_timeout(mode, FecMode::Rs, None, Duration::from_millis(9000))
        .map(|v| v == payload)
        .unwrap_or(false)
}

/// Offsets spanning sub-sample-scale through a realistic multi-second lead.
const OFFSETS: [usize; 8] = [0, 1, 2, 3, 5, 16, 40_000, 40_001];

/// THE GATE: every frame position must decode.
///
/// Before the fix only `lead = 0` worked — a one-sample offset was enough to fail.
#[test]
fn scfdma52_lp_decodes_at_every_frame_position() {
    let failures: Vec<usize> = OFFSETS
        .iter()
        .copied()
        .filter(|&lead| !decodes_at("SCFDMA52-LP", lead))
        .collect();
    assert!(
        failures.is_empty(),
        "SCFDMA52-LP failed at lead offsets {failures:?} of {OFFSETS:?} — the frame must decode \
         wherever it sits in the capture, since a real receiver never has it at sample 0"
    );
}

/// The specific case that made this unusable: one sample of lead silence.
#[test]
fn a_one_sample_offset_is_tolerated() {
    assert!(
        decodes_at("SCFDMA52-LP", 1),
        "a single sample of lead silence broke SCFDMA52-LP; that is the whole defect in one case"
    );
}

/// Control: the interleaved-pilot sibling shares the deramp path and must be unaffected.
#[test]
fn scfdma52_is_unaffected() {
    let failures: Vec<usize> = OFFSETS
        .iter()
        .copied()
        .filter(|&lead| !decodes_at("SCFDMA52", lead))
        .collect();
    assert!(
        failures.is_empty(),
        "SCFDMA52 regressed at lead offsets {failures:?} — enabling the localized deramp must not \
         change the interleaved path"
    );
}

/// Anti-vacuity: the offsets actually span more than one sample, so the suite cannot pass by only
/// ever testing the easy `lead = 0` case that used to be the only one that worked.
#[test]
fn the_offsets_exercise_more_than_the_origin() {
    assert!(OFFSETS.contains(&0), "keep the origin as a baseline");
    assert!(
        OFFSETS.iter().filter(|&&o| o > 0).count() >= 5,
        "at least five non-zero offsets are needed for this to be testing frame POSITION"
    );
    assert!(
        OFFSETS.iter().any(|&o| o > 1000),
        "include a realistic multi-second lead, not just sub-symbol offsets"
    );
}
