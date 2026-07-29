//! Replay REAL recorded radio audio through the modem.
//!
//! The emulated impairments (idle floor, capture level, carrier offset, AGC, read cadence) are each
//! a *model* of a radio, and a model can be wrong in precisely the way that hides a bug. A recorded
//! capture cannot be: it is the signal a rig actually produced. These tests replay the corpus in
//! `tests/captures/` (provenance in the README there) and assert against measurements taken
//! independently, on air, at record time.
//!
//! Two distinct jobs are done here:
//!
//! 1. **Corpus integrity** — the recorded levels and the recorded carrier must still measure what
//!    the README says. If a file is re-encoded, resampled or truncated, the tests below that use it
//!    as a stand-in for reality would quietly stop meaning anything.
//! 2. **Modem behaviour in a real capture context** — a synthetic frame padded with genuinely
//!    recorded idle audio, which is the closest this suite gets to the on-air situation without a
//!    radio.
//!
//! The corpus deliberately brackets BOTH level failures: an IC-9700 floor far above the energy
//! gate's ceiling, and an FT-991A floor far below its absolute threshold.

use std::time::Duration;

use bpsk_plugin::BpskPlugin;
use openpulse_modem::capture_replay::{load_corpus, Capture};
use openpulse_modem::channel_sim::ChannelSimHarness;

/// `EnergyGate::MAX_THRESHOLD` — the ceiling the adaptive `floor * 3` rule clamps to.
const GATE_CEILING_MEAN_SQ: f32 = 0.0032;
/// `EnergyGate::ABS_THRESHOLD` — the floor used until 32 windows of history exist.
const GATE_ABS_MEAN_SQ: f32 = 0.0001;

fn harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    for eng in [&mut h.tx_engine, &mut h.rx_engine] {
        eng.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    }
    h
}

fn corpus(name: &str) -> Capture {
    load_corpus(name).unwrap_or_else(|e| panic!("corpus file {name} must load: {e}"))
}

// ── Corpus integrity ─────────────────────────────────────────────────────────

/// Every corpus file must load, be at the modem's rate, and carry actual audio.
#[test]
fn the_corpus_loads_and_is_at_the_modem_sample_rate() {
    for name in [
        "ic9700-idle-hot.wav",
        "ft991a-idle.wav",
        "ic9700-tone-1501hz.wav",
    ] {
        let c = corpus(name);
        assert_eq!(
            c.sample_rate, 8_000,
            "{name} must be 8 kHz (the modem's rate)"
        );
        assert!(
            c.duration_secs() > 1.0,
            "{name} is only {:.2} s — too short to stand in for a capture context",
            c.duration_secs()
        );
        assert!(
            c.samples.iter().any(|&s| s != 0.0),
            "{name} is silent; a corpus file that decoded to zeros would make every test using it \
             vacuous"
        );
    }
}

/// The IC-9700 capture must still be the HOT floor that broke #1020 — above the gate ceiling.
/// This is the property the file exists to preserve; re-encoding that changed the level would make
/// it an ordinary recording rather than evidence.
#[test]
fn the_ic9700_capture_is_still_above_the_gate_ceiling() {
    let c = corpus("ic9700-idle-hot.wav");
    let m = c.mean_sq();
    assert!(
        m > GATE_CEILING_MEAN_SQ,
        "recorded IC-9700 idle floor measures {m:.6}, no longer above the {GATE_CEILING_MEAN_SQ} \
         gate ceiling — this file's whole purpose is to BE the level that saturated the gate"
    );
    // Recorded on air at ~0.0154; allow for decimation but catch a wholesale level change.
    assert!(
        (0.008..0.030).contains(&m),
        "recorded IC-9700 idle floor measures {m:.6}, far from the 0.0154 measured on air"
    );
}

/// The FT-991A capture must still be the opposite failure: below the gate's ABSOLUTE floor, where
/// a signal comfortably above the noise can still fail to open the gate.
#[test]
fn the_ft991a_capture_is_still_below_the_absolute_threshold() {
    let c = corpus("ft991a-idle.wav");
    let m = c.mean_sq();
    assert!(
        m < GATE_ABS_MEAN_SQ,
        "recorded FT-991A idle floor measures {m:.9}, no longer below the {GATE_ABS_MEAN_SQ} \
         absolute threshold — it documents the too-quiet failure and must stay under it"
    );
}

/// The recorded tone must still sit where it was independently measured on air (1501.5 Hz after the
/// +64 Hz rig trim). This pins our measurement chain against a real-world truth: if the analysis
/// path ever drifts, this disagrees with a number taken off the radio.
#[test]
fn the_recorded_carrier_is_still_where_the_radio_put_it() {
    let c = corpus("ic9700-tone-1501hz.wav");
    let n = c.samples.len().min(8192);
    let seg = &c.samples[..n];

    // Goertzel over the plausible band; the tone is strong, so a coarse scan is enough.
    let best = (1400..=1600)
        .map(|f| {
            let w = std::f32::consts::TAU * f as f32 / 8_000.0;
            let coeff = 2.0 * w.cos();
            let (mut s1, mut s2) = (0.0f32, 0.0f32);
            for &x in seg {
                let s0 = x + coeff * s1 - s2;
                s2 = s1;
                s1 = s0;
            }
            (f, s1 * s1 + s2 * s2 - coeff * s1 * s2)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .expect("non-empty scan")
        .0;

    assert!(
        (1495..=1510).contains(&best),
        "recorded carrier measures {best} Hz; it was measured at 1501.5 Hz on air after the +64 Hz \
         rig trim. Either the capture changed or the measurement path drifted."
    );
}

// ── Modem behaviour inside a real capture ────────────────────────────────────

/// A frame padded with REAL recorded idle audio, at a level well clear of that floor, must decode.
///
/// This is the control for the failing case below: if a frame cannot be decoded inside real
/// recorded noise at a healthy signal level, then the failure that follows says nothing about the
/// gate.
#[test]
fn a_frame_decodes_inside_real_recorded_idle_audio() {
    let idle = corpus("ft991a-idle.wav");
    let payload = b"replayed capture context".to_vec();
    let mut h = harness();
    h.tx_engine
        .transmit(&payload, "BPSK250", None)
        .expect("transmit");
    let n = h.route_embedded_in_capture(&idle, 24_000, 24_000, 0.3);
    assert!(n > 0, "nothing transmitted — the test would prove nothing");

    let got = h
        .rx_engine
        .receive_with_timeout("BPSK250", None, Duration::from_millis(15_000))
        .expect("a frame at a healthy level must decode inside real recorded idle audio");
    assert_eq!(got, payload);
}

/// THE REPLAY OF THE DEFECT: the same frame, padded with the REAL capture that broke #1020, is not
/// reliably acquired — the recorded floor saturates the energy gate exactly as it did on air.
///
/// Asserted as "not reliable" rather than "always fails": a saturated gate does not make decoding
/// impossible, it makes it depend on where the scan happens to land. Demanding a hard error would
/// encode luck as a requirement.
#[test]
fn the_recorded_hot_floor_degrades_acquisition_as_it_did_on_air() {
    let hot = corpus("ic9700-idle-hot.wav");
    let payload = b"replayed capture context".to_vec();

    let mut successes = 0;
    let trials = 3;
    for trial in 0..trials {
        let mut h = harness();
        h.tx_engine
            .transmit(&payload, "BPSK250", None)
            .expect("transmit");
        // Vary the lead so the frame lands at a different offset in the recorded noise each trial.
        let lead = 16_000 + trial * 3_000;
        h.route_embedded_in_capture(&hot, lead, 16_000, 0.3);
        if h.rx_engine
            .receive_with_timeout("BPSK250", None, Duration::from_millis(6_000))
            .is_ok()
        {
            successes += 1;
        }
    }

    assert!(
        successes < trials,
        "all {trials} trials decoded inside the recorded IC-9700 floor ({:.4} mean-square, {:.1}x \
         the {GATE_CEILING_MEAN_SQ} gate ceiling). On air this level failed. Either the gate no \
         longer clamps, or the replay is not delivering the recorded level.",
        hot.mean_sq(),
        hot.mean_sq() / GATE_CEILING_MEAN_SQ
    );
}

// ── The open #1021 defect, captured off the air ──────────────────────────────

/// THE END-TO-END GATE: a real on-air frame, recorded off a radio, decoding through the modem.
///
/// Recorded 2026-07-29 with `scripts/onair-dual-capture.sh`: the FT-991A transmitted `DUALCAP
/// TEST 1` at 5 W on 144.600 MHz from a **whitening** build while the IC-9700's USB audio and an
/// independent SDR both recorded. This is the artifact the corpus README long listed as its missing
/// piece — the only thing that can assert a decode against audio a radio actually produced, rather
/// than against a model of one.
#[test]
fn a_real_on_air_frame_decodes_end_to_end() {
    let c = corpus("ic9700-frame-bpsk250-none-whitened.wav");
    assert!(
        c.mean_sq() > 1e-4,
        "the captured frame audio has gone silent (mean_sq {:.6}); the artifact is corrupt",
        c.mean_sq()
    );

    let mut h = harness();
    h.feed_capture(&c);
    let got = h
        .rx_engine
        .receive_with_timeout("BPSK250", None, Duration::from_millis(40_000))
        .expect("a real on-air BPSK250 frame must decode");

    assert_eq!(
        String::from_utf8_lossy(&got),
        "DUALCAP TEST 1",
        "decoded payload must match what was actually transmitted"
    );
}

/// THE #1021 GATE: the real coded on-air frame that could not be decoded, decoding.
///
/// Recorded 2026-07-29, IC-9700 ↔ FT-991A over 2 m at 5 W on 144.600 MHz from a whitening build.
/// [`a_real_on_air_frame_decodes_end_to_end`] is its control — same link, same rigs, same levels,
/// minutes apart, uncoded.
///
/// **The bug was never in the signal.** Reconstructing the exact transmitted wire and diffing it
/// against the demodulated bytes showed **0 byte errors in all 255** — the 8.3 s frame arrived off
/// the air byte-perfect, and demodulates cleanly across a **±32 Hz** AFC window. Every physical
/// hypothesis was measured and refuted: carrier at +2.42 Hz drifting −0.3 Hz over the whole burst,
/// margin 7.2 dB against the control's 7.3 dB, amplitude stable to 1.07, and a tight ±0.5 s window
/// failing identically (so not the frame-location class either).
///
/// **It was a livelock in the settle recovery.** The energy gate falls back to a fixed 1e-4
/// absolute threshold until it holds 32 windows of history, and this station's idle floor is 4.1e-4
/// — four times that — so the very first window of pure noise passed the gate and AFC settled on it
/// at sample 96, ~82 000 samples before the frame, with a bogus +364 Hz correction (BPSK250 can
/// track ±62.5 Hz). `ScanPlanner::note_settle_failure` correctly condemned that anchor — and
/// `unsettle` then rewound the scan to 0, where the same noise passed the same gate and re-settled
/// at the same sample. Measured on this artifact: **78 settles, 77 condemnations, all at sample
/// 96**, until the listen window expired. The recovery was reachable and ineffective.
///
/// `unsettle` now resumes just past the condemned anchor, which is sound by the same argument its
/// own comment already made for rewinding — a premature noise anchor sits *before* the real frame.
/// The scan then walks forward, the gate's adaptive threshold takes over once it has history, and
/// the receiver settles at onset 82304 with a **+2.0 Hz** correction — matching the independently
/// measured carrier. 8 settles instead of 78.
///
/// Note what closing this required: the whitening fix (#1027) was real and is retained — it removed
/// 6.2 s of dead carrier, verified on this capture at **0 of 33** dead windows against the
/// predecessor's 25 of 33 — but it was not the cause of the decode failure. Two defects, one
/// symptom.
#[test]
fn the_real_on_air_frame_decodes() {
    let c = corpus("ic9700-frame-bpsk250-rs-whitened.wav");
    // Guard the artifact itself: a capture that lost its burst would turn this into a test of
    // silence, and it would then "fail" for a reason unrelated to #1021.
    assert!(
        c.mean_sq() > 1e-4,
        "the captured frame audio has gone silent (mean_sq {:.6}); the artifact is corrupt",
        c.mean_sq()
    );

    let mut h = harness();
    h.feed_capture(&c);
    let got = h
        .rx_engine
        .receive_with_fec_mode_timeout(
            "BPSK250",
            openpulse_core::fec::FecMode::Rs,
            None,
            Duration::from_millis(40_000),
        )
        .expect("the real on-air BPSK250|rs frame must decode (issue #1021)");

    assert_eq!(
        String::from_utf8_lossy(&got),
        "DUALCAP TEST 1",
        "decoded payload must match what was actually transmitted"
    );
}
