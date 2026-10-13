//! #1460 round 3: does the notch bank's tone classifier separate foreign narrowband overs from our
//! own bursts? A measurement, not a gate (`docs/dev/design/failed-burst-ownership.md`, *Round 3*).
//!
//! For every burst `accumulate_capture` gathers over a fixture, with at least the 0.5 s evidence
//! floor of post-lead audio, T is the share of its 4 096-sample blocks (hop 512) that
//! `NotchBank::tone_dominated` calls tone-dominated. Protected band: fc ± baud, as `apply_rx_notch`
//! sets it from `occupied_bandwidth_hz`. Peak span: fc ± (baud + 50 Hz), the AFC range.
//!
//! "Ours" is every gathered burst of our frames at the failing SNRs, decodable or not: a superset of
//! the failing ones, so its largest T can only overstate the overlap.
//!
//! `PROBE_SEEDS` (default 50) seeds per cell; run with `--release --ignored --nocapture`.

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_channel::watterson::WattersonChannel;
use openpulse_channel::{ChannelModel, WattersonConfig};
use openpulse_core::fec::FecMode;
use openpulse_dsp::notch::{NotchBank, NotchParams};
use openpulse_modem::capture_replay::{load_corpus, Capture};
use openpulse_modem::ModemEngine;
use qpsk_plugin::QpskPlugin;

const TICK: usize = 400;
const FS: f32 = 8_000.0;
const FC: f32 = 1_500.0;
const WARM: usize = 3 * TICK * 20;
const FLOOR: usize = 4_000;
const IDLES: [&str; 5] = [
    "ft991a-idle.wav",
    "ic9700-idle-250hz.wav",
    "ic9700-idle-500hz.wav",
    "ic9700-idle-hot.wav",
    "ic9700-idle-wide-500hz-control.wav",
];
/// (rung, mode, baud, floor dB).
const RUNGS: [(&str, &str, f32, f32); 5] = [
    ("SL2", "BPSK31", 31.25, 3.0),
    ("SL3", "BPSK63", 62.5, 4.0),
    ("SL4", "BPSK100", 100.0, 4.5),
    ("SL5", "BPSK250", 250.0, 5.0),
    ("SL6", "QPSK250-D", 250.0, 7.0),
];

fn engine() -> (ModemEngine, LoopbackBackend) {
    let lb = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(lb.clone_shared()));
    e.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    e.register_plugin(Box::new(QpskPlugin::new())).unwrap();
    (e, lb)
}

fn rng(seed: u64) -> impl FnMut() -> f32 {
    let mut st = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1);
    move || {
        st = st
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (st >> 40) as f32 / (1u64 << 24) as f32
    }
}

fn tone(f: f32, n: usize, ph0: f32) -> Vec<f32> {
    (0..n)
        .map(|k| (std::f32::consts::TAU * f * k as f32 / FS + ph0).cos())
        .collect()
}

fn frame(mode: &str, seed: u64, offset_hz: f32) -> Vec<f32> {
    let (mut e, lb) = engine();
    let payload: Vec<u8> = (0..64u32)
        .map(|i| (i * 131 + seed as u32 * 7) as u8)
        .collect();
    e.transmit_with_fec_mode(&payload, mode, FecMode::Rs, None)
        .expect("tx");
    let mut s = lb.drain_samples();
    let mut cfg = WattersonConfig::moderate_f1(Some(seed));
    cfg.snr_db = 60.0;
    s = WattersonChannel::new(cfg).unwrap().apply(&s);
    if offset_hz != 0.0 {
        // Analytic shift: the frame is real, so mix with its Hilbert pair via a quadrature tone.
        s = shift(&s, offset_hz);
    }
    s
}

fn shift(x: &[f32], df: f32) -> Vec<f32> {
    use rustfft::{num_complex::Complex, FftPlanner};
    let n = x.len().next_power_of_two();
    let mut b: Vec<Complex<f32>> = x
        .iter()
        .map(|&v| Complex::new(v, 0.0))
        .chain(std::iter::repeat(Complex::new(0.0, 0.0)))
        .take(n)
        .collect();
    let mut p = FftPlanner::new();
    p.plan_fft_forward(n).process(&mut b);
    for c in b.iter_mut().take(n / 2).skip(1) {
        *c *= 2.0;
    }
    for c in b.iter_mut().skip(n / 2 + 1) {
        *c = Complex::new(0.0, 0.0);
    }
    p.plan_fft_inverse(n).process(&mut b);
    (0..x.len())
        .map(|k| {
            let ph = std::f32::consts::TAU * df * k as f32 / FS;
            let z = b[k] / n as f32;
            z.re * ph.cos() - z.im * ph.sin()
        })
        .collect()
}

/// Morse at `wpm`, random letters, keyed with 5 ms raised-cosine edges.
fn cw(f: f32, wpm: f32, secs: f32, seed: u64) -> Vec<f32> {
    const CODES: [&str; 10] = [
        ".-", "-...", "-.-.", "-..", ".", "..-.", "--.", "....", "..", ".---",
    ];
    let dot = (1.2 / wpm * FS) as usize;
    let mut r = rng(seed);
    let mut key: Vec<f32> = Vec::new();
    let total = (secs * FS) as usize;
    while key.len() < total {
        for _ in 0..5 {
            let c = CODES[(r() * CODES.len() as f32) as usize % CODES.len()];
            for el in c.chars() {
                key.extend(std::iter::repeat_n(
                    1.0,
                    if el == '.' { dot } else { 3 * dot },
                ));
                key.extend(std::iter::repeat_n(0.0, dot));
            }
            key.extend(std::iter::repeat_n(0.0, 2 * dot));
        }
        key.extend(std::iter::repeat_n(0.0, 4 * dot));
    }
    key.truncate(total);
    let edge = (0.005 * FS) as usize;
    let mut smooth = key.clone();
    for i in 0..key.len() {
        let lo = i.saturating_sub(edge);
        smooth[i] = key[lo..=i].iter().sum::<f32>() / (i - lo + 1) as f32;
    }
    tone(f, total, 0.0)
        .into_iter()
        .zip(smooth)
        .map(|(t, k)| t * k)
        .collect()
}

/// RTTY 45.45 Bd, 170 Hz shift, continuous phase, random 5-bit characters with start/stop.
fn rtty(f_center: f32, secs: f32, seed: u64) -> Vec<f32> {
    let sym = (FS / 45.45) as usize;
    let mut r = rng(seed);
    let mut bits: Vec<bool> = Vec::new();
    while bits.len() * sym < (secs * FS) as usize {
        bits.push(false);
        for _ in 0..5 {
            bits.push(r() < 0.5);
        }
        bits.push(true);
        bits.push(true);
    }
    let mut ph = 0.0f32;
    let mut out = Vec::new();
    for b in bits {
        let f = f_center + if b { 85.0 } else { -85.0 };
        for _ in 0..sym {
            ph += std::f32::consts::TAU * f / FS;
            out.push(ph.cos());
        }
    }
    out.truncate((secs * FS) as usize);
    out
}

fn in_band_noise(idle: &[f32], baud: f32) -> f32 {
    let x = &idle[..WARM.min(idle.len())];
    let filtered = band_power(x, FC - baud, FC + baud);
    filtered.max(1e-12)
}

/// Mean power of `x` inside `lo..hi`, from 4096-point periodograms (scaled to sample power).
fn band_power(x: &[f32], lo: f32, hi: f32) -> f32 {
    use rustfft::{num_complex::Complex32, FftPlanner};
    const N: usize = 4096;
    let fft = FftPlanner::<f32>::new().plan_fft_forward(N);
    let (mut band, mut segs) = (0.0f64, 0usize);
    for seg in x.chunks_exact(N) {
        let mut buf: Vec<Complex32> = seg.iter().map(|&v| Complex32::new(v, 0.0)).collect();
        fft.process(&mut buf);
        for (k, c) in buf.iter().enumerate().take(N / 2).skip(1) {
            let f = k as f32 * FS / N as f32;
            if (lo..=hi).contains(&f) {
                band += 2.0 * c.norm_sqr() as f64 / (N as f64 * N as f64);
            }
        }
        segs += 1;
    }
    (band / segs.max(1) as f64) as f32
}

/// Every gathered burst's T for one fixture: `sig` at `in_band_db` over `idle`, plus `extra` added
/// unscaled (already at its own level).
fn bursts(
    idle_name: &str,
    seed: u64,
    mode: &str,
    baud: f32,
    sig: &[f32],
    in_band_db: f32,
    extra: Option<&[f32]>,
) -> Vec<(f32, f32)> {
    let idle: Capture = load_corpus(idle_name).expect("idle");
    let lead = 4_000 + (seed as usize * 9_173) % 20_000;
    let total = WARM + lead + sig.len() + 5 * 8_000;
    let mut buf = idle.cycled(seed as usize * 9_000, total);
    let noise = in_band_noise(&buf, baud);
    let sp = band_power(sig, FC - baud - 60.0, FC + baud + 60.0).max(1e-12);
    let g = (noise * 10f32.powf(in_band_db / 10.0) / sp).sqrt();
    let f0 = WARM + lead;
    for (i, s) in sig.iter().enumerate() {
        buf[f0 + i] += s * g;
    }
    if let Some(x) = extra {
        for (i, s) in x.iter().enumerate() {
            if f0 + i < buf.len() {
                buf[f0 + i] += s * g;
            }
        }
    }
    let (mut e, _lb) = engine();
    let mut out = Vec::new();
    let mut fed = 0usize;
    for chunk in buf.chunks(TICK) {
        fed += chunk.len();
        let Ok(Some(b)) = e.accumulate_capture(Some(mode), chunk.to_vec()) else {
            continue;
        };
        let end = fed - chunk.len();
        let start = end - b.samples.len();
        if start >= f0 + sig.len() || end <= f0 {
            continue;
        }
        let post = &b.samples[e.last_flush_lead().min(b.samples.len())..];
        if post.len() < FLOOR {
            continue;
        }
        out.push(t_of(post, baud));
    }
    out
}

fn t_of(post: &[f32], baud: f32) -> (f32, f32) {
    let mut bank = NotchBank::new(NotchParams::default());
    bank.set_protect_band(FC - baud, FC + baud);
    let (lo, hi) = (FC - baud - 50.0, FC + baud + 50.0);
    let n = 4_096.min(post.len());
    let (mut hit, mut peak, mut blocks) = (0usize, 0usize, 0usize);
    let mut i = 0;
    while i + n <= post.len() {
        blocks += 1;
        hit += bank.tone_dominated(&post[i..i + n], lo, hi) as usize;
        peak += bank.has_prominent_peak(&post[i..i + n], lo, hi) as usize;
        i += 512;
    }
    let b = blocks.max(1) as f32;
    (hit as f32 / b, peak as f32 / b)
}

fn summary(label: &str, both: &mut [(f32, f32)]) {
    let mut t: Vec<f32> = both.iter().map(|x| x.0).collect();
    let mut p: Vec<f32> = both.iter().map(|x| x.1).collect();
    one(&format!("{label} [T]"), &mut t);
    one(&format!("{label} [P]"), &mut p);
}

fn one(label: &str, ts: &mut [f32]) {
    if ts.is_empty() {
        println!("  {label:<44} n=0");
        return;
    }
    ts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let q = |p: f32| ts[((ts.len() - 1) as f32 * p).round() as usize];
    println!(
        "  {label:<44} n={:<4} min {:.3}  p50 {:.3}  p95 {:.3}  max {:.3}",
        ts.len(),
        ts[0],
        q(0.5),
        q(0.95),
        ts[ts.len() - 1]
    );
}

#[test]
#[ignore = "measurement (#1460 round 3): run with --release --ignored --nocapture"]
fn burst_tone_dominance_by_class() {
    let seeds: u64 = std::env::var("PROBE_SEEDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(50);
    let only = std::env::var("PROBE_RUNG").ok();
    for (rung, mode, baud, floor) in RUNGS {
        if only.as_deref().is_some_and(|o| o != rung) {
            continue;
        }
        println!("{rung} {mode}");
        let preamble = {
            use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
            let cfg = ModulationConfig {
                mode: mode.into(),
                ..ModulationConfig::default()
            };
            if mode.starts_with("QPSK") {
                QpskPlugin::new().frame_geometry(&cfg)
            } else {
                BpskPlugin::new().frame_geometry(&cfg)
            }
            .expect("geometry")
            .preamble_samples
        };
        let idle = |s: u64| IDLES[s as usize % IDLES.len()];
        let mut ours = Vec::new();
        let mut cells: Vec<(String, Vec<(f32, f32)>)> = Vec::new();
        for (name, db, off) in [
            ("ours floor-2 0Hz", floor - 2.0, 0.0),
            ("ours floor-6 0Hz", floor - 6.0, 0.0),
            ("ours floor-2 +50Hz", floor - 2.0, 50.0),
            ("ours floor-6 +50Hz", floor - 6.0, 50.0),
        ] {
            let mut ts = Vec::new();
            for s in 0..seeds {
                let f = frame(mode, s, off);
                ts.extend(bursts(idle(s), s, mode, baud, &f, db, None));
            }
            ours.extend(ts.iter().copied());
            cells.push((name.into(), ts));
        }
        let (mut heads, mut tails) = (Vec::new(), Vec::new());
        for s in 0..seeds {
            let f = frame(mode, s, 0.0);
            let head: Vec<f32> = f[..preamble.max(FLOOR + 512).min(f.len())].to_vec();
            heads.extend(bursts(idle(s), s, mode, baud, &head, floor, None));
            let tail = f[f.len().saturating_sub(4_000 + 512)..].to_vec();
            tails.extend(bursts(idle(s), s, mode, baud, &tail, floor, None));
        }
        ours.extend(heads.iter().copied());
        ours.extend(tails.iter().copied());
        cells.push(("ours head piece".into(), heads));
        cells.push(("ours tail piece".into(), tails));
        summary("OURS (all above)", &mut ours.clone());
        for (n, mut t) in cells {
            summary(&n, &mut t);
        }
        for rel in [-6.0f32, 0.0, 6.0] {
            let mut ts = Vec::new();
            for s in 0..seeds {
                let f = frame(mode, s, 0.0);
                let mut r = rng(s + 99);
                let df = (r() * 2.0 - 1.0) * baud;
                let fp = band_power(&f, FC - baud - 60.0, FC + baud + 60.0).sqrt();
                let a = fp * 10f32.powf(rel / 20.0) * std::f32::consts::SQRT_2;
                let c: Vec<f32> = tone(FC + df, f.len(), 0.3).iter().map(|v| v * a).collect();
                ts.extend(bursts(idle(s), s, mode, baud, &f, floor, Some(&c)));
            }
            summary(&format!("ours + carrier {rel:+} dB"), &mut ts);
        }
        for db in [6.0f32, 12.0] {
            let secs = 8.0;
            let n = (secs * FS) as usize;
            let mut sweep = Vec::new();
            let mut cw20 = Vec::new();
            let mut cw30 = Vec::new();
            let mut rt = Vec::new();
            let mut pair = Vec::new();
            for s in 0..seeds {
                let span = baud + 50.0;
                let f = FC - span + 2.0 * span * (s as f32 + 0.5) / seeds as f32;
                sweep.extend(bursts(idle(s), s, mode, baud, &tone(f, n, 0.0), db, None));
                cw20.extend(bursts(
                    idle(s),
                    s,
                    mode,
                    baud,
                    &cw(f, 20.0, secs, s),
                    db,
                    None,
                ));
                cw30.extend(bursts(
                    idle(s),
                    s,
                    mode,
                    baud,
                    &cw(f, 30.0, secs, s),
                    db,
                    None,
                ));
                rt.extend(bursts(idle(s), s, mode, baud, &rtty(f, secs, s), db, None));
                let p: Vec<f32> = tone(FC - baud / 2.0, n, 0.0)
                    .iter()
                    .zip(tone(FC + baud / 2.0, n, 1.0))
                    .map(|(a, b)| a + b)
                    .collect();
                pair.extend(bursts(idle(s), s, mode, baud, &p, db, None));
            }
            summary(&format!("carrier sweep +{db} dB"), &mut sweep);
            summary(&format!("CW 20 wpm +{db} dB"), &mut cw20);
            summary(&format!("CW 30 wpm +{db} dB"), &mut cw30);
            summary(&format!("RTTY 170 Hz +{db} dB"), &mut rt);
            summary(&format!("carrier pair fc±baud/2 +{db} dB"), &mut pair);
        }
    }
}
