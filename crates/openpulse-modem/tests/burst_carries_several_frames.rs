//! A burst that carries several back-to-back frames yields all of them, in order (#1461).
//!
//! A sender keys once and sends its fragments back to back, so `accumulate_capture` hands them over
//! as one burst; `decode_burst_with_fec` returns only the first. Driven through the production entry
//! over a recorded IC-9700 idle, so the spectral detect's hold leaves the real tail behind the last
//! frame — the tail the continuation must stop on without decoding garbage or a duplicate.

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::fec::FecMode;
use openpulse_modem::capture_replay::load_corpus;
use openpulse_modem::pipeline::AudioSamples;
use openpulse_modem::ModemEngine;
use std::time::Instant;

const MODE: &str = "BPSK250";
const TICK: usize = 400;
const WARM: usize = 24_000;
const IDLE: &str = "ic9700-idle-wide-500hz-control.wav";

fn engine() -> (ModemEngine, LoopbackBackend) {
    let lb = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(lb.clone_shared()));
    e.register_plugin(Box::new(BpskPlugin::new()))
        .expect("register");
    (e, lb)
}

fn payload(tag: u8) -> Vec<u8> {
    (0..200u16)
        .map(|i| (i as u8).wrapping_mul(7) ^ tag)
        .collect()
}

/// `payloads` sent back to back in one keying, laid over the idle at +20 dB, gathered by
/// `accumulate_capture` in daemon-sized reads. Returns the engine and the first flushed burst.
fn gathered(payloads: &[Vec<u8>]) -> (ModemEngine, AudioSamples) {
    let (mut tx, lb) = engine();
    for p in payloads {
        tx.transmit(p, MODE, None).expect("tx");
    }
    let keying = lb.drain_samples();
    let idle = load_corpus(IDLE).expect("idle corpus");
    let mut buf = idle.cycled(0, WARM + keying.len() + 2 * 8000);
    let ms = |x: &[f32]| x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32;
    let g = (ms(&buf[..WARM]) * 100.0 / ms(&keying)).sqrt();
    for (i, s) in keying.iter().enumerate() {
        buf[WARM + i] += s * g;
    }
    let (mut rx, _lb) = engine();
    for chunk in buf.chunks(TICK) {
        if let Ok(Some(b)) = rx.accumulate_capture(Some(MODE), chunk.to_vec()) {
            if b.samples.len() > keying.len() / 2 {
                return (rx, b);
            }
        }
    }
    panic!("the keying never flushed as one burst");
}

#[test]
fn every_frame_of_a_multi_frame_burst_is_returned_in_order() {
    let sent: Vec<Vec<u8>> = (1..=3).map(payload).collect();
    let (mut rx, burst) = gathered(&sent);
    let got = rx
        .decode_burst_frames(MODE, FecMode::None, &burst)
        .expect("decode");
    assert_eq!(got, sent, "all three frames, in order, no duplicate");
}

/// The tail after a lone frame is the spectral hold's; it must yield nothing extra. Also prints what
/// the continuation costs there, against the first-frame-only entry on the same burst.
#[test]
fn a_single_frame_burst_yields_exactly_one_frame() {
    let sent = vec![payload(9)];
    let (mut rx, burst) = gathered(&sent);
    let (mut first_only, _) = gathered(&sent);
    let t = Instant::now();
    let got = rx
        .decode_burst_frames(MODE, FecMode::None, &burst)
        .expect("decode");
    let all = t.elapsed();
    let t = Instant::now();
    let one = first_only
        .decode_burst_with_fec(MODE, FecMode::None, &burst)
        .expect("decode");
    let first = t.elapsed();
    println!(
        "single-frame burst of {} samples: decode_burst_frames {all:?}, decode_burst_with_fec {first:?}",
        burst.samples.len()
    );
    assert_eq!(got, sent);
    assert_eq!(one, sent[0]);
}
