//! #1460 round 4b, pre-registered measurements M2 and M3 (`docs/dev/design/failed-burst-ownership.md`).
//!
//! Both drive the daemon's own per-burst path: `accumulate_capture` in daemon-sized reads, then
//! `nack_budget::decode_ota_burst`, which `server::run`'s rx tick calls. Release only:
//! `cargo test --release -p openpulse-daemon --no-default-features --lib ladder_evidence_probe -- --ignored --nocapture`.

use openpulse_audio::LoopbackBackend;
use openpulse_core::cw_id::CwId;
use openpulse_core::fec::free_rs_strengthening;
use openpulse_core::frame::Frame;
use openpulse_core::ota_rate::{NackVerdict, RateDecision};
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::capture_replay::load_corpus;
use openpulse_modem::event::EngineEvent;
use openpulse_modem::ModemEngine;

use crate::nack_budget::{decode_ota_burst, NackBudget};

const FS: usize = 8_000;
/// The daemon's default read: `receive_tick_ms = 50` at 8 kHz.
const TICK: usize = 400;
/// The station's own mode, as in `idle_flicker_is_not_evidence`: its uncoded fallback runs on
/// every failed burst.
const STATION_MODE: &str = "BPSK250";

fn engine() -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(LoopbackBackend::new()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(ofdm_plugin::OfdmPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(mfsk16_plugin::Mfsk16Plugin::new()))
        .unwrap();
    e.register_plugin(Box::new(fsk4_plugin::Fsk4Plugin::new()))
        .unwrap();
    e
}

fn fast_at(level: SpeedLevel) -> SessionProfile {
    let mut p = SessionProfile::fast();
    p.initial_level = level;
    p
}

/// A ladder frame as the ISS sends it at `level` (`server.rs`, `ota_send_with_ptt`).
fn our_frame(level: SpeedLevel) -> Vec<f32> {
    let lb = LoopbackBackend::new();
    let mut tx = ModemEngine::new(Box::new(lb.clone_shared()));
    for p in [
        Box::new(bpsk_plugin::BpskPlugin::new())
            as Box<dyn openpulse_core::plugin::ModulationPlugin>,
        Box::new(qpsk_plugin::QpskPlugin::new()),
        Box::new(ofdm_plugin::OfdmPlugin::new()),
        Box::new(mfsk16_plugin::Mfsk16Plugin::new()),
        Box::new(fsk4_plugin::Fsk4Plugin::new()),
    ] {
        tx.register_plugin(p).unwrap();
    }
    tx.start_ota_session(fast_at(level));
    let body = b"#1460 probe frame";
    let mode = tx.ota_tx_mode().unwrap().to_owned();
    let fec = free_rs_strengthening(tx.ota_tx_fec(), body.len() + Frame::WIRE_OVERHEAD);
    tx.transmit_with_fec_mode(body, &mode, fec, None).unwrap();
    lb.drain_samples()
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// The recorded 250 Hz IC-9700 idle, `secs` long, cycled from shifting offsets as
/// `idle_flicker_is_not_evidence` does so the repeats do not line up.
fn idle(secs: usize) -> Vec<f32> {
    let rec = load_corpus("ic9700-idle-250hz.wav").expect("corpus");
    let mut out = Vec::new();
    let mut offset = 0;
    while out.len() < secs * FS {
        out.extend(rec.cycled(offset, rec.samples.len()));
        offset += 7_919;
    }
    out.truncate(secs * FS);
    out
}

/// Add `signal` at `at` into `input`, scaled to `snr_db` against the idle's RMS. The recording is
/// behind a 250 Hz filter, so its whole power lies in the band.
fn overlay(input: &mut [f32], at: usize, signal: &[f32], snr_db: f32) {
    let g = rms(input) * 10f32.powf(snr_db / 20.0) / rms(signal);
    for (i, s) in signal.iter().enumerate() {
        input[at + i] += g * s;
    }
}

/// One counted burst: the rate decision it drew, the verdict handed to the engine, and whether
/// its ACK was keyed.
#[derive(Debug, Clone, Copy)]
struct Counted {
    at_s: f32,
    decoded: Option<SpeedLevel>,
    decision: RateDecision,
    verdict: NackVerdict,
    keyed: bool,
}

/// Feed `input` through the daemon's per-burst path; return every burst that reached the ladder.
fn run(rx: &mut ModemEngine, budget: &mut NackBudget, input: &[f32]) -> Vec<Counted> {
    let mut events = rx.subscribe();
    let mut out = Vec::new();
    for (k, chunk) in input.chunks(TICK).enumerate() {
        let Ok(Some(burst)) = rx.accumulate_capture(Some(STATION_MODE), chunk.to_vec()) else {
            continue;
        };
        let Ok((_, keyed)) = decode_ota_burst(rx, budget, true, &burst, "peer", STATION_MODE)
        else {
            continue;
        };
        while let Ok(ev) = events.try_recv() {
            if let EngineEvent::OtaRateDecision {
                decoded_level,
                decision,
                verdict,
                ..
            } = ev
            {
                out.push(Counted {
                    at_s: ((k + 1) * TICK) as f32 / FS as f32,
                    decoded: decoded_level,
                    decision,
                    verdict,
                    keyed,
                });
            }
        }
    }
    out
}

fn steps(c: &[Counted]) -> usize {
    c.iter()
        .filter(|c| c.decision == RateDecision::NackStepDown)
        .count()
}

fn muted(c: &[Counted]) -> usize {
    c.iter()
        .filter(|c| c.decoded.is_none() && c.verdict == NackVerdict::Muted)
        .count()
}

fn failed(c: &[Counted]) -> usize {
    c.iter().filter(|c| c.decoded.is_none()).count()
}

/// M2, the #1456 falsifier relaxed: 90 min of the 250 Hz idle at SL2, then a failing SL2 frame.
/// Rule: at most one step. Whether the frame keys a NACK is the budget's property, reported only.
#[test]
#[ignore = "release probe, ~minutes: #1460 M2"]
fn m2_ninety_minutes_of_250hz_idle_at_sl2() {
    let mut rx = engine();
    rx.start_ota_session(fast_at(SpeedLevel::Sl2));
    let mut budget = NackBudget::default();
    let mut input = idle(90 * 60);
    let idle_end = input.len();
    // Time-reversed: our frame's energy and spectrum, and it cannot decode.
    let mut frame = our_frame(SpeedLevel::Sl2);
    frame.reverse();
    let mut tail = idle(frame.len() / FS + 20);
    overlay(&mut tail, 5 * FS, &frame, 6.0);
    input.extend(tail);
    let counted = run(&mut rx, &mut budget, &input);
    let (during, after): (Vec<Counted>, Vec<Counted>) = counted
        .iter()
        .partition(|c| (c.at_s * FS as f32) as usize <= idle_end);
    eprintln!(
        "M2 idle: {} counted failures, {} muted, {} keyed, {} steps",
        failed(&during),
        muted(&during),
        during.iter().filter(|c| c.keyed).count(),
        steps(&during)
    );
    eprintln!(
        "M2 idle, today (every counted failure in the hysteresis): {} steps",
        failed(&during) / 3
    );
    for c in &counted {
        eprintln!("  {c:?}");
    }
    eprintln!("M2 frame: {after:?}");
    eprintln!("M2 level after: {:?}", rx.ota_rx_recommended_level());
    assert!(
        after.iter().any(|c| c.decoded.is_none()),
        "the reversed frame did not reach the ladder: {after:?}"
    );
    assert!(steps(&counted) <= 1, "M2: more than one step: {counted:?}");
}

/// M3: one minute of 20 wpm CW with word gaps at +6 dB, at SL4, then a frame of ours at SL3.
/// Rule: at most one step and at most 3 NACKs, and the SL3 frame decodes.
#[test]
#[ignore = "release probe, ~minutes: #1460 M3"]
fn m3_a_minute_of_cw_at_sl4() {
    let mut rx = engine();
    rx.start_ota_session(fast_at(SpeedLevel::Sl4));
    let mut budget = NackBudget::default();
    let cw = CwId {
        wpm: 20,
        tone_hz: 1_500.0,
        amplitude: 0.6,
    };
    let mut over = Vec::new();
    while over.len() < 60 * FS {
        over.extend(cw.samples("CQ CQ CQ DE DL1ABC DL1ABC DL1ABC K ", FS as u32));
    }
    over.truncate(60 * FS);
    let frame = our_frame(SpeedLevel::Sl3);
    let frame_at = (10 + 60 + 10) * FS;
    let mut input = idle(80 + frame.len() / FS + 10);
    overlay(&mut input, 10 * FS, &over, 6.0);
    overlay(&mut input, frame_at, &frame, 10.0);
    let counted = run(&mut rx, &mut budget, &input);
    let (cw_part, after): (Vec<Counted>, Vec<Counted>) = counted
        .iter()
        .partition(|c| (c.at_s * FS as f32) as usize <= frame_at);
    let nacks = cw_part
        .iter()
        .filter(|c| c.keyed && c.decoded.is_none())
        .count();
    eprintln!(
        "M3 CW: {} counted failures, {} muted, {nacks} NACKs keyed, {} steps",
        failed(&cw_part),
        muted(&cw_part),
        steps(&cw_part)
    );
    eprintln!(
        "M3 CW, today (every counted failure in the hysteresis): {} steps",
        failed(&cw_part) / 3
    );
    for c in &counted {
        eprintln!("  {c:?}");
    }
    eprintln!("M3 frame: {after:?}");
    assert!(steps(&cw_part) <= 1, "M3: more than one step: {cw_part:?}");
    assert!(nacks <= 3, "M3: {nacks} NACKs keyed at the CW");
    assert!(
        after.iter().any(|c| c.decoded == Some(SpeedLevel::Sl3)),
        "M3: our SL3 frame did not decode after the CW: {after:?}"
    );
}
