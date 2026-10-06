//! F1 of `docs/dev/design/pn-preamble.md`: BPSK250 with the PN-63 candidate preamble against the
//! shipped `--++` preamble, through the daemon's production receive entry.
//!
//! Paired trials: for each seed both arms get the same payload, the same Watterson fade, the same
//! carrier offset and the same noise realisation, at the same lead into the capture. Each arm's
//! receiver is an OTA engine on a one-rung ladder (SL5 = its mode, `Rs`), fed in 4 096-sample reads
//! through `accumulate_capture`, every flushed burst decoded by `ota_decode_burst` as the daemon
//! does. A trial is a success when the payload comes back.
//!
//! Pass rule (pre-registered in the design): non-inferiority per column, the lower bound of the
//! paired 95 % CI of (PN − shipped) ≥ −0.03, n = 600.
//!
//! `F1_TRIALS` overrides n; `F1_PILOT=1` runs an AWGN SNR sweep (`F1_PILOT_DB`) to locate the cliff,
//! which is then fixed in the design before the main run. `F1_RUNG` and `F1_PAYLOAD_MAX` select the
//! slow rungs for F5. Release build:
//! `cargo test --release -p openpulse-modem --no-default-features --test pn_preamble_parity -- --ignored --nocapture`.

use openpulse_audio::LoopbackBackend;
use openpulse_channel::cfo::{CfoChannel, CfoConfig};
use openpulse_channel::watterson::WattersonChannel;
use openpulse_channel::{ChannelModel, WattersonConfig};
use openpulse_core::fec::FecMode;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::ModemEngine;

/// One rung under test: its shipped and candidate modes, its ladder level and its SNR floor.
#[derive(Clone, Copy)]
struct Rung {
    shipped: &'static str,
    candidate: &'static str,
    level: SpeedLevel,
    floor_db: f32,
}

const RUNGS: [Rung; 4] = [
    Rung {
        shipped: "BPSK31",
        candidate: "BPSK31-PN",
        level: SpeedLevel::Sl2,
        floor_db: 3.0,
    },
    Rung {
        shipped: "BPSK63",
        candidate: "BPSK63-PN",
        level: SpeedLevel::Sl3,
        floor_db: 4.0,
    },
    Rung {
        shipped: "BPSK100",
        candidate: "BPSK100-PN",
        level: SpeedLevel::Sl4,
        floor_db: 4.5,
    },
    Rung {
        shipped: "BPSK250",
        candidate: "BPSK250-PN",
        level: SpeedLevel::Sl5,
        floor_db: 5.0,
    },
];

/// `F1_RUNG` (31, 63, 100 or 250; default 250) picks the rung; F5 runs the slow ones.
fn rung() -> Rung {
    let baud = std::env::var("F1_RUNG").unwrap_or_else(|_| "250".into());
    *RUNGS
        .iter()
        .find(|r| r.shipped == format!("BPSK{baud}"))
        .expect("F1_RUNG is 31, 63, 100 or 250")
}

/// `F1_PAYLOAD_MAX` caps the random payload (default 200 B, F1's range 16..=200).
fn payload_max() -> usize {
    std::env::var("F1_PAYLOAD_MAX")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200)
}
const READ: usize = 4_096;
const DELTA: f64 = 0.03;

/// One measurement column: the channel every paired trial in it sees.
#[derive(Clone, Copy)]
struct Column {
    name: &'static str,
    fading: bool,
    snr_db: f32,
    offset_hz: f32,
}

fn rx_engine(mode: &'static str, level: SpeedLevel) -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(LoopbackBackend::new()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::pn_candidate()))
        .unwrap();
    e.register_plugin(Box::new(fsk4_plugin::Fsk4Plugin::new()))
        .unwrap();
    let profile =
        SessionProfile::from_rungs(&[(level, mode, FecMode::Rs, Some(5.0), None)], level, 3);
    e.start_ota_session(profile);
    e.ota_lock_level(level);
    e
}

fn tx_frame(mode: &str, payload: &[u8]) -> Vec<f32> {
    let bk = LoopbackBackend::new();
    let mut tx = ModemEngine::new(Box::new(bk.clone_shared()));
    tx.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .unwrap();
    tx.register_plugin(Box::new(bpsk_plugin::BpskPlugin::pn_candidate()))
        .unwrap();
    tx.transmit_with_fec_mode(payload, mode, FecMode::Rs, None)
        .expect("transmit");
    bk.drain_samples()
}

/// Deterministic xorshift, so every trial is reproducible from its seed.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn gauss(&mut self) -> f32 {
        let (u1, u2) = (self.unit().max(1e-12), self.unit());
        ((-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()) as f32
    }
}

/// The capture one arm of one trial sees, and the frame's length in it.
fn capture(mode: &str, col: Column, seed: u64, payload: &[u8], lead: usize) -> (Vec<f32>, usize) {
    let mut signal = tx_frame(mode, payload);
    if col.fading {
        let mut cfg = WattersonConfig::moderate_f1(Some(seed));
        cfg.snr_db = 60.0;
        signal = WattersonChannel::new(cfg).unwrap().apply(&signal);
    }
    if col.offset_hz != 0.0 {
        signal = CfoChannel::new(CfoConfig::new(col.offset_hz, 8_000.0))
            .unwrap()
            .apply(&signal);
    }
    let rms = (signal.iter().map(|s| s * s).sum::<f32>() / signal.len() as f32).sqrt();
    let sigma = rms / 10f32.powf(col.snr_db / 20.0);
    // The same noise sequence for both arms: seeded identically, read in order.
    let mut noise = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let total = lead + signal.len() + 8 * READ;
    let mut audio: Vec<f32> = (0..total).map(|_| sigma * noise.gauss()).collect();
    for (a, &s) in audio[lead..].iter_mut().zip(&signal) {
        *a += s;
    }
    (audio, signal.len())
}

/// One arm of one trial: does `mode` deliver `payload` through the production receive path?
fn trial(
    mode: &'static str,
    level: SpeedLevel,
    col: Column,
    seed: u64,
    payload: &[u8],
    lead: usize,
) -> bool {
    let (audio, _) = capture(mode, col, seed, payload, lead);
    let mut rx = rx_engine(mode, level);
    for read in audio.chunks(READ) {
        if let Some(burst) = rx.accumulate_capture(Some(mode), read.to_vec()).unwrap() {
            if let Ok(r) = rx.ota_decode_burst(&burst, "f1", None) {
                if r.payload.as_deref() == Some(payload) {
                    return true;
                }
            }
        }
    }
    false
}

/// What one arm did: every gathered burst (start and end against the frame's onset, lead, outcome),
/// then the same frame handed to `ota_decode_burst` cut at its true onset ± 1 000 samples (oracle).
fn diagnose(
    mode: &'static str,
    level: SpeedLevel,
    col: Column,
    seed: u64,
    payload: &[u8],
    lead: usize,
) -> String {
    let (audio, len) = capture(mode, col, seed, payload, lead);
    let mut rx = rx_engine(mode, level);
    let mut out = String::new();
    let mut fed = 0usize;
    for read in audio.chunks(READ) {
        fed += read.len();
        if let Some(burst) = rx.accumulate_capture(Some(mode), read.to_vec()).unwrap() {
            let start = fed as i64 - read.len() as i64 - burst.samples.len() as i64 - lead as i64;
            let end = start + burst.samples.len() as i64;
            let verdict = match rx.ota_decode_burst(&burst, "diag", None) {
                Ok(r) if r.payload.as_deref() == Some(payload) => "ok".to_string(),
                Ok(r) => format!("no payload (mode {:?})", r.mode),
                Err(e) => format!("err {e}"),
            };
            out += &format!(
                "\n    burst [{start}, {end}) of frame [0, {len}), lead {}: {verdict}",
                rx.last_flush_lead()
            );
        }
    }
    let cut = audio[lead - 1_000..(lead + len + 1_000).min(audio.len())].to_vec();
    let mut rx = rx_engine(mode, level);
    let oracle = match rx.ota_decode_burst(
        &openpulse_modem::pipeline::AudioSamples { samples: cut },
        "oracle",
        None,
    ) {
        Ok(r) if r.payload.as_deref() == Some(payload) => "ok".to_string(),
        Ok(_) => "no payload".to_string(),
        Err(e) => format!("err {e}"),
    };
    out + &format!("\n    oracle cut: {oracle}")
}

/// The trial's payload and lead for `seed`, as `run_column` draws them.
fn draw(seed: u64, max: usize) -> (Vec<u8>, usize) {
    let mut rng = Rng(seed ^ 0xD1B5_4A32_D192_ED03);
    let len = 16 + (rng.next() % (max as u64 - 15)) as usize;
    let payload: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
    // A real lead: noise to warm the carrier detect, then the frame anywhere in a read.
    let lead = 8 * READ + (rng.next() % READ as u64) as usize;
    (payload, lead)
}

/// Paired outcomes `(shipped, candidate)` for `n` seeds of `col`, on all cores.
fn run_column(r: Rung, col: Column, n: usize) -> Vec<(bool, bool)> {
    let max = payload_max();
    let threads = std::thread::available_parallelism().map_or(4, |p| p.get());
    let mut out = vec![(false, false); n];
    std::thread::scope(|s| {
        let chunks: Vec<_> = out.chunks_mut(n.div_ceil(threads)).enumerate().collect();
        for (c, chunk) in chunks {
            let base = c * n.div_ceil(threads);
            s.spawn(move || {
                for (k, slot) in chunk.iter_mut().enumerate() {
                    let seed = (base + k) as u64 + 1;
                    let (payload, lead) = draw(seed, max);
                    *slot = (
                        trial(r.shipped, r.level, col, seed, &payload, lead),
                        trial(r.candidate, r.level, col, seed, &payload, lead),
                    );
                }
            });
        }
    });
    out
}

/// Mean and paired 95 % CI of (candidate − shipped).
fn paired_ci(pairs: &[(bool, bool)]) -> (f64, f64, f64) {
    let n = pairs.len() as f64;
    let d: Vec<f64> = pairs
        .iter()
        .map(|&(a, b)| b as u8 as f64 - a as u8 as f64)
        .collect();
    let mean = d.iter().sum::<f64>() / n;
    let var = d.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0).max(1.0);
    let half = 1.96 * (var / n).sqrt();
    (mean, mean - half, mean + half)
}

fn columns(r: Rung) -> Vec<Column> {
    // The AWGN cliff SNR is fixed from the pilot before the main run (design F1).
    let cliff: f32 = std::env::var("F1_CLIFF_DB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(f32::NAN);
    let mut c = vec![
        Column {
            name: "moderate_f1 at the floor",
            fading: true,
            snr_db: r.floor_db,
            offset_hz: 0.0,
        },
        Column {
            name: "moderate_f1 8 dB",
            fading: true,
            snr_db: 8.0,
            offset_hz: 0.0,
        },
        Column {
            name: "moderate_f1 8 dB, +50 Hz",
            fading: true,
            snr_db: 8.0,
            offset_hz: 50.0,
        },
        Column {
            name: "moderate_f1 8 dB, -50 Hz",
            fading: true,
            snr_db: 8.0,
            offset_hz: -50.0,
        },
    ];
    if cliff.is_finite() {
        c.push(Column {
            name: "AWGN at the cliff",
            fading: false,
            snr_db: cliff,
            offset_hz: 0.0,
        });
    }
    c
}

#[test]
#[ignore = "measurement: #1062 design F1/F5, release build, long"]
fn f1_bpsk250_pn63_against_the_shipped_preamble() {
    let r = rung();
    let n: usize = std::env::var("F1_TRIALS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(600);
    if std::env::var_os("F1_PILOT").is_some() {
        // `F1_PILOT_DB` overrides the sweep, comma-separated: the slow rungs' cliffs sit lower.
        let sweep: Vec<f32> = std::env::var("F1_PILOT_DB")
            .ok()
            .map(|v| v.split(',').filter_map(|x| x.trim().parse().ok()).collect())
            .unwrap_or_else(|| vec![-12.0, -10.0, -8.0, -6.0, -4.0, -2.0]);
        println!("\nF1 pilot: {}, AWGN, {n} trials per SNR", r.shipped);
        for snr in sweep {
            let col = Column {
                name: "pilot",
                fading: false,
                snr_db: snr,
                offset_hz: 0.0,
            };
            let t = std::time::Instant::now();
            let pairs = run_column(r, col, n);
            let a = pairs.iter().filter(|p| p.0).count();
            let b = pairs.iter().filter(|p| p.1).count();
            print!("  ({:.0} s)", t.elapsed().as_secs_f64());
            println!("  {snr:>5.1} dB: shipped {a}/{n}  candidate {b}/{n}");
        }
        return;
    }
    println!(
        "\nF1: {} PN-63 vs shipped, production entry, n = {n} paired, delta = {DELTA}, payload 16..={} B",
        r.shipped,
        payload_max()
    );
    let mut all_pass = true;
    for col in columns(r) {
        let t = std::time::Instant::now();
        let pairs = run_column(r, col, n);
        let a = pairs.iter().filter(|p| p.0).count();
        let b = pairs.iter().filter(|p| p.1).count();
        let disc_a = pairs.iter().filter(|p| p.0 && !p.1).count();
        let disc_b = pairs.iter().filter(|p| !p.0 && p.1).count();
        let (mean, lo, hi) = paired_ci(&pairs);
        let pass = lo >= -DELTA;
        all_pass &= pass;
        println!(
            "  {:<30} shipped {a:>4}/{n}  PN {b:>4}/{n}  discordant {disc_a}/{disc_b}  \
             PN-shipped {mean:+.3} CI [{lo:+.3}, {hi:+.3}]  {}  ({:.0} s)",
            col.name,
            if pass { "PASS" } else { "FAIL" },
            t.elapsed().as_secs_f64()
        );
    }
    println!("F1 verdict: {}", if all_pass { "PASS" } else { "FAIL" });
}

/// The harness itself: a clean frame through each arm decodes, so a FAIL above is about the
/// preamble, not a harness that cannot deliver either one.
#[test]
fn both_arms_deliver_a_clean_frame() {
    let col = Column {
        name: "clean",
        fading: false,
        snr_db: 30.0,
        offset_hz: 0.0,
    };
    let payload = b"F1 positive control".to_vec();
    let r = RUNGS[3];
    for mode in [r.shipped, r.candidate] {
        assert!(
            trial(mode, r.level, col, 7, &payload, 8 * READ + 1_234),
            "{mode}"
        );
    }
}

/// The discordant seeds of the floor column among `F1_DIAG_SEEDS` (default 1..=60), each arm
/// diagnosed: does the loss sit in the gathering or in the demodulation?
#[test]
#[ignore = "diagnostic: #1062 design F5"]
fn f5_diagnose_discordant_seeds() {
    let r = rung();
    let col = columns(r)[0];
    let n: u64 = std::env::var("F1_DIAG_SEEDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    for seed in 1..=n {
        let (payload, lead) = draw(seed, payload_max());
        let a = trial(r.shipped, r.level, col, seed, &payload, lead);
        let b = trial(r.candidate, r.level, col, seed, &payload, lead);
        if a == b {
            continue;
        }
        println!(
            "seed {seed}: shipped {a}, PN {b}, payload {} B",
            payload.len()
        );
        for mode in [r.shipped, r.candidate] {
            println!(
                "  {mode}:{}",
                diagnose(mode, r.level, col, seed, &payload, lead)
            );
        }
    }
}
