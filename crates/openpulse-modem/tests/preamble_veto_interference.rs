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

/// Mix an interferer with a broadband noise floor at a given interferer power fraction.
///
/// A noiseless full-amplitude interferer opens the energy gate trivially and puts every row at
/// 100% interferer power — the reverse of an artificially-easy fixture, and it cannot support any
/// claim about how often the class occurs.
fn at_power_fraction(interferer: &[f32], frac: f32, seed: u64) -> Vec<f32> {
    let n = interferer.len();
    let p_i: f32 = interferer.iter().map(|s| s * s).sum::<f32>() / n as f32;
    let target_noise_p = p_i * (1.0 - frac) / frac.max(1e-6);
    let nz = noise(1.0, n, seed);
    let p_n: f32 = nz.iter().map(|s| s * s).sum::<f32>() / n as f32;
    let g = (target_noise_p / p_n.max(1e-12)).sqrt();
    interferer.iter().zip(nz).map(|(a, b)| a + b * g).collect()
}

struct Outcome {
    accepted: u64,
    rejected: u64,
    condemned: u64,
    correction_hz: f32,
    decoded: bool,
}

/// Feed `signal` through the real receive path under a chosen configuration.
fn run_cfg(signal: Vec<f32>, notch: bool, afc: bool) -> Outcome {
    let (backend, mut e) = engine();
    if notch {
        e.enable_notch();
    }
    if !afc {
        e.disable_afc();
    }
    backend.fill_samples(&signal);
    let decoded = e
        .receive_with_fec_mode_timeout(MODE, FecMode::Rs, None, Duration::from_millis(20_000))
        .is_ok();
    Outcome {
        accepted: e.rho_accepted_settles(),
        rejected: e.rho_rejected_settles(),
        condemned: e.settle_condemnations(),
        correction_hz: e.afc_correction_hz(),
        decoded,
    }
}

/// Feed `signal` through the real receive path; return (accepted, rejected, decoded).
fn run(signal: Vec<f32>) -> (u64, u64, bool) {
    let o = run_cfg(signal, false, true);
    (o.accepted, o.rejected, o.decoded)
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

// ── G2: does the finding hold in the configuration the shipped binaries run? ──

/// The G1 table ran `notch_enabled = false`, which is `ModemEngine`'s default but NOT the shipped
/// one — `openpulse-config` defaults the notch ON (REQ-QRM-01). It also ran AFC enabled, which is
/// what parks a lone tone at the safe point; with AFC off the grid centres at 0 and the
/// component-level tone exploit is expected back. Both are boundaries of the G1 result, and neither
/// can be inferred from a doc comment.
///
/// `condemned` distinguishes the two readings of `accepted == 1`: zero condemnations means the
/// corroborated settle held the whole window, non-zero means recovery cycled without re-corroborating.
#[test]
#[ignore = "verification"]
fn g2_config_boundaries_and_sir() {
    let n = secs(4.0);
    let cases: Vec<(String, Vec<f32>)> = vec![
        ("AM FC 60 Hz m=1.0".into(), am(60.0, 1.0, 0.3, n)),
        ("DSB FC x 60 Hz".into(), dsb(60.0, 0.3, n)),
        ("comb FC-60 / FC+65".into(), comb(60.0, 65.0, 0.3, n)),
        (
            format!("lone tone FC+{LINE_HZ}"),
            tone(FC + LINE_HZ, 0.3, n),
        ),
    ];

    println!("\nG2a: configuration boundaries (interferer alone, no noise floor)");
    println!(
        "{:<26} {:>16} {:>9} {:>9} {:>10} {:>12}",
        "input", "config", "accept", "reject", "condemned", "afc corr Hz"
    );
    for (name, sig) in &cases {
        for (label, notch, afc) in [
            ("notch off, afc on", false, true),
            ("notch ON (shipped)", true, true),
            ("notch off, afc OFF", false, false),
        ] {
            let o = run_cfg(sig.clone(), notch, afc);
            println!(
                "{name:<26} {label:>16} {:>9} {:>9} {:>10} {:>12.1}",
                o.accepted, o.rejected, o.condemned, o.correction_hz
            );
        }
    }

    println!("\nG2b: interferer power fraction sweep (notch ON, afc on — the shipped config)");
    println!(
        "{:<26} {:>8} {:>8} {:>8} {:>8} {:>8}",
        "input", "100%", "70%", "50%", "30%", "15%"
    );
    for (name, sig) in &cases {
        let mut row = vec![];
        for frac in [1.0f32, 0.7, 0.5, 0.3, 0.15] {
            let mixed = if frac >= 1.0 {
                sig.clone()
            } else {
                at_power_fraction(sig, frac, 31 + (frac * 100.0) as u64)
            };
            let o = run_cfg(mixed, true, true);
            row.push(if o.accepted > 0 {
                "DEFEAT".to_string()
            } else if o.rejected > 0 {
                "block".to_string()
            } else {
                "-".to_string()
            });
        }
        println!(
            "{name:<26} {:>8} {:>8} {:>8} {:>8} {:>8}",
            row[0], row[1], row[2], row[3], row[4]
        );
    }
    println!("  DEFEAT = a settle was corroborated; block = settles attempted and all refused;");
    println!("  '-' = the chain never reached the veto at all.");
}

// ── G3: does interference actually cost a real frame? ────────────────────────

/// The harm question. A corroborated settle on interference is only a defect if a frame arriving
/// during it fails, or is delayed. Equally, a *rejected* settle is not automatically cheap — the
/// lone tone costs thousands of refusals, and whether that starves a real frame is unmeasured.
#[test]
#[ignore = "verification"]
fn g3_does_a_frame_survive_the_interference() {
    let (backend, mut tx) = engine();
    tx.transmit_with_fec_mode(b"frame during interference", MODE, FecMode::Rs, None)
        .expect("transmit");
    let frame = backend.drain_samples();

    let n_pad = secs(2.0);
    let total = n_pad * 2 + frame.len();
    let frame_rms = (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt();

    // Sweep the interferer against the FRAME's own level. The first pass fixed it at 0.05, which is
    // ~30 dB below the frame -- every row decoded, which says nothing about harm. SIR is the
    // variable that decides whether a corroborated settle costs anything.
    let sirs_db = [20.0f32, 10.0, 0.0, -6.0, -12.0];
    let shapes: Vec<(&str, Box<dyn Fn(f32, usize) -> Vec<f32>>)> = vec![
        ("noise floor (control)", Box::new(|a, n| noise(a, n, 5))),
        (
            "lone tone FC+62.5",
            Box::new(|a, n| tone(FC + LINE_HZ, a, n)),
        ),
        ("DSB FC x 60 Hz", Box::new(|a, n| dsb(60.0, a, n))),
        (
            "comb FC-60 / FC+65",
            Box::new(|a, n| comb(60.0, 65.0, a, n)),
        ),
    ];

    println!("\nG3: a real frame embedded in continuous interference (notch ON, shipped config)");
    println!("    decoded? at each signal-to-interference ratio, frame RMS {frame_rms:.4}\n");
    println!(
        "{:<26} {:>10} {:>10} {:>10} {:>10} {:>10}",
        "interferer", "+20 dB", "+10 dB", "0 dB", "-6 dB", "-12 dB"
    );
    for (name, make) in &shapes {
        let mut row = vec![];
        for sir in sirs_db {
            let amp = frame_rms / 10f32.powf(sir / 20.0) * std::f32::consts::SQRT_2;
            let bed = make(amp, total);
            let mut sig = bed.clone();
            for (i, s) in frame.iter().enumerate() {
                if n_pad + i < sig.len() {
                    sig[n_pad + i] += s;
                }
            }
            let o = run_cfg(sig, true, true);
            row.push(format!(
                "{}{}",
                if o.decoded { "OK" } else { "FAIL" },
                if o.accepted > 0 { "" } else { "*" }
            ));
        }
        println!(
            "{name:<26} {:>10} {:>10} {:>10} {:>10} {:>10}",
            row[0], row[1], row[2], row[3], row[4]
        );
    }
    println!("\n  * = no settle was ever corroborated in that run.");
    println!("  The control row is the counterfactual: broadband noise at the same SIR. An");
    println!("  interferer shape only indicts the veto where it does WORSE than the control.");
}
