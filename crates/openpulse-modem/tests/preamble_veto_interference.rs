//! RESEARCH HARNESS — does line-frequency interference corroborate a settle END-TO-END? No asserts.
//!
//! Correlating a template against an interferer by hand answers a different question from the one
//! that matters, and answers it wrongly. The deployed veto centres its residual-frequency grid on
//! the settle's own estimate (`engine.rs` `preamble_search_plan`: `settled_hz + k*step`), not on
//! zero — so for a lone tone the AFC settle first estimates the tone's own offset, and the rotated
//! template's lines land a fixed distance away. A bench measurement with the grid pinned at zero
//! reports an exploit the shipped chain may already block.
//!
//! This drives `ModemEngine::receive_with_timeout`, which is the entry that actually runs
//! `EnergyGate -> refine_onset -> afc_mini_settle -> preamble veto`, and reads the engine's own
//! accept/reject counters. Note the daemon's streaming path (`accumulate_capture`) does NOT reach
//! this code — the veto lives in `receive_with_timeout_fec` — so "production path" here means the
//! CLI/receive family, and the daemon remains uncovered by #1049 entirely.

use openpulse_audio::loopback::LoopbackBackend;
use openpulse_core::fec::FecMode;
use openpulse_modem::engine::ModemEngine;
use std::f32::consts::PI;
use std::time::Duration;

const FS: f32 = 8_000.0;
const FC: f32 = 1_500.0;
const MODE: &str = "BPSK250";
/// BPSK250's preamble lines sit at odd multiples of baud/4. This is the fundamental.
const LINE_HZ: f32 = 62.5;

fn engine() -> (LoopbackBackend, ModemEngine) {
    let backend = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(backend.clone_shared()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .expect("register bpsk");
    (backend, e)
}

fn secs(n: f32) -> usize {
    (n * FS) as usize
}

/// A pure tone at `f`, amplitude `a`.
fn tone(f: f32, a: f32, n: usize) -> Vec<f32> {
    (0..n)
        .map(|k| a * (2.0 * PI * f * k as f32 / FS).cos())
        .collect()
}

/// Carrier at FC amplitude-modulated at `m_hz` with depth `depth`.
///
/// Sidebands land at FC +/- m_hz. With `m_hz` near 62.5 they sit on the template's lines while the
/// carrier itself stays at FC — so the squaring AFC estimator sees a legitimate line at 2*FC and
/// settles near zero, leaving the grid centred where the sidebands are reachable.
fn am(m_hz: f32, depth: f32, a: f32, n: usize) -> Vec<f32> {
    (0..n)
        .map(|k| {
            let t = k as f32 / FS;
            a * (1.0 + depth * (2.0 * PI * m_hz * t).cos()) * (2.0 * PI * FC * t).cos()
        })
        .collect()
}

/// Double-sideband suppressed carrier: two lines at FC +/- m_hz and nothing at FC.
fn dsb(m_hz: f32, a: f32, n: usize) -> Vec<f32> {
    (0..n)
        .map(|k| {
            let t = k as f32 / FS;
            a * (2.0 * PI * m_hz * t).cos() * (2.0 * PI * FC * t).cos()
        })
        .collect()
}

/// Two unequal tones straddling FC — an asymmetric birdie pair, the shape a switching supply makes.
fn comb(lo_off: f32, hi_off: f32, a: f32, n: usize) -> Vec<f32> {
    let l = tone(FC - lo_off, a, n);
    let h = tone(FC + hi_off, a * 0.8, n);
    l.iter().zip(h).map(|(x, y)| x + y).collect()
}

fn noise(a: f32, n: usize, seed: u64) -> Vec<f32> {
    let mut s = seed | 1;
    (0..n)
        .map(|_| {
            s ^= s >> 12;
            s ^= s << 25;
            s ^= s >> 27;
            let u = (s.wrapping_mul(0x2545F4914F6CDD1D) >> 33) as f32 / (1u64 << 31) as f32;
            (u * 2.0 - 1.0) * a
        })
        .collect()
}

/// Feed `signal` through the real receive path; return (accepted, rejected, decoded).
fn run(signal: Vec<f32>) -> (u64, u64, bool) {
    let (backend, mut e) = engine();
    backend.fill_samples(&signal);
    let decoded = e
        .receive_with_fec_mode_timeout(MODE, FecMode::Rs, None, Duration::from_millis(20_000))
        .is_ok();
    (e.rho_accepted_settles(), e.rho_rejected_settles(), decoded)
}

#[test]
#[ignore = "verification"]
fn g1_does_line_interference_corroborate_a_settle() {
    // FILTER VALIDATION, first and non-negotiable: a real frame must produce a non-zero ACCEPT
    // count. Without this row, every "0 accepts" below is a claim about the counter, not about the
    // interferer -- the counter stays 0 when the veto never runs at all.
    let (backend, mut tx) = engine();
    tx.transmit_with_fec_mode(b"veto interference probe", MODE, FecMode::Rs, None)
        .expect("transmit");
    let frame = backend.drain_samples();
    let mut real: Vec<f32> = noise(0.002, secs(1.0), 7);
    real.extend_from_slice(&frame);
    real.extend(noise(0.002, secs(1.0), 9));

    let cases: Vec<(String, Vec<f32>)> = vec![
        ("REAL FRAME (filter validation)".into(), real),
        ("silence".into(), vec![0.0; secs(4.0)]),
        ("broadband noise".into(), noise(0.05, secs(4.0), 4242)),
        (
            format!("lone tone FC+{LINE_HZ}"),
            tone(FC + LINE_HZ, 0.3, secs(4.0)),
        ),
        (
            format!("lone tone FC-{LINE_HZ}"),
            tone(FC - LINE_HZ, 0.3, secs(4.0)),
        ),
        ("lone tone FC".into(), tone(FC, 0.3, secs(4.0))),
        (
            "AM FC, 60 Hz, depth 1.0".into(),
            am(60.0, 1.0, 0.3, secs(4.0)),
        ),
        (
            format!("AM FC, {LINE_HZ} Hz, depth 1.0"),
            am(LINE_HZ, 1.0, 0.3, secs(4.0)),
        ),
        ("DSB FC x 60 Hz".into(), dsb(60.0, 0.3, secs(4.0))),
        (
            format!("DSB FC x {LINE_HZ} Hz"),
            dsb(LINE_HZ, 0.3, secs(4.0)),
        ),
        (
            "comb FC-60 / FC+65".into(),
            comb(60.0, 65.0, 0.3, secs(4.0)),
        ),
    ];

    println!(
        "\nG1: settles ACCEPTED vs REJECTED by the preamble veto, through receive_with_timeout"
    );
    println!("    mode {MODE}, threshold 0.40, grid centred on the settle as the engine does it\n");
    println!(
        "{:<34} {:>9} {:>9} {:>9} {:>9}",
        "input", "accepted", "rejected", "decoded", "verdict"
    );
    for (name, sig) in cases {
        let (acc, rej, dec) = run(sig);
        let verdict = if name.starts_with("REAL") {
            if acc > 0 {
                "VALID"
            } else {
                "COUNTER DEAD"
            }
        } else if acc > 0 {
            "DEFEATS"
        } else if rej > 0 {
            "blocked"
        } else {
            "no settle"
        };
        println!("{name:<34} {acc:>9} {rej:>9} {:>9} {verdict:>9}", dec);
    }
    println!(
        "\n  'no settle' means the chain never reached the veto -- the energy gate or the onset\n  \
         refinement stopped it first, which is a different defence and a different fragility."
    );
}
