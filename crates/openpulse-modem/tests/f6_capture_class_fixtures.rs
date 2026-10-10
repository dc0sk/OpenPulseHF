//! #1062 F6: synthetic fixtures for the defect classes the real-capture replay rows pinned.
//!
//! The four `#[ignore]`d rows of `capture_replay_corpus.rs` replay frames recorded on air, so they
//! go dark whenever the wire changes (#1148 already darkened them; the PN preamble would again).
//! Each class they pinned is reproduced here with a FRESHLY MODULATED frame in the same recording's
//! own idle floor, at the recorded onset, carrier and level, so the fixture follows the wire. Every
//! fixture runs on both `BPSK250` and the `BPSK250-PN` candidate. Plan, sabotages and results:
//! `docs/dev/design/pn-preamble.md`, *F6*.

use std::time::Duration;

use bpsk_plugin::BpskPlugin;
use openpulse_core::fec::FecMode;
use openpulse_modem::capture_replay::{load_corpus, Capture};
use openpulse_modem::channel_sim::ChannelSimHarness;

mod common;
use common::saturating_floor as hot_fixture;

/// The #1021 recording: a coded `BPSK250|rs` frame that arrived byte-perfect and did not decode.
const CAPTURE_1021: &str = "ic9700-frame-bpsk250-rs-whitened.wav";
/// Recorded burst onset, in samples (the settle the fixed receiver reached on the recording).
const ONSET: usize = 82_304;
/// Start of the recorded idle after the burst (the burst ends at ≈ 18.6 s).
const TAIL_START: usize = 152_000;
/// A span wholly inside the recorded burst, for its level.
const BURST: std::ops::Range<usize> = 84_000..148_000;
/// Recorded carrier offset from 1500 Hz (README: 1502.42 Hz).
const CARRIER_OFFSET_HZ: f32 = 2.42;
/// The payload the recording carried.
const PAYLOAD: &[u8] = b"DUALCAP TEST 1";
/// Trailing recorded idle after the frame.
const TRAIL: usize = 40_000;
/// `EnergyGate::ABS_THRESHOLD`: the gate's floor until it holds 32 windows of history.
const GATE_ABS_MEAN_SQ: f32 = 0.0001;
/// The replaced pin's condemnation bound (`the_settle_recovery_reaches_the_frame_without_crawling`).
const MAX_CONDEMNATIONS: u64 = 2;

/// The two arms: the shipped preamble and the flag day's.
const ARMS: [&str; 2] = ["BPSK250", "BPSK250-PN"];

fn plugin(mode: &str) -> BpskPlugin {
    if mode.ends_with("-PN") {
        BpskPlugin::pn_candidate()
    } else {
        BpskPlugin::new()
    }
}

fn harness(mode: &str) -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    for eng in [&mut h.tx_engine, &mut h.rx_engine] {
        eng.register_plugin(Box::new(plugin(mode))).unwrap();
    }
    h
}

fn mean_sq(x: &[f32]) -> f32 {
    x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32
}

/// The #1021 recording split into what the fixture reuses, with its measured levels.
struct Floor1021 {
    lead: Vec<f32>,
    tail: Vec<f32>,
    floor_mean_sq: f32,
    signal_mean_sq: f32,
}

fn floor_1021() -> Floor1021 {
    let c: Capture = load_corpus(CAPTURE_1021)
        .unwrap_or_else(|e| panic!("corpus file {CAPTURE_1021} must load: {e}"));
    let lead = c.samples[..ONSET].to_vec();
    let tail = c.samples[TAIL_START..].to_vec();
    let floor_mean_sq = (mean_sq(&lead) * lead.len() as f32 + mean_sq(&tail) * tail.len() as f32)
        / (lead.len() + tail.len()) as f32;
    let signal_mean_sq = mean_sq(&c.samples[BURST]) - floor_mean_sq;
    Floor1021 {
        lead,
        tail,
        floor_mean_sq,
        signal_mean_sq,
    }
}

/// Embed a freshly modulated frame in the #1021 recording's floor, as it was recorded, and decode.
fn decode_in_the_1021_floor(mode: &str, fec: FecMode) -> (Result<Vec<u8>, String>, u64) {
    let f = floor_1021();
    let mut h = harness(mode);
    h.tx_engine
        .transmit_with_fec_mode(PAYLOAD, mode, fec, None)
        .expect("transmit");
    h.route_over_recorded(
        &f.lead,
        &f.tail,
        &f.lead[..TRAIL],
        f.signal_mean_sq,
        CARRIER_OFFSET_HZ,
    );
    // #1066: bound the search in work, not wall clock (the #1058 family's budget).
    h.rx_engine.set_deterministic_scan_positions(Some(8_000));
    h.rx_engine.set_deterministic_max_iterations(Some(64_000));
    let got = h
        .rx_engine
        .receive_with_fec_mode_timeout(mode, fec, None, Duration::from_millis(40_000))
        .map_err(|e| e.to_string());
    let condemnations = h.rx_engine.settle_condemnations();
    eprintln!(
        "{mode} {fec:?} in the #1021 floor: decoded {}, condemnations {condemnations}, rho rejections {}",
        got.is_ok(),
        h.rx_engine.rho_rejected_settles()
    );
    (got, condemnations)
}

/// The fixture is only a #1021 reproduction while the recording still measures as it did: a floor
/// above the gate's absolute threshold (the premise of #1021) and below its ceiling (else it is the
/// #1045 class), a burst clearly above it, and pure idle on both sides of the burst.
#[test]
fn the_1021_recording_still_measures_as_the_fixtures_assume() {
    let f = floor_1021();
    assert!(
        f.floor_mean_sq > 2.0 * GATE_ABS_MEAN_SQ
            && f.floor_mean_sq < hot_fixture::GATE_CEILING_MEAN_SQ,
        "floor {:.2e} is no longer between the gate's absolute threshold and its ceiling",
        f.floor_mean_sq
    );
    assert!(
        f.signal_mean_sq > 2.0 * f.floor_mean_sq,
        "burst signal {:.2e} against floor {:.2e}: the burst span no longer holds the frame",
        f.signal_mean_sq,
        f.floor_mean_sq
    );
    let (lead, tail) = (mean_sq(&f.lead), mean_sq(&f.tail));
    assert!(
        lead < 1.5 * tail && tail < 1.5 * lead,
        "idle before ({lead:.2e}) and after ({tail:.2e}) the burst differ: one span holds signal"
    );
}

/// Replaces `the_real_on_air_frame_decodes` and `the_settle_recovery_reaches_the_frame_without_crawling`.
///
/// The coded frame in the floor that tripped #1021 (above the energy gate's absolute threshold, so
/// noise could pass the gate before it had history) decodes without condemnations. The bound is the
/// replaced pin's. This is NOT a pin of the #1021 recovery: measured, no settle in this floor is
/// condemned with or without the veto, so the recovery never runs here. That pin is
/// [`the_recovery_reaches_the_frame_when_noise_passes_the_veto`].
#[test]
fn a_coded_frame_in_the_1021_floor_decodes_without_crawling() {
    for mode in ARMS {
        let (got, condemnations) = decode_in_the_1021_floor(mode, FecMode::Rs);
        let got = got.unwrap_or_else(|e| {
            panic!("{mode}: a coded frame in the #1021 floor must decode: {e} ({condemnations} condemnations)")
        });
        assert_eq!(String::from_utf8_lossy(&got), "DUALCAP TEST 1", "{mode}");
        assert!(
            condemnations <= MAX_CONDEMNATIONS,
            "{mode}: {condemnations} settle condemnations (~{} wasted decodes); the recovery is \
             re-settling on ground it already condemned (#1021, #1040)",
            condemnations * 18
        );
    }
}

/// Replaces `a_real_on_air_frame_decodes_end_to_end`: the uncoded control in the same floor, so a
/// coded failure above cannot be the floor or the placement.
#[test]
fn an_uncoded_frame_in_the_1021_floor_decodes() {
    for mode in ARMS {
        let (got, condemnations) = decode_in_the_1021_floor(mode, FecMode::None);
        let got = got.unwrap_or_else(|e| {
            panic!("{mode}: the uncoded control must decode: {e} ({condemnations} condemnations)")
        });
        assert_eq!(String::from_utf8_lossy(&got), "DUALCAP TEST 1", "{mode}");
    }
}

/// The #1045 / #1049 pin (`the_receiver_never_settles_on_a_saturating_noise_floor`) on the flag
/// day's preamble, with the same fixture by reference. `BPSK250` runs in its own file.
#[test]
fn the_pn_receiver_never_settles_on_a_saturating_noise_floor() {
    let mode = "BPSK250-PN";
    let hot = load_corpus(hot_fixture::CORPUS).expect("hot idle corpus");
    assert!(hot.mean_sq() > hot_fixture::GATE_CEILING_MEAN_SQ);
    for lead in hot_fixture::LEADS {
        let mut h = harness(mode);
        h.tx_engine
            .transmit_with_fec_mode(hot_fixture::PAYLOAD, mode, FecMode::Rs, None)
            .expect("transmit");
        h.route_embedded_in_capture(&hot, lead, hot_fixture::TRAIL, hot_fixture::EMBED_LEVEL);
        h.rx_engine.set_deterministic_scan_positions(Some(8_000));
        h.rx_engine.set_deterministic_max_iterations(Some(64_000));
        let got = h
            .rx_engine
            .receive_with_fec_mode_timeout(
                mode,
                FecMode::Rs,
                None,
                Duration::from_millis(hot_fixture::TIMEOUT_MS),
            )
            .unwrap_or_else(|e| {
                panic!(
                    "{mode} lead {lead}: {e} (condemnations {}, rho rejections {})",
                    h.rx_engine.settle_condemnations(),
                    h.rx_engine.rho_rejected_settles()
                )
            });
        assert_eq!(got, hot_fixture::PAYLOAD, "{mode} lead {lead}");
        let condemnations = h.rx_engine.settle_condemnations();
        eprintln!(
            "{mode} lead {lead} saturating floor: condemnations {condemnations}, rho rejections {}",
            h.rx_engine.rho_rejected_settles()
        );
        assert!(
            condemnations <= 6,
            "{mode} lead {lead}: {condemnations} condemnations; the receiver settles on noise again"
        );
        assert!(
            h.rx_engine.rho_rejected_settles() > 0,
            "{mode} lead {lead}: no settle refused on correlation on a floor that saturates the \
             energy gate, so the veto is inert and the count above proves nothing"
        );
    }
}

/// The #1045 pin (`a_coded_frame_decodes_through_a_saturating_floor`, `capture_replay_corpus.rs`)
/// on the flag day's preamble: the same leads, level and bound.
#[test]
fn a_pn_coded_frame_decodes_through_a_saturating_floor() {
    let mode = "BPSK250-PN";
    let hot = load_corpus(hot_fixture::CORPUS).expect("hot idle corpus");
    assert!(hot.mean_sq() > hot_fixture::GATE_CEILING_MEAN_SQ);
    for lead in [80_000usize, 120_000] {
        let mut h = harness(mode);
        h.tx_engine
            .transmit_with_fec_mode(b"saturated gate probe", mode, FecMode::Rs, None)
            .expect("transmit");
        h.route_embedded_in_capture(&hot, lead, 40_000, 0.3);
        h.rx_engine.set_deterministic_scan_positions(Some(8_000));
        h.rx_engine.set_deterministic_max_iterations(Some(64_000));
        let got = h
            .rx_engine
            .receive_with_fec_mode_timeout(mode, FecMode::Rs, None, Duration::from_millis(40_000))
            .unwrap_or_else(|e| {
                panic!(
                    "{mode} lead {lead}: {e} after {} condemnations",
                    h.rx_engine.settle_condemnations()
                )
            });
        assert_eq!(String::from_utf8_lossy(&got), "saturated gate probe");
        let c = h.rx_engine.settle_condemnations();
        assert!(
            c <= 12,
            "{mode} lead {lead}: decoded after {c} condemnations"
        );
    }
}

/// THE #1021 PIN: the settle recovery must not re-settle on the anchor it just condemned.
///
/// The recovery is load-bearing only where noise reaches the settle. On BPSK the correlation veto
/// keeps it from doing so, and the #1021 floor no longer gets a noise settle at all, so neither the
/// floor fixture above nor any veto-on run can see a recovery that rewinds onto a condemned anchor.
/// With the veto off, the saturating floor does put noise through: measured, the recovery reaches the
/// frame after 66 condemnations on `BPSK250-PN`, and with `unsettle` rewinding to 0 (the pre-#1021
/// code) it livelocks, 3 555 condemnations and no decode. The veto is off here on purpose, as a
/// stand-in for any noise that passes it.
#[test]
fn the_recovery_reaches_the_frame_when_noise_passes_the_veto() {
    let hot = load_corpus(hot_fixture::CORPUS).expect("hot idle corpus");
    assert!(hot.mean_sq() > hot_fixture::GATE_CEILING_MEAN_SQ);
    let lead = hot_fixture::LEADS[0];
    let mut failures = Vec::new();
    for mode in ARMS {
        let mut h = harness(mode);
        h.rx_engine.set_preamble_veto_gate(false);
        h.tx_engine
            .transmit_with_fec_mode(hot_fixture::PAYLOAD, mode, FecMode::Rs, None)
            .expect("transmit");
        h.route_embedded_in_capture(&hot, lead, hot_fixture::TRAIL, hot_fixture::EMBED_LEVEL);
        h.rx_engine.set_deterministic_scan_positions(Some(8_000));
        h.rx_engine.set_deterministic_max_iterations(Some(64_000));
        let got = h.rx_engine.receive_with_fec_mode_timeout(
            mode,
            FecMode::Rs,
            None,
            Duration::from_millis(hot_fixture::TIMEOUT_MS),
        );
        let condemnations = h.rx_engine.settle_condemnations();
        eprintln!(
            "{mode} lead {lead}, veto off: decoded {}, condemnations {condemnations}",
            got.is_ok()
        );
        match got {
            Err(e) => failures.push(format!(
                "{mode}: with noise reaching the settle, the recovery never reached the frame: {e} \
                 after {condemnations} condemnations (#1021: re-settling on a condemned anchor)"
            )),
            Ok(got) if got != hot_fixture::PAYLOAD => {
                failures.push(format!("{mode}: decoded the wrong payload"))
            }
            Ok(_) if condemnations == 0 => failures.push(format!(
                "{mode}: no settle was condemned with the veto off, so the recovery never ran and \
                 this test pins nothing"
            )),
            Ok(_) => {}
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
