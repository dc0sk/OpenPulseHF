//! A wideband frame does not close its own burst (#1304, REQ-DCD-01).
//!
//! #1304: OFDM52 occupies ~2031 Hz, ~85 % of the 300–2700 Hz band. The pre-#1452 floor was a low
//! percentile ACROSS bins, so during such a frame it read signal bins, climbed, and — by the issue's
//! arithmetic — passed the frame's own level after ~1.3 s, flushing the burst mid-frame. #1452 made
//! the floor per-bin over time and holds it while a burst is gathered; this pins that on the case the
//! issue names, which `dcd_floor_follows_the_filter.rs` covers only for BPSK250 behind 500 Hz.
//!
//! Driven through the production entry (`accumulate_capture`) at the daemon's 100 ms read, on the
//! two recorded wide-filter idles. Levels are set against the noise inside OFDM52's occupied band.

use ofdm_plugin::OfdmPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::fec::FecMode;
use openpulse_modem::capture_replay::{load_corpus, Capture};
use openpulse_modem::ModemEngine;
use rustfft::num_complex::Complex32;
use rustfft::FftPlanner;

const TICK: usize = 800;
const RATE: f32 = 8000.0;
/// Half of OFDM52's occupied bandwidth (2031.25 Hz), centred at 1500 Hz.
const HALF_BAND: f32 = 1015.625;
/// Longest frame a single `transmit` carries.
const MAX_FRAME: usize = 255;

fn engine() -> (ModemEngine, LoopbackBackend) {
    let lb = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(lb.clone_shared()));
    e.register_plugin(Box::new(OfdmPlugin::new()))
        .expect("register");
    (e, lb)
}

fn corpus(name: &str) -> Capture {
    load_corpus(name).unwrap_or_else(|e| panic!("corpus file {name} must load: {e}"))
}

fn payload(n: usize, seed: u32) -> Vec<u8> {
    (0..n as u32)
        .map(|i| (i.wrapping_add(seed).wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect()
}

fn frame(p: &[u8], mode: &str, fec: FecMode) -> Vec<f32> {
    let (mut e, lb) = engine();
    e.transmit_with_fec_mode(p, mode, fec, None).expect("tx");
    lb.drain_samples()
}

/// Mean power of `x` inside `lo..=hi` Hz (DC removed), from averaged 4096-point periodograms.
fn in_band_power(x: &[f32], lo: f32, hi: f32) -> f32 {
    const N: usize = 4096;
    let fft = FftPlanner::<f32>::new().plan_fft_forward(N);
    let (mut band, mut segs) = (0.0f64, 0usize);
    for seg in x.as_chunks::<N>().0 {
        let m = seg.iter().sum::<f32>() / N as f32;
        let mut buf: Vec<Complex32> = seg.iter().map(|&v| Complex32::new(v - m, 0.0)).collect();
        fft.process(&mut buf);
        for (k, c) in buf.iter().enumerate().take(N / 2).skip(1) {
            if (lo..=hi).contains(&(k as f32 * RATE / N as f32)) {
                // One-sided: each bin carries its mirror's power too.
                band += 2.0 * c.norm_sqr() as f64;
            }
        }
        segs += 1;
    }
    (band / (segs as f64 * (N * N) as f64)) as f32
}

struct Outcome {
    /// Bursts flushed that overlap the transmission.
    over: usize,
    /// The one overlapping burst spans the whole transmission.
    whole: bool,
    /// Frames of the transmission decoded from the bursts.
    decoded: usize,
}

/// `frames` back-to-back frames of `mode` on `idle` at `in_band_db`, fed in `TICK` reads.
fn run(
    idle: &Capture,
    from: usize,
    mode: &str,
    fec: FecMode,
    frames: &[Vec<u8>],
    in_band_db: f32,
) -> Outcome {
    let tx: Vec<f32> = frames.iter().flat_map(|p| frame(p, mode, fec)).collect();
    let warm = 3 * 8000;
    let total = warm + tx.len() + 5 * 8000;
    let mut buf = idle.cycled(from, total);
    let noise = in_band_power(&buf[..warm], 1500.0 - HALF_BAND, 1500.0 + HALF_BAND);
    let fms = tx.iter().map(|v| v * v).sum::<f32>() / tx.len() as f32;
    let g = (noise * 10f32.powf(in_band_db / 10.0) / fms).sqrt();
    for (i, s) in tx.iter().enumerate() {
        buf[warm + i] += s * g;
    }
    let (f0, f1) = (warm, warm + tx.len());
    let (mut e, _lb) = engine();
    let (mut fed, mut over, mut whole, mut decoded) = (0usize, 0usize, false, 0usize);
    for chunk in buf.chunks(TICK) {
        let before = fed;
        fed += chunk.len();
        if let Ok(Some(b)) = e.accumulate_capture(Some(mode), chunk.to_vec()) {
            let start = before.saturating_sub(b.samples.len());
            if start < f1 && before > f0 {
                over += 1;
                whole = start <= f0 && before >= f1;
                if frames.len() == 1 {
                    decoded += matches!(
                        e.decode_burst_with_fec(mode, fec, &b),
                        Ok(p) if p == frames[0]
                    ) as usize;
                }
            }
        }
    }
    Outcome {
        over,
        whole,
        decoded,
    }
}

fn sweep(mode: &str, fec: FecMode, n_frames: usize, in_band_db: f32) -> (usize, usize, usize) {
    let (mut split, mut whole, mut decoded) = (0, 0, 0);
    let mut cells = 0;
    for idle in ["ic9700-idle-hot.wav", "ft991a-idle.wav"] {
        let cap = corpus(idle);
        for t in 0..4usize {
            let frames: Vec<Vec<u8>> = (0..n_frames)
                .map(|i| payload(MAX_FRAME, (t * 31 + i) as u32))
                .collect();
            let o = run(&cap, t * 11_000, mode, fec, &frames, in_band_db);
            println!(
                "{idle} t{t} {mode} x{n_frames}: bursts {} whole {} decoded {}",
                o.over, o.whole, o.decoded
            );
            cells += 1;
            split += (o.over != 1) as usize;
            whole += o.whole as usize;
            decoded += o.decoded;
        }
    }
    assert_eq!(cells, 8);
    (split, whole, decoded)
}

/// One maximum-size SL7 frame (~1.6 s, past the ~1.3 s #1304 predicted) is one burst and decodes.
///
/// A production-path decode check, NOT the #1304 discriminator: it also passes with the hold
/// disabled (sabotage S1 below), because one frame is too short to walk an unheld floor past it.
#[test]
fn a_full_ofdm52_frame_is_one_burst_and_decodes() {
    let (split, whole, decoded) = sweep("OFDM52", FecMode::SoftConcatenated, 1, 15.0);
    assert_eq!(split, 0, "{split}/8 OFDM52 frames were not one burst");
    assert_eq!(whole, 8, "{whole}/8 bursts spanned the whole frame");
    assert!(decoded >= 7, "{decoded}/8 OFDM52 frames decoded");
}

/// Eight back-to-back SL7 frames (~13 s of continuous wideband signal) are gathered as one burst —
/// ten times the length at which #1304's arithmetic had the floor pass the signal.
///
/// Sabotage S1 (`NoiseFloorTracker::hold` made a no-op, so the floor learns from the frame): 8/8
/// cells fail — six split into 2–4 bursts, two closed before the transmission ended. That is
/// #1304's mechanism, reproduced; the hold is what this test proves.
#[test]
fn a_long_ofdm52_transmission_is_one_burst() {
    let (split, whole, _) = sweep("OFDM52", FecMode::SoftConcatenated, 8, 15.0);
    assert_eq!(
        split, 0,
        "{split}/8 long OFDM52 transmissions were not one burst"
    );
    assert_eq!(whole, 8, "{whole}/8 bursts spanned the whole transmission");
}

/// The unit-level control: the probe's level is what it claims, so the frame is not trivially loud.
#[test]
fn the_probe_level_is_set_against_in_band_noise() {
    let idle = corpus("ic9700-idle-hot.wav").cycled(0, 3 * 8000);
    let n = in_band_power(&idle, 1500.0 - HALF_BAND, 1500.0 + HALF_BAND);
    let m = idle.iter().sum::<f32>() / idle.len() as f32;
    let total = idle.iter().map(|v| (v - m) * (v - m)).sum::<f32>() / idle.len() as f32;
    assert!(
        n > 0.0 && n < total,
        "in-band noise {n} is not a fraction of total {total}"
    );
}
