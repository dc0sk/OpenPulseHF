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
//! `F1_TRIALS` overrides n; `F1_PILOT=1` runs the shipped arm alone over an AWGN SNR sweep to locate
//! the cliff, which is then fixed in the design before the main run. Release build:
//! `cargo test --release -p openpulse-modem --no-default-features --test pn_preamble_parity -- --ignored --nocapture`.

use openpulse_audio::LoopbackBackend;
use openpulse_channel::cfo::{CfoChannel, CfoConfig};
use openpulse_channel::watterson::WattersonChannel;
use openpulse_channel::{ChannelModel, WattersonConfig};
use openpulse_core::fec::FecMode;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::ModemEngine;

const SHIPPED: &str = "BPSK250";
const CANDIDATE: &str = "BPSK250-PN";
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

fn rx_engine(mode: &'static str) -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(LoopbackBackend::new()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::pn_candidate()))
        .unwrap();
    e.register_plugin(Box::new(fsk4_plugin::Fsk4Plugin::new()))
        .unwrap();
    let profile = SessionProfile::from_rungs(
        &[(SpeedLevel::Sl5, mode, FecMode::Rs, Some(5.0), None)],
        SpeedLevel::Sl5,
        3,
    );
    e.start_ota_session(profile);
    e.ota_lock_level(SpeedLevel::Sl5);
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

/// One arm of one trial: does `mode` deliver `payload` through the production receive path?
fn trial(mode: &'static str, col: Column, seed: u64, payload: &[u8], lead: usize) -> bool {
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
    let mut rx = rx_engine(mode);
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

/// Paired outcomes `(shipped, candidate)` for `n` seeds of `col`, on all cores.
fn run_column(col: Column, n: usize) -> Vec<(bool, bool)> {
    let threads = std::thread::available_parallelism().map_or(4, |p| p.get());
    let mut out = vec![(false, false); n];
    std::thread::scope(|s| {
        let chunks: Vec<_> = out.chunks_mut(n.div_ceil(threads)).enumerate().collect();
        for (c, chunk) in chunks {
            let base = c * n.div_ceil(threads);
            s.spawn(move || {
                for (k, slot) in chunk.iter_mut().enumerate() {
                    let seed = (base + k) as u64 + 1;
                    let mut rng = Rng(seed ^ 0xD1B5_4A32_D192_ED03);
                    let len = 16 + (rng.next() % 185) as usize;
                    let payload: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
                    // A real lead: noise to warm the carrier detect, then the frame anywhere in a read.
                    let lead = 8 * READ + (rng.next() % READ as u64) as usize;
                    *slot = (
                        trial(SHIPPED, col, seed, &payload, lead),
                        trial(CANDIDATE, col, seed, &payload, lead),
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

fn columns() -> Vec<Column> {
    // The AWGN cliff SNR is fixed from the pilot before the main run (design F1).
    let cliff: f32 = std::env::var("F1_CLIFF_DB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(f32::NAN);
    let mut c = vec![
        Column {
            name: "moderate_f1 5 dB (SL5 floor)",
            fading: true,
            snr_db: 5.0,
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
#[ignore = "measurement: #1062 design F1, release build, long"]
fn f1_bpsk250_pn63_against_the_shipped_preamble() {
    let n: usize = std::env::var("F1_TRIALS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(600);
    if std::env::var_os("F1_PILOT").is_some() {
        println!("\nF1 pilot: shipped arm, AWGN, {n} trials per SNR");
        for snr in [-12.0f32, -10.0, -8.0, -6.0, -4.0, -2.0] {
            let col = Column {
                name: "pilot",
                fading: false,
                snr_db: snr,
                offset_hz: 0.0,
            };
            let pairs = run_column(col, n);
            let a = pairs.iter().filter(|p| p.0).count();
            let b = pairs.iter().filter(|p| p.1).count();
            println!("  {snr:>5.1} dB: shipped {a}/{n}  candidate {b}/{n}");
        }
        return;
    }
    println!("\nF1: BPSK250 PN-63 vs shipped, production entry, n = {n} paired, delta = {DELTA}");
    let mut all_pass = true;
    for col in columns() {
        let t = std::time::Instant::now();
        let pairs = run_column(col, n);
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
    for mode in [SHIPPED, CANDIDATE] {
        assert!(trial(mode, col, 7, &payload, 8 * READ + 1_234), "{mode}");
    }
}
