//! A burst that total power opens keeps the start of its frame (#1443), and the onset scan reaches
//! every onset a flushed burst can hold (#1443, fixing a reach #1454 shipped).
//!
//! **The lead.** A frame that begins in the last part of a read too quiet to trip the squelch used to
//! lose that part: the burst started at the next read. BPSK250 and QPSK500 fail with 19 samples lost.
//! And total power has no hold, so near the squelch it flickers over a frame's head — trip, drop,
//! re-trip — until the spectral test arms: the surviving burst began up to ~2 100 samples in. A burst
//! total power opens now carries the previous read plus the spectral test's arming latency as a lead. For BPSK the lead is mainly ROOM
//! — its decode reads a fixed preamble from the onset it is handed, and a burst starting inside the
//! frame offers no onset at or before its first sample (the same length of idle from elsewhere rescues
//! it too); QPSK500 needs the real head (its timing search has no negative reach).
//!
//! **The reach.** A frame that opens a burst can start anywhere in its trigger read, so the scan must
//! reach lead + trigger read + acquisition window; `lead + acq` missed every late onset in a long read.
//!
//! Every fixture here drives `accumulate_capture` over the recorded IC-9700 idles. The decode cells are
//! held out for runtime (`scripts/slow-tests.sh spectral`).

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::fec::FecMode;
use openpulse_modem::capture_replay::{load_corpus, Capture};
use openpulse_modem::pipeline::AudioSamples;
use openpulse_modem::ModemEngine;
use rustfft::num_complex::Complex32;
use rustfft::FftPlanner;

const TICK: usize = 400;
const RATE: f32 = 8000.0;
const WARM: usize = 3 * TICK * 20;
const PLACEMENTS: usize = 16;
const WIDE: &str = "ic9700-idle-wide-500hz-control.wav";
const NARROW_500: &str = "ic9700-idle-500hz.wav";

fn engine() -> (ModemEngine, LoopbackBackend) {
    let lb = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(lb.clone_shared()));
    e.register_plugin(Box::new(BpskPlugin::new()))
        .expect("register");
    e.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))
        .expect("register");
    (e, lb)
}

fn corpus(name: &str) -> Capture {
    load_corpus(name).unwrap_or_else(|e| panic!("corpus file {name} must load: {e}"))
}

fn rms(x: &[f32]) -> f32 {
    let m = x.iter().sum::<f32>() / x.len() as f32;
    (x.iter().map(|v| (v - m) * (v - m)).sum::<f32>() / x.len() as f32).sqrt()
}

/// Share of `x`'s power inside `lo..hi` Hz, from averaged 4096-point periodograms.
fn in_band_share(x: &[f32], lo: f32, hi: f32) -> f32 {
    const N: usize = 4096;
    let fft = FftPlanner::<f32>::new().plan_fft_forward(N);
    let (mut band, mut total) = (0.0f64, 0.0f64);
    for seg in x.as_chunks::<N>().0 {
        let m = seg.iter().sum::<f32>() / N as f32;
        let mut buf: Vec<Complex32> = seg.iter().map(|&v| Complex32::new(v - m, 0.0)).collect();
        fft.process(&mut buf);
        for (k, c) in buf.iter().enumerate().take(N / 2).skip(1) {
            let f = k as f32 * RATE / N as f32;
            let p = c.norm_sqr() as f64;
            total += p;
            if (lo..=hi).contains(&f) {
                band += p;
            }
        }
    }
    (band / total) as f32
}

fn frame(payload: &[u8], mode: &str) -> Vec<f32> {
    let (mut e, lb) = engine();
    e.transmit_with_fec_mode(payload, mode, FecMode::Rs, None)
        .expect("tx");
    lb.drain_samples()
}

/// Half the occupied band of each BPSK rung used here (its baud).
fn half_band(mode: &str) -> f32 {
    match mode {
        "BPSK250" => 250.0,
        "BPSK63" => 63.0,
        "BPSK31" => 31.25,
        "QPSK500" => 250.0,
        _ => panic!("no band for {mode}"),
    }
}

fn geometry(mode: &str) -> openpulse_core::plugin::FrameGeometry {
    use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
    let cfg = ModulationConfig {
        mode: mode.into(),
        ..ModulationConfig::default()
    };
    if mode.starts_with("QPSK") {
        qpsk_plugin::QpskPlugin::new().frame_geometry(&cfg)
    } else {
        BpskPlugin::new().frame_geometry(&cfg)
    }
    .expect("the mode publishes its frame geometry")
}

fn payload(n: usize) -> Vec<u8> {
    (0..n as u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect()
}

/// Trigger offset of one placement: `(−f0) mod read` measured against the read grid, i.e. how far into
/// the frame the read after the frame's first sample begins. `place` spreads these evenly.
fn place(t: usize) -> usize {
    4000 + t * 17_137
}

struct Burst {
    start: usize,
    lead: usize,
    samples: AudioSamples,
}

/// Feed `tx` at `in_band_db` into `idle` at placement `t` in `read`-sample reads; return every flushed
/// burst overlapping the frame, the frame start, and the engine (so a burst can be decoded with the
/// flush flags it carries).
#[allow(clippy::too_many_arguments)] // a fixture's knobs, each varied by some gate below
fn run(
    idle: &Capture,
    t: usize,
    mode: &str,
    tx: &[f32],
    in_band_db: f32,
    read: usize,
    frame_at: Option<usize>,
    decode: Option<&[u8]>,
) -> (usize, Vec<Burst>, bool) {
    let lead = frame_at.unwrap_or_else(|| place(t));
    let total = WARM + lead + tx.len() + 5 * 8000;
    let mut buf = idle.cycled(t * 9000, total);
    let noise = rms(&buf[..WARM]).powi(2)
        * in_band_share(
            &buf[..WARM],
            1500.0 - half_band(mode),
            1500.0 + half_band(mode),
        );
    let fms = tx.iter().map(|v| v * v).sum::<f32>() / tx.len() as f32;
    let g = (noise * 10f32.powf(in_band_db / 10.0) / fms).sqrt();
    let f0 = WARM + lead;
    for (i, s) in tx.iter().enumerate() {
        buf[f0 + i] += s * g;
    }
    let shortest = geometry(mode).preamble_samples;
    let (mut e, _lb) = engine();
    let (mut fed, mut out, mut decoded) = (0usize, Vec::new(), false);
    for chunk in buf.chunks(read) {
        fed += chunk.len();
        let Ok(Some(b)) = e.accumulate_capture(Some(mode), chunk.to_vec()) else {
            continue;
        };
        let end = fed - chunk.len();
        let start = end - b.samples.len();
        let lead = e.last_flush_lead();
        if start >= f0 + tx.len() || end <= f0 || b.samples.len() - lead < shortest {
            continue;
        }
        if let Some(p) = decode {
            decoded |= matches!(e.decode_burst_with_fec(mode, FecMode::Rs, &b), Ok(q) if q == p);
        }
        out.push(Burst {
            start,
            lead,
            samples: b,
        });
    }
    (f0, out, decoded)
}

const CELLS: [(&str, &str, f32); 5] = [
    (WIDE, "BPSK250", 8.0),
    (WIDE, "BPSK63", 12.0),
    (NARROW_500, "BPSK63", 8.0),
    (NARROW_500, "BPSK250", 8.0),
    (WIDE, "QPSK500", 12.0),
];

/// Flicker-chain cells: total power trips, drops and re-trips over the frame's head before the
/// spectral test holds. BPSK63's 4 096-sample preamble keeps the ring through the chain's fragments;
/// BPSK250 and QPSK500 at small reads are left out — a fragment as long as their preamble clears the
/// ring (#1454's retention rule), a known loss a lead cannot reach.
const CHAIN_CELLS: [(&str, &str, f32, usize); 2] = [
    (WIDE, "BPSK63", 8.0, TICK),
    (NARROW_500, "BPSK63", 8.0, 171),
];

/// The lead covers the frame's start on every burst total power opened after it (#1443): the first
/// burst over the frame starts at or before the frame's first sample once its lead is counted — on
/// the block-boundary cells and the flicker-chain cells. Two positive controls: the same bursts, lead
/// NOT counted, start inside the frame in at least three placements (the trigger really was late), and
/// more than one read inside it in at least three (the chain class is exercised — a block-boundary
/// offset satisfies the first control and not this one).
#[test]
fn a_total_power_burst_s_lead_covers_the_frame_start() {
    let p = payload(64);
    let (mut late_triggers, mut chain_triggers) = (0usize, 0usize);
    let cells = CELLS
        .iter()
        .map(|&(c, m, d)| (c, m, d, TICK))
        .chain(CHAIN_CELLS);
    for (cap, mode, db, read) in cells {
        let idle = corpus(cap);
        let tx = frame(&p, mode);
        for t in 0..PLACEMENTS {
            let (f0, bursts, _) = run(&idle, t, mode, &tx, db, read, None, None);
            let Some(b) = bursts.first() else {
                panic!("{cap} {mode} +{db} t{t}: no burst over the frame");
            };
            assert!(
                b.start <= f0,
                "{cap} {mode} +{db} t{t}: the burst starts {} samples into the frame (lead {})",
                b.start - f0,
                b.lead
            );
            late_triggers += (b.start + b.lead > f0) as usize;
            chain_triggers += (b.start + b.lead > f0 + read) as usize;
            let _ = &b.samples;
        }
    }
    eprintln!(
        "triggers inside the frame: {late_triggers}; more than one read inside: {chain_triggers}"
    );
    assert!(
        late_triggers >= 3,
        "only {late_triggers} triggers fell inside a frame: the fixture no longer exercises the lead"
    );
    assert!(
        chain_triggers >= 3,
        "only {chain_triggers} triggers fell more than one read inside a frame: the flicker-chain \
         class is no longer exercised"
    );
}

/// The onset scan reaches every onset a flushed burst can hold (#1443). At long reads — the daemon reads
/// whatever buffered since its last tick, thousands of samples after any slow decode; this fixture uses
/// 4 096 — every BPSK250 +8 dB burst is opened by the spectral test with
/// the full 8 192-sample ring, and its frame can start anywhere in the trigger read. Reaching only
/// `lead + acq` decoded 9 of these 16 on `main`, the seven misses exactly the onsets past it.
#[test]
fn the_onset_scan_reaches_the_whole_trigger_read() {
    let p = payload(64);
    let tx = frame(&p, "BPSK250");
    let idle = corpus(WIDE);
    let decoded = (0..PLACEMENTS)
        .filter(|&t| run(&idle, t, "BPSK250", &tx, 8.0, 4096, None, Some(&p)).2)
        .count();
    assert!(
        decoded >= 15,
        "BPSK250 +8 dB at 4 096-sample reads decoded {decoded}/16"
    );
}

/// A QPSK500 frame whose first symbol is the LAST symbol of its trigger read decodes, at 400- and
/// 4 096-sample reads — the tightest acquisition window among the modes this fixture registers
/// (QPSK500, 256; the 1 000/2 000-baud dense modes are tighter still and not measured here).
/// Onsets are kept at a multiple of the symbol period so the frame's carrier phase is the suite's
/// usual 0° (#1463 is a different defect).
#[test]
fn a_frame_starting_on_the_last_symbol_of_its_trigger_read_decodes() {
    let p = payload(64);
    let tx = frame(&p, "QPSK500");
    let sym = geometry("QPSK500").symbol_period_samples;
    let idle = corpus(WIDE);
    for read in [TICK, 4096] {
        // Frame start = the last symbol of a read, on the read grid of the ABSOLUTE stream.
        let f0 = (WARM / read + 21) * read - sym;
        let at = f0 - WARM;
        assert_eq!(
            f0 % read,
            read - sym,
            "the onset must be the trigger read's last symbol"
        );
        assert_eq!(
            f0 % sym,
            0,
            "the fixture must keep the onset on the symbol grid"
        );
        let decoded = (0..4)
            .filter(|&t| {
                run(
                    &idle,
                    t,
                    "QPSK500",
                    &tx,
                    20.0,
                    read,
                    Some(at + 4 * read * t),
                    Some(&p),
                )
                .2
            })
            .count();
        assert_eq!(
            decoded, 4,
            "QPSK500 at {read}-sample reads, onset on the trigger read's last symbol: {decoded}/4"
        );
    }
}

/// Decode counts — the held-out half (minutes unoptimised): the flicker-chain cells at 400-sample
/// reads, BPSK63 at 64-sample reads (the longest chains the rule covers), and the block-boundary cells
/// at 400, 171 and 4 096 (at 4 096 every burst is spectral-opened, so those rows test the ring path and
/// the reach). QPSK500's bar is below 16 because of #1463 (its onset-phase window moves with the lead
/// whenever the lead is not a multiple of the symbol period) and phase 2's span-dependent correction.
#[test]
#[ignore = "held out for runtime: run by scripts/slow-tests.sh spectral"]
fn total_power_bursts_decode_through_the_daemon_path() {
    let p = payload(64);
    let mut failures = Vec::new();
    let late: [(&str, &str, f32); 2] = [(WIDE, "BPSK63", 8.0), (WIDE, "BPSK31", 12.0)];
    for (cap, mode, db) in late {
        let idle = corpus(cap);
        let tx = frame(&p, mode);
        let decoded = (0..PLACEMENTS)
            .filter(|&t| run(&idle, t, mode, &tx, db, TICK, None, Some(&p)).2)
            .count();
        eprintln!(
            "400-sample reads, flicker-chain {cap} {mode} +{db} dB: decoded {decoded}/16 (bar 15)"
        );
        if decoded < 15 {
            failures.push(format!("400 late {cap} {mode} +{db}: {decoded}/16 < 15"));
        }
    }
    for (cap, mode, db) in CELLS.iter().copied().filter(|c| c.1 == "BPSK63") {
        let idle = corpus(cap);
        let tx = frame(&p, mode);
        let decoded = (0..PLACEMENTS)
            .filter(|&t| run(&idle, t, mode, &tx, db, 64, None, Some(&p)).2)
            .count();
        eprintln!("64-sample reads, {cap} {mode} +{db} dB: decoded {decoded}/16 (bar 15)");
        if decoded < 15 {
            failures.push(format!("64 {cap} {mode} +{db}: {decoded}/16 < 15"));
        }
    }
    for read in [TICK, 171, 4096] {
        for (cap, mode, db) in CELLS {
            let idle = corpus(cap);
            let tx = frame(&p, mode);
            let decoded = (0..PLACEMENTS)
                .filter(|&t| run(&idle, t, mode, &tx, db, read, None, Some(&p)).2)
                .count();
            // Two cells sit on their bar, each with a named mechanism: QPSK500 (13/16 at 400 and 4 096
            // reads) loses frames to #1463's onset-phase window and phase 2's span-dependent
            // correction; wide BPSK250 +8 dB at 171 reads (15/16) loses its t0 because a 1 026-sample
            // flicker fragment is longer than BPSK250's 1 024-sample preamble and clears the ring
            // (#1454's retention rule, keyed on the preamble) — a loss no lead can reach.
            let bar = if mode == "QPSK500" { 13 } else { 15 };
            eprintln!(
                "{read}-sample reads, {cap} {mode} +{db} dB: decoded {decoded}/16 (bar {bar})"
            );
            if decoded < bar {
                failures.push(format!("{read} {cap} {mode} +{db}: {decoded}/16 < {bar}"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
