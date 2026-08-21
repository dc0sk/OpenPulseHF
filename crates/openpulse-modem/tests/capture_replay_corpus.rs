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
#[ignore = "#1148: this capture was whitened with the pre-#1148 21-bit keystream, and it also carries the pre-#1062 preamble; re-record after the wire-break package per release-1.0-criteria decision 1, then un-ignore"]
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
#[ignore = "#1148: this capture was whitened with the pre-#1148 21-bit keystream, and it also carries the pre-#1062 preamble; re-record after the wire-break package per release-1.0-criteria decision 1, then un-ignore"]
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

/// The IC-9700's TRANSMIT chain, proven off-air by an independent receiver.
///
/// Every other frame in this corpus was recorded from a *rig's* USB audio, which measures the
/// transmitter and the receiver together. These three were recorded by an SDR (RSPdx, off-air,
/// its own reference and its own front end) during the same keyed transmissions whose rig-side
/// captures **failed**, on 2026-07-30. That asymmetry is the whole point of the file:
///
/// * SDR decodes + rig audio does not → the fault is in that rig's RECEIVE chain, with evidence.
///
/// It is what closed the A→B question of 2026-07-30. Four keyed `BPSK250|rs` runs from the IC-9700
/// to the FT-991A all failed, and the FT-991A's received carrier sat at ~1372 Hz **regardless of
/// its dial** — verified by raw CAT readback before and after each capture, across a 128 Hz dial
/// change that moved the measured offset by ~0 Hz. The SDR heard the same transmissions at
/// 1511–1513 Hz with 47–56 dB SNR and decoded all three. So the transmitter was never the problem,
/// and neither was the modem.
///
/// **The three distinct payloads are the anti-vacuity control.** A test that decoded one fixed
/// string could pass by returning a constant; three captures each recovering their own different
/// string cannot. Do not collapse them to one file.
///
/// Provenance: 144.600 MHz, IC-9700 at 5 W over 2 m, RSPdx on Antenna A, centre 144.600 MHz,
/// fs 192 kHz, RFGR 22 / IFGR 40 (RFGR 12 saturated and produced an undecodable 523–808 Hz smear —
/// the gain matters). IQ demodulated to 8 kHz USB audio by keeping only positive baseband
/// frequencies and taking twice the real part.
#[test]
#[ignore = "#1148: this capture was whitened with the pre-#1148 21-bit keystream, and it also carries the pre-#1062 preamble; re-record after the wire-break package per release-1.0-criteria decision 1, then un-ignore"]
fn the_ic9700_transmit_chain_decodes_off_air_from_an_independent_receiver() {
    for (name, expected) in [
        ("sdr-ic9700tx-bpsk250-rs-1.wav", "RFGAINFIX RS TEST"),
        ("sdr-ic9700tx-bpsk250-rs-2.wav", "TRIMSIGN RS TEST"),
        ("sdr-ic9700tx-bpsk250-rs-3.wav", "TRIMEMP RS TEST"),
    ] {
        let c = corpus(name);
        assert!(
            c.mean_sq() > 1e-4,
            "{name} has gone silent (mean_sq {:.6}); the artifact is corrupt",
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
            .unwrap_or_else(|e| panic!("{name} must decode off-air: {e}"));

        assert_eq!(
            String::from_utf8_lossy(&got),
            expected,
            "{name} must decode to its OWN payload — matching the wrong one would mean this \
             test is not distinguishing the captures"
        );
    }
}

/// Regression guard on the *cost* of the settle recovery, not a proof of the #1040 fix.
///
/// **Be clear about what this does not do.** It passes identically before and after the #1040
/// change — measured 2026-07-30, this capture needs **1 condemnation either way**, because #1039
/// left the gate settling essentially at the frame. So it cannot fail for the crawl it is named
/// after, and it is not the gate that proved the fix; that is
/// `engine::tests::scan_planner_reanchors_past_the_span_the_sweep_already_proved`, which fails
/// without it.
///
/// What it *is* worth: `the_real_on_air_frame_decodes` kept passing through two wildly different
/// recoveries — the original livelock (78 settles at one sample) and the 32-sample crawl — because
/// a decode-or-not assertion is structurally blind to cost. Reading the condemnation count pins
/// this capture's cost so a future change that reintroduces a crawl *here* is caught. Re-anchoring
/// is not free: each condemnation costs `SETTLE_FAILURE_LIMIT` (18) fully-buffered decodes, and a
/// coded BPSK250 decode is a multi-second demodulation.
#[test]
#[ignore = "#1148: this capture was whitened with the pre-#1148 21-bit keystream, and it also carries the pre-#1062 preamble; re-record after the wire-break package per release-1.0-criteria decision 1, then un-ignore"]
fn the_settle_recovery_reaches_the_frame_without_crawling() {
    let c = corpus("ic9700-frame-bpsk250-rs-whitened.wav");
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
        .expect("the #1021 capture must still decode");
    assert_eq!(String::from_utf8_lossy(&got), "DUALCAP TEST 1");

    // The micro-sweep tests onsets at `fep + k*(step/2)` for k in 0..9 — four whole symbols — so
    // every condemnation has already proven that span undecodable. Re-offering it costs the
    // recovery a factor of four for nothing. Bound chosen from measurement, not taste: see the
    // recorded before/after in the traceability ledger.
    let condemnations = h.rx_engine.settle_condemnations();
    assert!(
        condemnations <= 2,
        "settle recovery took {condemnations} condemnations (~{} wasted decodes); it is crawling \
         over ground the micro-sweep already proved undecodable (#1040)",
        condemnations * 18
    );
}

/// #1045: a coded frame must decode through a SATURATING noise floor, at a realistic lead.
///
/// When the idle floor reaches `EnergyGate::MAX_THRESHOLD` the clamped threshold lands *under* the
/// noise, so the gate passes every window and stops carrying information. Nothing downstream knows:
/// the receiver settles on noise, condemns after 18 fully-buffered decodes, re-anchors, and
/// immediately re-settles on the same noise. Measured before the fix, coded `BPSK250|rs` in the
/// recorded `ic9700-idle-hot.wav` floor: **83 / 73 / 73 condemnations at leads 40k / 80k / 120k and
/// not one decode**, while the identical frame at an 8k lead decoded fine — never a margin problem,
/// only how far the recovery had to walk.
///
/// The lead is the point of the test. A short lead passes even on the broken code, because the walk
/// is short enough to finish; anything under ~40k proves nothing here.
///
/// Three fixes were measured and rejected before the one that shipped, all worth not re-attempting:
/// forcing the full-buffer retry live (it reuses the same saturated gate, so it settles on noise too
/// — no lead rescued); removing `MAX_THRESHOLD` (3x a hot floor lands *on* the signal — 0 settles,
/// nothing decoded, including the 8k lead that previously worked); and engaging a relative criterion
/// on *level* saturation, which gates out every buffer-is-the-frame fixture, and cannot be bounded by
/// an absolute constant because the 0.010 AGC fixture sits BELOW the 0.0154 hot noise floor.
#[test]
fn a_coded_frame_decodes_through_a_saturating_floor() {
    let hot = corpus("ic9700-idle-hot.wav");
    // Guard the premise: if this file stopped saturating the gate, the test would silently become an
    // ordinary decode and prove nothing.
    assert!(
        hot.mean_sq() > GATE_CEILING_MEAN_SQ,
        "corpus floor {:.4} no longer saturates the gate — this test's premise is gone",
        hot.mean_sq()
    );

    for lead in [80_000usize, 120_000] {
        let mut h = harness();
        h.tx_engine
            .transmit_with_fec_mode(
                b"saturated gate probe",
                "BPSK250",
                openpulse_core::fec::FecMode::Rs,
                None,
            )
            .expect("transmit");
        h.route_embedded_in_capture(&hot, lead, 40_000, 0.3);

        // #1066: bound the search in WORK, not wall clock — the same input decodes 5/5 idle and
        // 0/5 on eight busy cores, and debug-vs-release is a ~5x speed proxy for that. Chosen to
        // reconcile the #1058 family (PR #1070), not derived.
        h.rx_engine.set_deterministic_scan_positions(Some(8_000));
        h.rx_engine.set_deterministic_max_iterations(Some(64_000));
        let got = h
            .rx_engine
            .receive_with_fec_mode_timeout(
                "BPSK250",
                openpulse_core::fec::FecMode::Rs,
                None,
                Duration::from_millis(40_000),
            )
            .unwrap_or_else(|e| {
                panic!(
                    "lead {lead}: a coded frame must decode through a saturating floor (#1045): \
                     {e} — after {} settle condemnations",
                    h.rx_engine.settle_condemnations()
                )
            });
        assert_eq!(String::from_utf8_lossy(&got), "saturated gate probe");

        // Decoding is necessary but not sufficient — the recovery must also stop thrashing. The
        // pre-fix runs burned 73-83 condemnations without arriving. One or two are expected here by
        // design: the gate raise is *triggered* by the first condemnation.
        let c = h.rx_engine.settle_condemnations();
        assert!(
            c <= 12,
            "lead {lead}: decoded, but after {c} settle condemnations (~{} wasted fully-buffered \
             decodes) — the condemnation feedback is not engaging",
            c * 18
        );
    }
}

/// A mode with NO preamble template must also decode through the saturating floor.
///
/// **This is the case #1045's fix made worse, and #1049 could not see.** The correlation veto only
/// runs where the plugin publishes a `preamble_template`, which today is BPSK alone. Everything else
/// — QPSK, 8PSK, the multicarrier modes — still decides frame start on energy, so it is the honest
/// test of what the energy path does on its own, and it is the configuration a station running any
/// rung above SL5 is actually in.
///
/// Measured on the recorded IC-9700 floor, `QPSK500 + Rs`, before the `condemned_floor` removal:
///
/// | lead | with `condemned_floor` | without |
/// |---|---|---|
/// | 40 000 | **FAIL**, 92 condemnations | OK, 315 |
/// | 80 000 | **FAIL**, 87 condemnations | OK, 314 |
/// | 120 000 | OK, 6 | OK, 315 |
///
/// The mechanism compounds: every condemnation raises the floor through `.max()`, and where no
/// correlation veto suppresses the noise settles, the raises stack until the gate sits *above the
/// signal* and no settle is possible at all. #1045 measured its fix on BPSK250 only and applied it
/// to every mode — the generalised-past-the-boundary shape, made visible only once #1049 removed the
/// BPSK justification for it.
///
/// Keep this test on a NO-TEMPLATE mode. If QPSK ever gains a `preamble_template`, this stops
/// covering the energy-only path and a different mode must take its place.
#[test]
fn a_no_template_mode_decodes_through_a_saturating_floor() {
    let hot = corpus("ic9700-idle-hot.wav");
    assert!(
        hot.mean_sq() > GATE_CEILING_MEAN_SQ,
        "corpus floor {:.4} no longer saturates the gate — this test's premise is gone",
        hot.mean_sq()
    );

    let mut h = ChannelSimHarness::new();
    for eng in [&mut h.tx_engine, &mut h.rx_engine] {
        eng.register_plugin(Box::new(BpskPlugin::new())).unwrap();
        eng.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))
            .unwrap();
    }
    // Guard the premise that makes this test what it is: QPSK must publish no template, or this is
    // just another BPSK case wearing a different name.
    assert!(
        openpulse_core::plugin::ModulationPlugin::preamble_template(
            &qpsk_plugin::QpskPlugin::new(),
            &openpulse_core::plugin::ModulationConfig {
                mode: "QPSK500".into(),
                ..Default::default()
            }
        )
        .is_none(),
        "QPSK now publishes a preamble template, so this no longer covers the energy-only path"
    );

    for lead in [40_000usize, 80_000] {
        let mut h2 = ChannelSimHarness::new();
        for eng in [&mut h2.tx_engine, &mut h2.rx_engine] {
            eng.register_plugin(Box::new(BpskPlugin::new())).unwrap();
            eng.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))
                .unwrap();
        }
        h2.tx_engine
            .transmit_with_fec_mode(
                b"no template probe",
                "QPSK500",
                openpulse_core::fec::FecMode::Rs,
                None,
            )
            .expect("transmit");
        h2.route_embedded_in_capture(&hot, lead, 40_000, 0.3);

        // #1066: bound the search in WORK, not wall clock — the same input decodes 5/5 idle and
        // 0/5 on eight busy cores, and debug-vs-release is a ~5x speed proxy for that. Chosen to
        // reconcile the #1058 family (PR #1070), not derived.
        h2.rx_engine.set_deterministic_scan_positions(Some(8_000));
        h2.rx_engine.set_deterministic_max_iterations(Some(64_000));
        let got = h2
            .rx_engine
            .receive_with_fec_mode_timeout(
                "QPSK500",
                openpulse_core::fec::FecMode::Rs,
                None,
                Duration::from_millis(40_000),
            )
            .unwrap_or_else(|e| {
                panic!(
                    "lead {lead}: a no-template mode must still decode through a saturating floor: \
                     {e} — after {} settle condemnations. A condemnation-triggered gate raise \
                     starves this path, because nothing here suppresses the noise settles that \
                     drive it.",
                    h2.rx_engine.settle_condemnations()
                )
            });
        assert_eq!(String::from_utf8_lossy(&got), "no template probe");
    }
    drop(h);
}
