//! The BPSK carrier estimate must not land on a preamble sideband (REQ-PHY-03).
//!
//! On air (run f2j, 2 m, 2026-11-06) station A's cold trigger settle read +7.21, −24.86 and
//! −25.31 Hz on B's first three SL2 frames against a carrier at about −9 Hz, so each decode ran past
//! the ACK deadline and B abandoned a message. The coarse stage was a 12.5 Hz Goertzel picket grid:
//! a long window's line is heard only through sidelobes, and at some offsets a structural line of the
//! squared `--++` preamble out-scores the carrier. Design, measurements and the gate numbering used
//! below: `docs/dev/design/bpsk31-settle-sideband.md`.
//!
//! Every gate runs the engine's own settle (`afc_mini_settle` and its convergence check, through the
//! `settle_probe` instrument) or its production receive entry, never a copy of either. Where a gate
//! needs the shipped estimator too it is reconstructed BY REFERENCE (`common::ref_bpsk`): the
//! product's window and stage 2 with `goertzel_carrier_scan(…, 2, 400, 12.5)` as stage 1.

mod common;

use std::time::{Duration, Instant};

use bpsk_plugin::BpskPlugin;
use common::ref_bpsk::{self, RefBpsk};
use fsk4_plugin::Fsk4Plugin;
use mfsk16_plugin::Mfsk16Plugin;
use ofdm_plugin::OfdmPlugin;
use openpulse_audio::loopback::LoopbackBackend;
use openpulse_config::ModemConfig;
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use openpulse_core::profile::SessionProfile;
use openpulse_modem::capture_replay::{load_corpus, Capture};
use openpulse_modem::engine::ModemEngine;
use pilot_plugin::PilotPlugin;
use psk8_plugin::Psk8Plugin;
use qam64_plugin::Qam64Plugin;
use qpsk_plugin::QpskPlugin;
use scfdma_plugin::ScFdmaPlugin;

const SR: usize = 8_000;
/// The daemon's read size on the replays the review ran (400 samples = 50 ms).
const READ: usize = 400;
/// The settle tolerance: the design's "right" class.
const RIGHT_HZ: f32 = 2.0;
/// f2j burst 2's onset in the full cut: 40 s of hiss (32 s of the pre-run, 8 s of its own) ahead.
const F2J2_ONSET: usize = 40 * SR;
/// The short cuts start this far ahead of the frame's measured level step.
const CUT_LEAD: usize = 4_000;
/// The corpus capture `ft991a-ota-frame-bpsk31-rs.wav`'s frame onset (its level step).
const FT991A_ONSET: usize = 58_900;
/// The recorded idle the synthetic gates embed frames in (the design's noise (1)).
const IDLE: &str = "ic9700-idle-wide-500hz-control.wav";

/// Which coarse stage the BPSK plugin runs.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Estimator {
    /// The product: the dense stage.
    Dense,
    /// The shipped picket grid, by reference.
    Picket,
    /// The design's measured prototype, a 0.25 Hz Goertzel grid.
    Prototype,
}

fn bpsk(est: Estimator) -> Box<dyn ModulationPlugin> {
    match est {
        Estimator::Dense => Box::new(BpskPlugin::new()),
        Estimator::Picket => Box::new(RefBpsk::new(ref_bpsk::picket)),
        Estimator::Prototype => Box::new(RefBpsk::new(ref_bpsk::prototype)),
    }
}

/// The engine as `server::run` builds it from the on-air `config-a.toml` (review item 7).
///
/// Nine plugins in `server::run`'s order; CE-SSB on; the notch exactly as the daemon configures it
/// from `ModemConfig`'s defaults (the on-air config left them in place: `notch_enabled = true`,
/// persistence 0); AGC off; the `fast` OTA session started with no lock. A harness with an SL2 lock,
/// `accumulate_capture(Some("BPSK31"))` and five plugins decoded nothing on these recordings, with the
/// fix or without it, so the construction is part of the gate.
fn daemon_like_engine(est: Estimator) -> ModemEngine {
    let cfg = ModemConfig::default();
    let mut e = ModemEngine::new(Box::new(LoopbackBackend::new()));
    e.register_plugin(bpsk(est)).expect("BPSK");
    e.register_plugin(Box::new(Fsk4Plugin::new()))
        .expect("FSK4");
    e.register_plugin(Box::new(Mfsk16Plugin::new()))
        .expect("MFSK16");
    e.register_plugin(Box::new(OfdmPlugin::new()))
        .expect("OFDM");
    e.register_plugin(Box::new(Psk8Plugin::new()))
        .expect("8PSK");
    e.register_plugin(Box::new(Qam64Plugin::new()))
        .expect("64QAM");
    e.register_plugin(Box::new(QpskPlugin::new()))
        .expect("QPSK");
    e.register_plugin(Box::new(ScFdmaPlugin::new()))
        .expect("SC-FDMA");
    e.register_plugin(Box::new(PilotPlugin::new()))
        .expect("PILOT");
    e.set_cessb_enabled(cfg.cessb_enabled);
    assert!(cfg.notch_enabled, "the notch ships on, and A ran with it");
    e.configure_notch(cfg.notch_max, cfg.notch_q, 2000.0);
    e.set_notch_persistence(cfg.notch_persistence);
    e.enable_notch();
    assert!(!cfg.agc_enabled, "A ran with AGC off");
    e.disable_agc();
    e.start_ota_session(SessionProfile::fast());
    e
}

/// The daemon's mode (`cfg.modem.mode`), passed to both receive calls as `server::run` does.
fn daemon_mode() -> String {
    ModemConfig::default().mode
}

/// The production ACK budget from the end of the frame: `ota_ack_timeout_ms` less the daemon's
/// keying allowance (2000 ms), slack (300 ms) and the FSK4 ACK airtime (`server.rs`).
fn production_budget(e: &ModemEngine) -> Duration {
    Duration::from_millis(
        e.ota_ack_timeout_ms()
            .saturating_sub(2_000 + 300 + e.fsk4_ack_airtime_ms()),
    )
}

/// The settle `(fine, credible)` at each window start; a start whose window does not fit is a panic.
fn settles(e: &mut ModemEngine, mode: &str, samples: &[f32], starts: &[usize]) -> Vec<(f32, bool)> {
    starts
        .iter()
        .map(|&s| {
            let (_, fine, ok) = e
                .settle_probe(mode, samples, s)
                .unwrap_or_else(|| panic!("{mode}: no settle window fits at {s}"));
            (fine, ok)
        })
        .collect()
}

/// 13 window starts across the frame's first 0.8 s, as the review sampled f2j burst 4 (item 5).
fn starts_across(onset: usize) -> Vec<usize> {
    (0..13).map(|k| onset + k * 533).collect()
}

fn mean_sq(x: &[f32]) -> f32 {
    x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32
}

/// A BPSK31 frame from the shipped modulator, at `1500 + offset_hz`.
fn bpsk31_frame(offset_hz: f32, payload: &[u8]) -> Vec<f32> {
    bpsk_plugin::modulate::bpsk_modulate(
        payload,
        &ModulationConfig {
            mode: "BPSK31".to_string(),
            sample_rate: SR as u32,
            center_frequency: 1500.0 + offset_hz,
            ..ModulationConfig::default()
        },
    )
    .expect("modulate")
}

/// `frame` in recorded idle at `snr_db` (total-band signal power against noise power), after `lead`.
fn embed(frame: &[f32], idle: &Capture, from: usize, lead: usize, snr_db: f32) -> Vec<f32> {
    let noise = idle.cycled(from, lead + frame.len() + 2 * SR);
    let scale = (mean_sq(frame) / 10f32.powf(snr_db / 10.0) / mean_sq(&noise)).sqrt();
    let mut out: Vec<f32> = noise.iter().map(|n| n * scale).collect();
    for (o, f) in out[lead..].iter_mut().zip(frame) {
        *o += f;
    }
    out
}

const SWEEP_PAYLOAD: &[u8] = b"REQ-PHY-03 settle sideband sweep";
const SWEEP_LEAD: usize = 16_000;
/// The ten offsets where the shipped settle picked a sideband in ~100 % of cells (design table),
/// and the mirror cells of review item 2, where a re-centred pass meets the Nyquist mirror.
const SWEEP_OFFSETS: [f32; 16] = [
    -17.0, -9.0, -8.0, -4.0, -3.0, 3.0, 4.0, 8.0, 9.0, 17.0, -399.0, -350.0, -300.0, 300.0, 350.0,
    399.0,
];
/// Two window starts per cell: on the frame's first sample, and 266 samples in (a sub-symbol phase).
const SWEEP_STARTS: [usize; 2] = [SWEEP_LEAD, SWEEP_LEAD + 266];

/// Gate 1 — f2j burst 2 decodes cold, inside an attempt budget, with the daemon's construction.
///
/// Host-independent: no deadline is set, so the decode-attempt count is the verdict. The patched
/// path decodes the frame in 21 attempts (24 counting the 1600-sample burst ahead of it); the
/// shipped one spent 107 and skipped 222 more past the deadline without decoding. The production
/// deadline itself is gated by the release-profile slow test below.
///
// VERIFIES: REQ-PHY-03
#[test]
fn f2j_burst_2_decodes_cold_within_the_attempt_budget() {
    const MAX_ATTEMPTS: u64 = 30;
    let full = load_corpus("ic9700-f2j-bpsk31-burst2-full.wav").expect("full cut");
    let (decoded, attempts, afc) = replay_full_cut(&full.samples, false);
    assert!(
        decoded,
        "f2j burst 2 did not decode cold ({attempts} attempts)"
    );
    assert!(
        attempts <= MAX_ATTEMPTS,
        "f2j burst 2 cost {attempts} decode attempts (budget {MAX_ATTEMPTS}; measured 24 with the \
         dense stage) — the first settle did not land on the carrier"
    );
    assert!(
        (afc + 9.25).abs() <= RIGHT_HZ,
        "the kept correction {afc:+.2} Hz is not the decoded carrier (−9.25 Hz)"
    );
}

/// Gate 1, the on-air criterion: inside the production ACK deadline. Release profile only, via
/// `scripts/slow-tests.sh settle`: an unoptimised build takes 3.7 s of the 6.18 s budget, so a loaded
/// host could fail it for a reason that is not the receiver's.
///
// VERIFIES: REQ-PHY-03
#[test]
#[ignore = "release-profile deadline gate; run by scripts/slow-tests.sh settle"]
fn f2j_burst_2_decodes_inside_the_production_deadline() {
    let full = load_corpus("ic9700-f2j-bpsk31-burst2-full.wav").expect("full cut");
    let (decoded, attempts, afc) = replay_full_cut(&full.samples, true);
    eprintln!("deadline gate: decoded {decoded}, {attempts} attempts, AFC {afc:+.2} Hz");
    assert!(
        decoded,
        "f2j burst 2 did not decode inside the ACK deadline"
    );
}

/// Replay the full cut through `accumulate_capture` → `ota_decode_burst` as `server::run` does, and
/// report `(decoded, decode attempts, kept AFC)`. With `deadline`, every burst is decoded under the
/// production budget and must skip no attempt past it.
fn replay_full_cut(samples: &[f32], deadline: bool) -> (bool, u64, f32) {
    let mut e = daemon_like_engine(Estimator::Dense);
    let mode = daemon_mode();
    let mut bursts = Vec::new();
    for chunk in samples.chunks(READ) {
        if let Ok(Some(b)) = e.accumulate_capture(Some(&mode), chunk.to_vec()) {
            bursts.push(b);
        }
    }
    let mut decoded = false;
    for b in &bursts {
        if deadline {
            e.set_ota_ack_deadline(Some(Instant::now() + production_budget(&e)));
        }
        let out = e
            .ota_decode_burst(b, "gate", Some(&mode))
            .expect("ota_decode_burst");
        if deadline {
            assert_eq!(
                e.decode_attempts_past_deadline(),
                0,
                "a {}-sample burst ran past the ACK deadline",
                b.samples.len()
            );
        }
        if out.payload.is_some() {
            decoded = true;
            break;
        }
    }
    (decoded, e.decode_attempts(), e.afc_correction_hz())
}

/// Gate 2 — the real trigger settles land on the decoded carrier at every window start.
///
/// The carrier references are independent of both settles: f2j's is the decoded frame's kept AFC on
/// the patched replay (−8.51, −9.25, −9.68; review), f2h's and f2i's the BPSK250-type wide anchor
/// (−3.80, −4.15; design table), the corpus capture's its measured 2fc line (+3.2 Hz, README).
/// Shipped: +7.21 / −24.86 / −25.31, −11.69, −19.81, −12.80.
///
// VERIFIES: REQ-PHY-03
#[test]
fn the_real_first_frames_settle_on_the_carrier() {
    let full = load_corpus("ic9700-f2j-bpsk31-burst2-full.wav").expect("full cut");
    let cut = |name: &str| load_corpus(name).expect("cut").samples;
    let cases: Vec<(&str, Vec<f32>, usize, f32)> = vec![
        (
            "f2j #1",
            cut("ic9700-f2j-bpsk31-burst1-settle.wav"),
            CUT_LEAD,
            -8.51,
        ),
        ("f2j #2", full.samples, F2J2_ONSET, -9.25),
        (
            "f2j #3",
            cut("ic9700-f2j-bpsk31-burst3-settle.wav"),
            CUT_LEAD,
            -9.68,
        ),
        (
            "f2h #1",
            cut("ic9700-f2h-bpsk31-burst1-settle.wav"),
            CUT_LEAD,
            -3.80,
        ),
        (
            "f2i #1",
            cut("ic9700-f2i-bpsk31-burst1-settle.wav"),
            CUT_LEAD,
            -4.15,
        ),
        (
            "corpus ft991a",
            cut("ft991a-ota-frame-bpsk31-rs.wav"),
            FT991A_ONSET,
            3.2,
        ),
    ];
    let mut e = daemon_like_engine(Estimator::Dense);
    let mut wrong = Vec::new();
    for (name, samples, onset, carrier) in &cases {
        let got = settles(&mut e, "BPSK31", samples, &starts_across(*onset));
        for (k, (fine, credible)) in got.iter().enumerate() {
            if !credible || (fine - carrier).abs() > RIGHT_HZ {
                wrong.push(format!(
                    "{name} start {k}: {fine:+.2} Hz (credible {credible})"
                ));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "settles off the decoded carrier by more than {RIGHT_HZ} Hz: {wrong:#?}"
    );
}

/// Gate 3 — the sweep, by reference: the attractor offsets and the Nyquist-mirror cells, SNR 12,
/// through the mini-settle. Shipped: 213 of 240 attractor-offset cells picked a sideband.
///
// VERIFIES: REQ-PHY-03
#[test]
fn the_sideband_attractor_offsets_settle_on_the_carrier() {
    let idle = load_corpus(IDLE).expect("idle");
    let mut e = daemon_like_engine(Estimator::Dense);
    let mut wrong = Vec::new();
    for (i, &off) in SWEEP_OFFSETS.iter().enumerate() {
        let audio = embed(
            &bpsk31_frame(off, SWEEP_PAYLOAD),
            &idle,
            i * 7_919,
            SWEEP_LEAD,
            12.0,
        );
        let got = settles(&mut e, "BPSK31", &audio, &SWEEP_STARTS);
        for ((fine, credible), start) in got.into_iter().zip(SWEEP_STARTS) {
            if !credible || (fine - off).abs() > RIGHT_HZ {
                wrong.push(format!(
                    "{off:+} Hz at {start}: {fine:+.2} (credible {credible})"
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "sweep cells off the carrier: {wrong:#?}");
}

/// Gate 6 — the production FFT form agrees with the measured 0.25 Hz Goertzel prototype, through the
/// same settle, on gate-3 windows. Review: within 0.021 Hz with interpolation, 0.061 Hz on the raw bin.
///
// VERIFIES: REQ-PHY-03
#[test]
fn the_fft_form_matches_the_measured_goertzel_prototype() {
    const TOLERANCE_HZ: f32 = 0.05;
    let idle = load_corpus(IDLE).expect("idle");
    let mut dense = daemon_like_engine(Estimator::Dense);
    let mut proto = daemon_like_engine(Estimator::Prototype);
    let mut worst = 0.0f32;
    for (i, &off) in SWEEP_OFFSETS.iter().enumerate() {
        if ![-9.0f32, -3.0, 3.0, 17.0, -350.0, 399.0].contains(&off) {
            continue;
        }
        let audio = embed(
            &bpsk31_frame(off, SWEEP_PAYLOAD),
            &idle,
            i * 7_919,
            SWEEP_LEAD,
            12.0,
        );
        let d = settles(&mut dense, "BPSK31", &audio, &SWEEP_STARTS[..1])[0].0;
        let p = settles(&mut proto, "BPSK31", &audio, &SWEEP_STARTS[..1])[0].0;
        eprintln!("{off:+}: dense {d:+.3}, prototype {p:+.3}");
        worst = worst.max((d - p).abs());
    }
    assert!(
        worst <= TOLERANCE_HZ,
        "the FFT form drifted {worst:.3} Hz from the measured prototype (tolerance {TOLERANCE_HZ})"
    );
}

/// The settle's acceptance on windows that hold no frame, for one estimator: `(accepted, windows)`.
fn noise_acceptance(est: Estimator, mode: &str, samples: &[f32], step: usize) -> (usize, usize) {
    let mut e = daemon_like_engine(est);
    let mut starts = Vec::new();
    let mut s = 0;
    while e.settle_probe(mode, samples, s).is_some() {
        starts.push(s);
        s += step;
    }
    let accepted = settles(&mut e, mode, samples, &starts)
        .iter()
        .filter(|(_, ok)| *ok)
        .count();
    (accepted, starts.len())
}

/// Gate 7 — the behaviour change on noise, stated in numbers and held there (review item 1).
///
/// A dense peak is stable under re-centring, so a noise settle now passes the convergence check the
/// picket's jitter used to fail. Both estimators run on the same windows, so the change is measured in
/// every run, and a coarse-stage change that moves either count out of its band is a change to what
/// the veto and the condemnation recovery see (gates 8–9).
fn assert_noise_band(
    label: &str,
    mode: &str,
    samples: &[f32],
    step: usize,
    picket_max: usize,
    dense_min: usize,
) {
    let (p, n) = noise_acceptance(Estimator::Picket, mode, samples, step);
    let (d, _) = noise_acceptance(Estimator::Dense, mode, samples, step);
    eprintln!("{label} {mode}: shipped accepted {p}/{n}, dense {d}/{n}");
    assert!(
        p <= picket_max,
        "{label} {mode}: the shipped estimator accepted {p}/{n} noise settles (≤ {picket_max})"
    );
    assert!(
        d >= dense_min,
        "{label} {mode}: the dense estimator accepted {d}/{n} noise settles, expected ≥ {dense_min} \
         — the behaviour change the design states has moved; re-measure gates 8–9 before trusting it"
    );
}

// Measured 2026-11-07: shipped 0/40, dense 38/40 (review, every 3000 samples: 0/118 vs 113/118).
// VERIFIES: REQ-PHY-03
#[test]
fn noise_settles_converge_on_the_idle_corpus_bpsk31() {
    let idle = load_corpus(IDLE).expect("idle");
    assert_noise_band("corpus idle", "BPSK31", &idle.samples, 9_000, 4, 34);
}

// Measured 2026-11-07: shipped 0/19, dense 17/19 (review, on the 1–48 s hiss: 2/120 vs 116/120).
// VERIFIES: REQ-PHY-03
#[test]
fn noise_settles_converge_on_the_f2j_hiss_bpsk31() {
    let full = load_corpus("ic9700-f2j-bpsk31-burst2-full.wav").expect("full cut");
    let hiss = &full.samples[..F2J2_ONSET - SR];
    assert_noise_band("f2j hiss", "BPSK31", hiss, 16_000, 4, 15);
}

// Measured 2026-11-07: shipped 0/39, dense 37/39 (review: 5/120 vs 99/120).
// VERIFIES: REQ-PHY-03
#[test]
fn noise_settles_converge_on_the_f2j_hiss_bpsk250() {
    let full = load_corpus("ic9700-f2j-bpsk31-burst2-full.wav").expect("full cut");
    let hiss = &full.samples[..F2J2_ONSET - SR];
    assert_noise_band("f2j hiss", "BPSK250", hiss, 8_000, 4, 32);
}

/// Gate 10's interference: a CW tone at fc + 137 Hz, or a second BPSK31 frame at +200 Hz.
#[derive(Clone, Copy, Debug)]
enum Interferer {
    /// A tone `dB` above (positive) or below the frame's power.
    Tone(f32),
    /// A second BPSK31 frame at +200 Hz, at the frame's power and timing.
    SecondFrame,
}

const TONE_HZ: f32 = 1500.0 + 137.0;
const SECOND_FRAME_HZ: f32 = 200.0;
const CONTROL_OFFSET_HZ: f32 = -9.0;
const CONTROL_LEAD: usize = 8 * SR;

/// The gate-10 capture: a BPSK31 frame at −9 Hz, SNR 12, in recorded idle, plus the interferer over
/// the whole capture (a tone) or over the frame (a second frame).
fn interference_capture(i: Interferer) -> Vec<f32> {
    let idle = load_corpus(IDLE).expect("idle");
    let frame = bpsk31_frame(CONTROL_OFFSET_HZ, SWEEP_PAYLOAD);
    let p = mean_sq(&frame);
    let mut audio = embed(&frame, &idle, 3_001, CONTROL_LEAD, 12.0);
    match i {
        Interferer::Tone(db) => {
            let a = (2.0 * p * 10f32.powf(db / 10.0)).sqrt();
            for (k, s) in audio.iter_mut().enumerate() {
                *s += a * (2.0 * std::f32::consts::PI * TONE_HZ * k as f32 / SR as f32).cos();
            }
        }
        Interferer::SecondFrame => {
            let other = bpsk31_frame(SECOND_FRAME_HZ, b"a second station, same rung, +200 Hz");
            for (s, o) in audio[CONTROL_LEAD..].iter_mut().zip(&other) {
                *s += o;
            }
        }
    }
    audio
}

/// What the decoder sees: `audio` through `accumulate_capture` (the daemon's notch at the input seam),
/// as the flushed burst holding the frame, and the frame's onset in it.
///
/// The burst's position in the input is found by matching its head against the input (the notch
/// only touches narrow out-of-band lines, so the match is sharp), searched around where the flush
/// arithmetic puts it.
fn notched_burst(e: &mut ModemEngine, audio: &[f32]) -> (Vec<f32>, usize) {
    let mode = daemon_mode();
    let mut fed = 0usize;
    let mut found = None;
    let head = 2_048;
    for chunk in audio.chunks(READ) {
        fed += chunk.len();
        let Ok(Some(b)) = e.accumulate_capture(Some(&mode), chunk.to_vec()) else {
            continue;
        };
        if b.samples.len() < head {
            continue;
        }
        let nominal = fed.saturating_sub(b.samples.len());
        let (lo, hi) = (
            nominal.saturating_sub(8_000),
            (nominal + 8_000).min(fed - head),
        );
        let score = |a: usize| -> f32 {
            let x = &audio[a..a + head];
            let y = &b.samples[..head];
            let dot: f32 = x.iter().zip(y).map(|(p, q)| p * q).sum();
            dot / (mean_sq(x) * mean_sq(y)).sqrt().max(f32::MIN_POSITIVE)
        };
        let Some(start) = (lo..=hi).max_by(|&a, &c| score(a).total_cmp(&score(c))) else {
            continue;
        };
        if start <= CONTROL_LEAD && CONTROL_LEAD < start + b.samples.len() {
            found = Some((b.samples, CONTROL_LEAD - start));
        }
    }
    for _ in 0..8 {
        if let Ok(Some(b)) = e.accumulate_capture(Some(&mode), vec![0.0; READ]) {
            if found.is_none() && b.samples.len() > CONTROL_LEAD {
                found = Some((b.samples, CONTROL_LEAD));
            }
        }
    }
    found.expect("no flushed burst holds the frame")
}

/// Gate 10 — tone and two-signal controls, through the daemon's notch (review item 8).
///
/// The notch cannot help here, and the gate proves why from the product's own instruments: the
/// receiver protects its candidates' band (1250–1750 Hz at this rung), so an interferer at
/// fc + 137 Hz or fc + 200 Hz is never notched. What the settle then does, measured 2026-11-07 on the
/// notched burst at three window starts (shipped picket grid in brackets):
/// - tone +6 dB: locks the tone at +137 Hz, credible (picket: the tone at one start, a credible
///   +71.9 at another, rejected at the third);
/// - tone 0 dB: a credible wrong answer at +69.3…+70.8 Hz, between the carrier and the tone, from
///   their cross term — **a known limit**, not worse in kind than the +6 dB tone lock both
///   estimators share (picket: the tone, credible, at one start, rejected at two);
/// - tone −6 dB: right, −9 Hz (picket: a credible sideband, −24.6, at one start, rejected at two);
/// - a second BPSK31 frame at +200 Hz: the cross term at +95.5 Hz, credible — **a known limit**
///   (picket: the other station's carrier at +200, credible).
///
// VERIFIES: REQ-PHY-03
#[test]
fn tone_and_two_signal_controls_through_the_notch() {
    let starts = [0usize, 533, 1066];
    let cases: [(Interferer, &str); 4] = [
        (Interferer::Tone(6.0), "tone +6 dB"),
        (Interferer::Tone(0.0), "tone 0 dB"),
        (Interferer::Tone(-6.0), "tone -6 dB"),
        (Interferer::SecondFrame, "second frame +200 Hz"),
    ];
    let mut failures = Vec::new();
    for (i, label) in cases {
        let audio = interference_capture(i);
        for est in [Estimator::Dense, Estimator::Picket] {
            let mut e = daemon_like_engine(est);
            let (burst, onset) = notched_burst(&mut e, &audio);
            let seen = e.notch_freqs_seen();
            let (_, lo_max, hi_min, _) = e.notch_protect_extremes().expect("the notch ran");
            for f in [
                TONE_HZ,
                1500.0 + SECOND_FRAME_HZ,
                1500.0 + CONTROL_OFFSET_HZ,
            ] {
                assert!(
                    lo_max <= f && f <= hi_min && !seen.iter().any(|&s| (s - f).abs() < 15.0),
                    "{label}: {f} Hz was not protected throughout ({lo_max}–{hi_min}) or was \
                     notched ({seen:?}) — the notch's reach changed; re-measure this gate"
                );
            }
            let at: Vec<usize> = starts.iter().map(|s| onset + s).collect();
            let got = settles(&mut e, "BPSK31", &burst, &at);
            eprintln!("{label} {est:?}: {got:?}");
            if est != Estimator::Dense {
                continue;
            }
            let ok = |pred: &dyn Fn(f32) -> bool| got.iter().all(|&(f, c)| c && pred(f));
            let pinned = match i {
                Interferer::Tone(db) if db > 3.0 => ok(&|f| (f - 137.0).abs() <= RIGHT_HZ),
                Interferer::Tone(db) if db > -3.0 => ok(&|f| (40.0..=80.0).contains(&f)),
                Interferer::Tone(_) => ok(&|f| (f - CONTROL_OFFSET_HZ).abs() <= RIGHT_HZ),
                Interferer::SecondFrame => {
                    ok(&|f| (f - (CONTROL_OFFSET_HZ + SECOND_FRAME_HZ) / 2.0).abs() <= RIGHT_HZ)
                }
            };
            if !pinned {
                failures.push(format!("{label}: dense settles {got:?}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "the interference controls moved from their measured classes: {failures:#?}"
    );
}
