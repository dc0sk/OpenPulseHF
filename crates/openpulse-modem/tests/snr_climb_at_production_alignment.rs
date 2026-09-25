//! The rate controller climbs on a TRUE SNR reading for a frame at a production alignment (#1438).
//!
//! **Why a new harness.** The BPSK timing search locks a quarter symbol early whenever the frame
//! starts at least that far into the slice, which a real burst nearly always does. Until #1438 PR1
//! the SNR estimate read the crossfade-cancelled stream, correct only on the exact symbol boundary,
//! so at that lock it read a near-constant (slope 0.09 dB/dB; ≈ 3 dB at −0.28 of a symbol on BPSK250)
//! and `ClimbOnSnr` could not fire on SL2–SL5. The three fixtures found that assert on the controller
//! at a non-zero onset place the frame at a multiple of the symbol period
//! (`ota_burst_sizes_for_the_fec`'s 4000, `coded_arm_scans_onset`'s 4032,
//! `ota_production_capture_path`'s 800-sample chunks — all multiples of 32), which forces the lock
//! onto the boundary, the one phase where the old estimator was right. So none could see it.
//! (DCD-triggered twin-daemon fixtures do land off the grid, but assert nothing about the
//! controller's SNR decision.)
//!
//! This one places the frame `k ∈ [n/4, n)` samples past a symbol-period multiple, asserted, so it
//! cannot drift back onto the boundary. The session is anchored at SL5 through
//! `SessionProfile::initial_level`, NOT `ota_lock_level`, which would pin the level and forbid the
//! very climb under test. Noise has a fixed sigma derived from the frame alone, because
//! `AwgnChannel` normalises to the whole buffer and a lead-in would move the frame's own SNR.
//!
//! What this does NOT cover: failed decodes still pass no reading — that is pinned by
//! `ota_rate_decision_events::the_decision_event_carries_the_snr_it_acted_on`.

use openpulse_core::fec::FecMode;
use openpulse_core::ota_rate::RateDecision;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::engine::ModemEngine;
use openpulse_modem::pipeline::AudioSamples;
use openpulse_modem::EngineEvent;

const MODE: &str = "BPSK250";
const N: usize = 32;
const SESSION: &str = "sess-1438-climb";
const LEAD_BASE: usize = 4_000;

fn coded_frame(payload: &[u8]) -> Vec<f32> {
    let lb = openpulse_audio::LoopbackBackend::new();
    let mut tx = ModemEngine::new(Box::new(lb.clone_shared()));
    tx.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .expect("register bpsk");
    tx.transmit_with_fec_mode(payload, MODE, FecMode::Rs, None)
        .expect("transmit coded");
    lb.drain_samples()
}

/// The frame placed `lead` samples into a burst, plus noise at `snr_db` relative to the FRAME's own
/// power (Box–Muller over an LCG, deterministic in `seed`).
fn burst(frame: &[f32], lead: usize, snr_db: f32, seed: u64) -> AudioSamples {
    let p = frame.iter().map(|x| x * x).sum::<f32>() / frame.len() as f32;
    let sigma = (p / 10f32.powf(snr_db / 10.0)).sqrt();
    let mut st = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1);
    let mut u = || -> f32 {
        st = st
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((st >> 11) as f32 / (1u64 << 53) as f32).clamp(1e-9, 1.0 - 1e-9)
    };
    let mut samples = vec![0.0f32; lead];
    samples.extend_from_slice(frame);
    samples.extend(std::iter::repeat_n(0.0, 2_000));
    for x in samples.iter_mut() {
        let (a, b) = (u(), u());
        *x += sigma * (-2.0 * a.ln()).sqrt() * (std::f32::consts::TAU * b).cos();
    }
    AudioSamples { samples }
}

fn engine_at_sl5() -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(openpulse_audio::LoopbackBackend::new()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .expect("register bpsk");
    e.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))
        .expect("register qpsk");
    let mut profile = SessionProfile::hpx_hf();
    profile.initial_level = SpeedLevel::Sl5;
    e.start_ota_session(profile);
    e
}

/// The decision the controller took for the one decoded frame in `burst`.
fn decide(burst: &AudioSamples) -> (Option<String>, Option<f32>, RateDecision) {
    let mut engine = engine_at_sl5();
    let mut rx = engine.subscribe();
    let _ = engine.ota_decode_burst(burst, SESSION, None);
    let mut out = None;
    while let Ok(ev) = rx.try_recv() {
        if let EngineEvent::OtaRateDecision {
            decoded_level,
            snr_db,
            decision,
            ..
        } = ev
        {
            out = Some((decoded_level.map(|l| l.name()), snr_db, decision));
        }
    }
    out.expect("ota_decode_burst must emit a decision")
}

/// THE GATE: a clean BPSK250 frame at a production alignment is read at its true SNR and climbs.
///
/// 15 dB is well above SL5's 9 dB ceiling, so a truthful reading must fire `ClimbOnSnr` on the first
/// decoded frame. With the pre-#1438 estimator the k = 8 frame read 7.81 dB, below the 9 dB
/// ceiling, and the controller held; the run stops there, so the other `k` were not observed.
#[test]
fn a_clean_frame_at_a_production_alignment_climbs_on_snr() {
    let ceiling = SessionProfile::hpx_hf()
        .snr_ceiling_for_level(SpeedLevel::Sl5)
        .expect("SL5 has a ceiling");
    let payload: Vec<u8> = (0..120u32).map(|i| (i * 37 % 251) as u8).collect();
    let frame = coded_frame(&payload);
    assert_eq!(
        LEAD_BASE % N,
        0,
        "the base lead must sit on a symbol boundary"
    );
    for (trial, k) in [8usize, 13, 19, 24, 31].into_iter().enumerate() {
        assert!(
            (N / 4..N).contains(&(k % N)),
            "k = {k} would put the lock on the boundary, the one phase the old estimator read"
        );
        let (level, snr, decision) =
            decide(&burst(&frame, LEAD_BASE + k, 15.0, 100 + trial as u64));
        println!("k = {k:2}: decoded {level:?}, snr {snr:?}, {decision:?}");
        assert_eq!(
            level.as_deref(),
            Some("SL5"),
            "k = {k}: the SL5 frame must decode"
        );
        let snr = snr.expect("a decoded frame carries a reading");
        assert!(
            snr >= ceiling,
            "k = {k}: a 15 dB frame read {snr:.2} dB, below SL5's {ceiling} dB ceiling"
        );
        assert_eq!(
            decision,
            RateDecision::ClimbOnSnr,
            "k = {k}: a truthful {snr:.2} dB reading above the ceiling must climb"
        );
    }
}

/// REPORTING, not a gate (#1438 PR1 criterion 3): on `moderate_f1`, the fraction of decoded SL5
/// frames that fire `ClimbOnSnr`, and the readings, at true 7 / 9 / 11 / 13 dB. The estimate's mean
/// crosses SL5's ceiling near 11 dB there; below it a climb fires only on a frame whose fade happened
/// to read high. The ladder's failure path bounds that cost, so this is recorded, not killed.
/// Watterson's `snr_db` normalises over the whole buffer, of which the lead-in and tail are ≈ 8 %,
/// so the nominal SNRs here are ≈ 0.3–0.4 dB optimistic for the frame itself.
#[test]
#[ignore = "reporting run for #1438 criterion 3; ~minutes"]
fn fade_climb_fraction_at_a_production_alignment() {
    use openpulse_channel::{watterson::WattersonChannel, ChannelModel, WattersonConfig};
    let payload: Vec<u8> = (0..120u32).map(|i| (i * 37 % 251) as u8).collect();
    let frame = coded_frame(&payload);
    for snr in [7.0f32, 9.0, 11.0, 13.0] {
        let (mut decoded, mut climbs, mut readings) = (0u32, 0u32, Vec::new());
        let frames = 48u64;
        for f in 0..frames {
            let k = N / 4 + (f as usize * 7) % (3 * N / 4);
            let mut buf = vec![0.0f32; LEAD_BASE + k];
            buf.extend_from_slice(&frame);
            buf.extend(std::iter::repeat_n(0.0, 2_000));
            let mut cfg = WattersonConfig::moderate_f1(Some(700 + f));
            cfg.snr_db = snr;
            let faded = WattersonChannel::new(cfg).expect("watterson").apply(&buf);
            let (level, reading, decision) = decide(&AudioSamples { samples: faded });
            if level.as_deref() == Some("SL5") {
                decoded += 1;
                if let Some(r) = reading {
                    readings.push(r);
                }
                if decision == RateDecision::ClimbOnSnr {
                    climbs += 1;
                }
            }
        }
        readings.sort_by(f32::total_cmp);
        let q = |p: f32| {
            readings
                .get(((readings.len() as f32 - 1.0) * p) as usize)
                .copied()
        };
        println!(
            "moderate_f1 {snr:>4} dB: decoded {decoded}/{frames}, ClimbOnSnr {climbs}/{decoded}, \
             reading p10 {:?} p50 {:?} p90 {:?}",
            q(0.1),
            q(0.5),
            q(0.9)
        );
    }
}
