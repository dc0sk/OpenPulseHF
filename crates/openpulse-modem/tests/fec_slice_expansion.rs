//! What a coded frame ACTUALLY costs in samples, versus the per-attempt slice the scanning
//! receive reserves for it.
//!
//! `frame_plan` multiplies the mode's `max_frame_samples` by a per-FEC factor to size the slice a
//! decode attempt hands the demodulator, and classifies `long_frame` on the result. Both are
//! load-bearing, and both were wrong under the previous blanket x3:
//!
//! * a mode's `max_frame_samples` is already sized for a **full 255-byte RS block plus envelope**
//!   (see the `frame_geometry` comment in each plugin), so multiplying by 3 double-counted the RS
//!   expansion that the geometry already contained;
//! * the inflated value pushed `BPSK250 + Rs` from 74 624 to 223 872 samples, across
//!   `LONG_FRAME_SAMPLES` (120 000), which disabled the full-buffer retry — and that is what left
//!   the coded rungs unable to recover from a bad settle on air (issue #1021).
//!
//! This test measures the real transmitted length for each FEC mode so the factors in `frame_plan`
//! stay grounded in what the codecs emit rather than in a guess. If a codec's expansion changes,
//! the assertion below fails and the factor table must be revisited.

use bpsk_plugin::BpskPlugin;
use openpulse_core::fec::FecMode;
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use openpulse_modem::channel_sim::ChannelSimHarness;
use openpulse_modem::engine::frame_plan;

const MODE: &str = "BPSK250";

fn raw_max_frame_samples() -> usize {
    BpskPlugin::new()
        .frame_geometry(&ModulationConfig {
            mode: MODE.to_string(),
            sample_rate: 8_000,
            ..ModulationConfig::default()
        })
        .expect("BPSK250 geometry")
        .max_frame_samples
}

/// Transmitted sample count for `payload` under `fec`.
fn coded_samples(fec: FecMode, payload: &[u8]) -> usize {
    let mut h = ChannelSimHarness::new();
    for eng in [&mut h.tx_engine, &mut h.rx_engine] {
        eng.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    }
    h.tx_engine
        .transmit_with_fec_mode(payload, MODE, fec, None)
        .expect("transmit");
    h.route_clean()
}

/// The slice `frame_plan` reserves must actually cover the frame the transmitter emits — for every
/// FEC mode and payload size the scanning receive supports. A slice shorter than the frame
/// truncates it; a slice far longer wastes the buffer the receiver must accumulate before it can
/// judge a position (which is what made the #1021 recovery unreachable on a real capture).
#[test]
fn the_reserved_slice_covers_the_real_coded_frame_without_gross_overshoot() {
    let raw = raw_max_frame_samples();
    // Payload sizes that bracket the RS block boundary: one block, and enough to force a second.
    let payloads = [16usize, 64, 200, 255];

    for fec in [
        FecMode::None,
        FecMode::Rs,
        FecMode::RsStrong,
        FecMode::RsInterleaved,
        FecMode::Concatenated,
        FecMode::SoftConcatenated,
    ] {
        let (slice, _long) = frame_plan(raw, fec);
        for &n in &payloads {
            let payload = vec![0xA5u8; n];
            let actual = coded_samples(fec, &payload);
            assert!(
                slice >= actual,
                "{fec:?} with a {n}-byte payload emits {actual} samples but frame_plan reserves \
                 only {slice} — a decode attempt would truncate the frame"
            );
            // Guard against the double-counting that caused #1021: the reserve should not be an
            // order of magnitude past what the codec actually emits at its largest payload.
            if n == 255 {
                assert!(
                    slice <= actual * 3,
                    "{fec:?} reserves {slice} samples for a frame that is only {actual} — an \
                     inflated reserve pushes modes across LONG_FRAME_SAMPLES and makes a settled \
                     position impossible to judge on a real capture (#1021)"
                );
            }
        }
    }
}

/// The reserve must be small enough that a bad settle can actually be **judged** on a real capture.
///
/// `BPSK250 + Rs` stays `long_frame` — measurement says that is correct, its worst-case frame really
/// is ~131 800 samples (16.5 s) — so the full-buffer retry stays disabled and the only recovery from
/// a settle on noise is the scan re-anchor. That re-anchor cannot fire until `max_frame_samples`
/// of audio exists past the settled position, which makes the reserve size a *reachability*
/// constraint, not just a memory one.
///
/// The numbers below are the measured on-air run of 2026-07-28 (issue #1021): AFC settled at sample
/// 83 608 and the capture reached ~253 000 samples. Under the old blanket ×3 reserve (223 872) the
/// re-anchor needed 307 480 samples and could never fire; the coded rungs failed for the whole
/// session. Any future factor edit that breaks this assertion re-breaks that recovery.
#[test]
fn a_bad_settle_stays_judgeable_within_a_real_capture() {
    const ONAIR_SETTLE_ONSET: usize = 83_608;
    const ONAIR_CAPTURE_REACHED: usize = 253_000;

    let raw = raw_max_frame_samples();
    let (slice, _) = frame_plan(raw, FecMode::Rs);
    let needed = ONAIR_SETTLE_ONSET + slice;

    assert!(
        needed <= ONAIR_CAPTURE_REACHED,
        "re-anchoring BPSK250+Rs needs {needed} samples of capture (settle at {ONAIR_SETTLE_ONSET} \
         + a {slice}-sample reserve), but the measured on-air capture only reached \
         {ONAIR_CAPTURE_REACHED}. A settle on noise could never be condemned, which is exactly how \
         issue #1021 survived the first fix attempt."
    );
}
