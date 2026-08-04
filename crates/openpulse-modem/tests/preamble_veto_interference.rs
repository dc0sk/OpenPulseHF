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
use openpulse_core::plugin::ModulationPlugin;
use openpulse_modem::engine::ModemEngine;
use std::f32::consts::PI;
use std::time::Duration;

/// A named interferer generator: (amplitude, sample count) -> samples.
type Shape = (&'static str, Box<dyn Fn(f32, usize) -> Vec<f32>>);

const FS: f32 = 8_000.0;
const FC: f32 = 1_500.0;
const MODE: &str = "BPSK250";
/// BPSK250's preamble lines sit at odd multiples of baud/4. This is the fundamental.
const LINE_HZ: f32 = 62.5;
/// Positions the retry scan may examine per pass. Fixed so results depend on the audio, not the CPU.
const SCAN_POSITIONS: usize = 3_000;
/// Outer receive-loop iterations. The retry budget alone is not enough — the outer loop keeps
/// scanning until the wall-clock listen deadline, so under load it completes fewer passes and
/// reaches a different verdict. Both budgets must be work-based.
const MAX_ITERATIONS: usize = 400;

fn engine() -> (LoopbackBackend, ModemEngine) {
    let backend = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(backend.clone_shared()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .expect("register bpsk");
    // Budget the scan in POSITIONS, not seconds. With the shipped wall-clock budget the same input
    // decoded on one run and failed on the next, because the pass covers however many positions the
    // machine had time for. Every number in this file would otherwise be a measure of CPU load.
    e.set_deterministic_scan_positions(Some(SCAN_POSITIONS));
    e.set_deterministic_max_iterations(Some(MAX_ITERATIONS));
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
    let shapes: Vec<Shape> = vec![
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

// ── G4: which mechanism? veto, settle-anchoring, or symbol corruption ────────

/// `BpskPlugin` with the preamble template removed and nothing else changed.
///
/// This is the counterfactual for the veto specifically: with `preamble_template` returning `None`
/// the engine falls back to the energy-only settle that every no-template mode already runs, which
/// is exactly the path #1049 was built on top of. Every other override is delegated, because an
/// incomplete delegation would silently ablate more than the veto and invalidate the comparison.
struct NoTemplateBpsk(bpsk_plugin::BpskPlugin);

impl openpulse_core::plugin::ModulationPlugin for NoTemplateBpsk {
    fn info(&self) -> &openpulse_core::plugin::PluginInfo {
        self.0.info()
    }
    fn modulate(
        &self,
        d: &[u8],
        c: &openpulse_core::plugin::ModulationConfig,
    ) -> Result<Vec<f32>, openpulse_core::error::ModemError> {
        self.0.modulate(d, c)
    }
    fn demodulate(
        &self,
        s: &[f32],
        c: &openpulse_core::plugin::ModulationConfig,
    ) -> Result<Vec<u8>, openpulse_core::error::ModemError> {
        self.0.demodulate(s, c)
    }
    fn demodulate_soft(
        &self,
        s: &[f32],
        c: &openpulse_core::plugin::ModulationConfig,
    ) -> Result<Vec<f32>, openpulse_core::error::ModemError> {
        self.0.demodulate_soft(s, c)
    }
    fn frame_geometry(
        &self,
        c: &openpulse_core::plugin::ModulationConfig,
    ) -> Option<openpulse_core::plugin::FrameGeometry> {
        self.0.frame_geometry(c)
    }
    fn estimate_snr_db(
        &self,
        s: &[f32],
        c: &openpulse_core::plugin::ModulationConfig,
    ) -> Option<f32> {
        self.0.estimate_snr_db(s, c)
    }
    fn supports_soft_demod(&self, m: &str) -> bool {
        self.0.supports_soft_demod(m)
    }
    fn estimate_afc_hz(
        &self,
        s: &[f32],
        c: &openpulse_core::plugin::ModulationConfig,
    ) -> Option<f32> {
        self.0.estimate_afc_hz(s, c)
    }
    fn occupied_bandwidth_hz(&self, m: &str) -> Option<f32> {
        self.0.occupied_bandwidth_hz(m)
    }
    fn modulate_iq(
        &self,
        d: &[u8],
        c: &openpulse_core::plugin::ModulationConfig,
    ) -> Result<(Vec<f32>, Vec<f32>), openpulse_core::error::ModemError> {
        self.0.modulate_iq(d, c)
    }
    // preamble_template deliberately NOT overridden -> trait default None -> no veto.
}

fn run_no_veto_full(signal: Vec<f32>) -> Outcome {
    let backend = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(backend.clone_shared()));
    e.register_plugin(Box::new(NoTemplateBpsk(bpsk_plugin::BpskPlugin::new())))
        .expect("register");
    e.set_deterministic_scan_positions(Some(SCAN_POSITIONS));
    e.set_deterministic_max_iterations(Some(MAX_ITERATIONS));
    e.enable_notch();
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

/// Decode-only shorthand for [`run_no_veto_full`].
fn run_no_veto(signal: Vec<f32>) -> bool {
    run_no_veto_full(signal).decoded
}

/// Byte-error rate of the raw demodulator on the exact frame span — no engine, no settle, no veto.
/// Non-zero means the interferer is corrupting symbols and the acquisition chain is a bystander.
fn demod_ber(frame: &[f32], interferer: &[f32]) -> f32 {
    let p = bpsk_plugin::BpskPlugin::new();
    let c = openpulse_core::plugin::ModulationConfig {
        mode: MODE.into(),
        sample_rate: 8_000,
        center_frequency: FC,
        ..Default::default()
    };
    let clean = match p.demodulate(frame, &c) {
        Ok(v) => v,
        Err(_) => return f32::NAN,
    };
    let mixed: Vec<f32> = frame.iter().zip(interferer).map(|(a, b)| a + b).collect();
    let got = match p.demodulate(&mixed, &c) {
        Ok(v) => v,
        Err(_) => return 1.0,
    };
    if clean.is_empty() {
        return f32::NAN;
    }
    let n = clean.len().min(got.len());
    let diff = clean[..n]
        .iter()
        .zip(&got[..n])
        .filter(|(a, b)| a != b)
        .count()
        + clean.len().abs_diff(got.len());
    diff as f32 / clean.len() as f32
}

#[test]
#[ignore = "verification"]
fn g4_which_mechanism() {
    let (backend, mut tx) = engine();
    tx.transmit_with_fec_mode(b"frame during interference", MODE, FecMode::Rs, None)
        .expect("transmit");
    let frame = backend.drain_samples();
    let frame_rms = (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt();

    const SEEDS: usize = 5;
    let cells: Vec<(&str, f32)> = vec![("comb FC-60/+65", 20.0), ("DSB FC x 60 Hz", 10.0)];

    println!("\nG4: mechanism ablation, {SEEDS} seeds per cell (decode fraction)");
    println!(
        "    lead-in = 2 s of interferer before the frame; none = interferer starts with it\n"
    );
    println!(
        "{:<20} {:>7} {:>12} {:>12} {:>12} {:>12}",
        "cell", "SIR", "baseline", "no lead-in", "veto REMOVED", "demod BER"
    );

    for (name, sir) in cells {
        let amp = frame_rms / 10f32.powf(sir / 20.0) * std::f32::consts::SQRT_2;
        let (mut base, mut nolead, mut noveto) = (0, 0, 0);
        let mut ber_sum = 0.0f32;
        for seed in 0..SEEDS {
            let pad = secs(2.0);
            let total = pad * 2 + frame.len();
            // Vary the interferer's phase per seed; at n=1 each boundary is a coin edge.
            let long = if name.starts_with("comb") {
                comb(60.0, 65.0, amp, total + 1_000)
            } else {
                dsb(60.0, amp, total + 1_000)
            };
            let off = (seed * 137) % 1_000;
            let bed = &long[off..off + total];

            let mut with_lead = bed.to_vec();
            for (i, s) in frame.iter().enumerate() {
                with_lead[pad + i] += s;
            }
            if run_cfg(with_lead.clone(), true, true).decoded {
                base += 1;
            }
            if run_no_veto(with_lead) {
                noveto += 1;
            }

            let mut no_lead = bed[..frame.len() + pad].to_vec();
            for (i, s) in frame.iter().enumerate() {
                no_lead[i] += s;
            }
            if run_cfg(no_lead, true, true).decoded {
                nolead += 1;
            }

            ber_sum += demod_ber(&frame, &bed[..frame.len()]);
        }
        println!(
            "{name:<20} {:>6.0}dB {:>10}/{SEEDS} {:>10}/{SEEDS} {:>10}/{SEEDS} {:>12.4}",
            sir,
            base,
            nolead,
            noveto,
            ber_sum / SEEDS as f32
        );
    }
    println!("\n  Reading: 'veto REMOVED' >> 'baseline' means the veto's corroboration causes the");
    println!("  loss. Similar means it is a bystander. A high demod BER means the interferer");
    println!("  corrupts symbols directly and no acquisition change can help.");
}

// ── G5: is the veto PROTECTIVE where it rejects? ─────────────────────────────

/// The cell the G4 matrix was missing, and it was missing for a reason worth naming: G4 removed the
/// veto only on inputs where it *corroborates* — where it was suspected of causing harm — and never
/// on the input it *rejects*. Ablating a mechanism only where you suspect it of guilt cannot find
/// the places it is protective.
///
/// The lone tone is the one input the veto refuses (~2500 refusals per run) and also the one that
/// survives +10/+20 dB SIR where the comb does not. If removing the veto makes the tone fail like
/// the comb, then refusal is what prevents the anchor — and the template's sequence decides which
/// interference can be refused, which is a direct #1062 consequence.
#[test]
#[ignore = "verification"]
fn g5_is_the_veto_protective_where_it_rejects() {
    let (backend, mut tx) = engine();
    tx.transmit_with_fec_mode(b"frame during interference", MODE, FecMode::Rs, None)
        .expect("transmit");
    let frame = backend.drain_samples();
    let frame_rms = (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt();

    const SEEDS: usize = 5;
    println!("\nG5: veto REMOVED on the input the veto REJECTS (lone tone), {SEEDS} seeds");
    println!("    plus the counters for the veto-less runs, which G4 did not print\n");
    println!(
        "{:<22} {:>7} {:>13} {:>14} {:>12} {:>12}",
        "input", "SIR", "veto on", "veto REMOVED", "cond(on)", "cond(off)"
    );

    let shapes: Vec<Shape> = vec![
        (
            "lone tone FC+62.5",
            Box::new(|a, n| tone(FC + LINE_HZ, a, n)),
        ),
        ("comb FC-60/+65", Box::new(|a, n| comb(60.0, 65.0, a, n))),
    ];

    for (name, make) in &shapes {
        for sir in [20.0f32, 10.0] {
            let amp = frame_rms / 10f32.powf(sir / 20.0) * std::f32::consts::SQRT_2;
            let (mut on, mut off) = (0, 0);
            let (mut cond_on, mut cond_off) = (0u64, 0u64);
            for seed in 0..SEEDS {
                let pad = secs(2.0);
                let total = pad * 2 + frame.len();
                let long = make(amp, total + 1_000);
                let offv = (seed * 137) % 1_000;
                let mut sig = long[offv..offv + total].to_vec();
                for (i, s) in frame.iter().enumerate() {
                    sig[pad + i] += s;
                }
                let a = run_cfg(sig.clone(), true, true);
                if a.decoded {
                    on += 1;
                }
                cond_on += a.condemned;
                let b = run_no_veto_full(sig);
                if b.decoded {
                    off += 1;
                }
                cond_off += b.condemned;
            }
            println!(
                "{name:<22} {sir:>6.0}dB {:>11}/{SEEDS} {:>12}/{SEEDS} {:>12} {:>12}",
                on, off, cond_on, cond_off
            );
        }
    }
    println!("\n  If the tone's 'veto REMOVED' column collapses while 'veto on' holds, refusal is");
    println!("  protective and the sequence choice decides what can be refused -- a #1062 result.");
    println!(
        "  If both hold, the veto is genuinely outcome-neutral and this files as #1021-family."
    );
}

// ── G6: WHERE does the receiver thrash? hold, or advance-and-still-fail? ─────

/// The mechanism question the counters cannot answer, and the one that decides whether a cheaper
/// non-wire fix exists.
///
/// ~550 condemnations is compatible with two different bugs. If the anchors all sit inside the
/// interferer-only lead-in, the receiver is stuck before the frame and a recovery that advanced
/// further would find it — a recovery-side fix, no wire change. If the anchors sweep across the
/// frame span and still fail, there is a second defect that a new preamble does not fix, and the
/// predicted benefit of a spread sequence is smaller than it looks.
#[test]
#[ignore = "verification"]
fn g6_where_are_the_condemned_anchors() {
    let (backend, mut tx) = engine();
    tx.transmit_with_fec_mode(b"frame during interference", MODE, FecMode::Rs, None)
        .expect("transmit");
    let frame = backend.drain_samples();
    let frame_rms = (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt();
    let pad = secs(2.0);
    let total = pad * 2 + frame.len();

    println!("\nG6: condemned anchor positions vs the frame span");
    println!(
        "    frame occupies samples {}..{} of {} ({:.1} s buffer)\n",
        pad,
        pad + frame.len(),
        total,
        total as f32 / FS
    );
    println!(
        "{:<24} {:>7} {:>6} {:>9} {:>9} {:>9} {:>11} {:>9}",
        "case", "SIR", "cond", "min", "max", "in-frame", "past frame", "decoded"
    );

    for (name, sir, veto) in [
        ("comb, veto on", 20.0f32, true),
        ("comb, veto REMOVED", 20.0, false),
        ("tone, veto REMOVED", 20.0, false),
        ("tone, veto on", 20.0, true),
    ] {
        let amp = frame_rms / 10f32.powf(sir / 20.0) * std::f32::consts::SQRT_2;
        let bed = if name.starts_with("comb") {
            comb(60.0, 65.0, amp, total)
        } else {
            tone(FC + LINE_HZ, amp, total)
        };
        let mut sig = bed.clone();
        for (i, s) in frame.iter().enumerate() {
            sig[pad + i] += s;
        }

        let backend = LoopbackBackend::new();
        let mut e = ModemEngine::new(Box::new(backend.clone_shared()));
        if veto {
            e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
                .expect("register");
        } else {
            e.register_plugin(Box::new(NoTemplateBpsk(bpsk_plugin::BpskPlugin::new())))
                .expect("register");
        }
        e.set_deterministic_scan_positions(Some(SCAN_POSITIONS));
        e.set_deterministic_max_iterations(Some(MAX_ITERATIONS));
        e.set_deterministic_max_iterations(Some(MAX_ITERATIONS));
        e.enable_notch();
        backend.fill_samples(&sig);
        let decoded = e
            .receive_with_fec_mode_timeout(MODE, FecMode::Rs, None, Duration::from_millis(20_000))
            .is_ok();
        let pos = e.condemned_positions();
        let lo = pos.iter().copied().min().unwrap_or(0);
        let hi = pos.iter().copied().max().unwrap_or(0);
        let in_frame = pos
            .iter()
            .filter(|&&p| p >= pad && p < pad + frame.len())
            .count();
        let past = pos.iter().filter(|&&p| p >= pad + frame.len()).count();
        println!(
            "{name:<24} {sir:>6.0}dB {:>6} {lo:>9} {hi:>9} {in_frame:>9} {past:>11} {:>9}",
            pos.len(),
            decoded
        );
    }
    println!(
        "\n  in-frame > 0 with decoded=false means recovery REACHED the frame and still could"
    );
    println!("  not decode it -- a second defect a new preamble would not fix. All anchors below");
    println!("  the frame start means the receiver never got there, which a recovery fix would.");
}

// ── G7: work-to-acquire, not a verdict ───────────────────────────────────────

/// The mechanism as a RATE, which is the only budget-independent way to state it.
///
/// Reporting "decodes / does not decode" hides that the budget IS the verdict: under the shipped
/// wall-clock budget on an idle machine this same case reached 582 condemnations and DECODED, while
/// a 400-iteration budget reaches 110 and does not. At ~126 samples of lead-in per condemnation,
/// 110 stops about 2 100 samples short of a frame at 16 000 — the budget was fitted to a fixture,
/// not to anything real.
///
/// So sweep the budget and report the work required, which is a property of the interference and
/// the crawl, not of the machine or the constant.
#[test]
#[ignore = "verification"]
fn g7_work_to_acquire() {
    let (backend, mut tx) = engine();
    tx.transmit_with_fec_mode(b"work to acquire", MODE, FecMode::Rs, None)
        .expect("transmit");
    let frame = backend.drain_samples();
    let frame_rms = (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt();
    let amp = frame_rms / 10f32.powf(20.0 / 20.0) * std::f32::consts::SQRT_2;

    for lead_s in [1.0f32, 2.0] {
        let pad = secs(lead_s);
        let total = pad + frame.len() + secs(1.0);
        println!(
            "\nG7: lead-in {lead_s} s ({pad} samples), frame at {pad}..{}",
            pad + frame.len()
        );
        println!(
            "{:<22} {:>10} {:>8} {:>12} {:>12} {:>12} {:>9}",
            "case", "iter cap", "cond", "anchor max", "samples/cond", "settled at", "decoded"
        );
        for &iters in &[100usize, 200, 400, 800, 1_600] {
            for (label, veto) in [("tone, veto REMOVED", false), ("comb, veto on", true)] {
                let bed = if label.starts_with("tone") {
                    tone(FC + LINE_HZ, amp, total)
                } else {
                    comb(60.0, 65.0, amp, total)
                };
                let mut sig = bed.clone();
                for (i, s) in frame.iter().enumerate() {
                    sig[pad + i] += s;
                }
                let backend = LoopbackBackend::new();
                let mut e = ModemEngine::new(Box::new(backend.clone_shared()));
                if veto {
                    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
                        .expect("reg");
                } else {
                    e.register_plugin(Box::new(NoTemplateBpsk(bpsk_plugin::BpskPlugin::new())))
                        .expect("reg");
                }
                e.set_deterministic_scan_positions(Some(SCAN_POSITIONS));
                e.set_deterministic_max_iterations(Some(iters));
                e.enable_notch();
                backend.fill_samples(&sig);
                let decoded = e
                    .receive_with_fec_mode_timeout(
                        MODE,
                        FecMode::Rs,
                        None,
                        Duration::from_millis(120_000),
                    )
                    .is_ok();
                let pos = e.condemned_positions();
                let hi = pos.iter().copied().max().unwrap_or(0);
                let rate = if pos.is_empty() {
                    0.0
                } else {
                    hi as f32 / pos.len() as f32
                };
                // Where the ACCEPTED settle landed. Without this the "walk reached the frame"
                // reading is inferred from the condemned anchors alone, and the 1 s row decodes
                // with its highest condemned anchor BELOW the frame start.
                let acc = e.accepted_settle_positions();
                let accepted = acc.last().map(|v| v.to_string()).unwrap_or("-".into());
                println!(
                    "{label:<22} {iters:>10} {:>8} {hi:>12} {rate:>12.1} {accepted:>12} {decoded:>9}",
                    pos.len()
                );
            }
        }
    }
    println!("\n  If decode flips ON once anchor max passes the frame start, the mechanism is a");
    println!("  CRAWL RATE against a budget -- not 'the receiver never advances'.");
}

// ── D: the shipped gate ──────────────────────────────────────────────────────

/// A frame preceded by interference the veto CAN refuse must still be acquired.
///
/// This is the system-level half of the steady-tone story. `preamble_correlation_settle`'s
/// `the_gate_is_not_fooled_by_a_steady_tone` tests the correlator as a component, with its grid
/// centred at 0; the deployed chain centres that grid on the AFC settle, which lands on a lone tone
/// and parks it ~baud/4 from both rotated template lines. The component and the system therefore
/// give opposite answers for the same input, and only this test covers the one that ships.
///
/// **What is deliberately NOT asserted:** that removing the veto loses the frame, and that the
/// condemnation counts differ by any particular amount. Both are properties of the present crawl
/// rate (see the work-to-acquire measurement in `g7_work_to_acquire`), and a legitimate fix to that
/// rate would break a gate written against them — for a good reason. Gate the outcome, tripwire the
/// mechanism.
///
/// Budgets are work-based because the wall-clock ones make this verdict a measure of CPU load
/// (#1066): the same case decoded 5/5 idle and 0/5 on eight busy cores before that fix.
#[test]
fn a_frame_behind_refusable_interference_is_still_acquired() {
    let (backend, mut tx) = engine();
    tx.transmit_with_fec_mode(b"refusable interference", MODE, FecMode::Rs, None)
        .expect("transmit");
    let frame = backend.drain_samples();
    let frame_rms = (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt();
    // +20 dB SIR: the interferer is well below the frame, so a failure here is an ACQUISITION
    // failure and not a signal-to-noise one.
    let amp = frame_rms / 10.0 * std::f32::consts::SQRT_2;

    let pad = secs(1.0);
    let total = pad + frame.len() + secs(1.0);
    let mut sig = tone(FC + LINE_HZ, amp, total);
    for (i, s) in frame.iter().enumerate() {
        sig[pad + i] += s;
    }

    let backend = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(backend.clone_shared()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .expect("register");
    e.set_deterministic_scan_positions(Some(SCAN_POSITIONS));
    e.set_deterministic_max_iterations(Some(MAX_ITERATIONS));
    e.enable_notch();
    backend.fill_samples(&sig);

    let got = e
        .receive_with_fec_mode_timeout(MODE, FecMode::Rs, None, Duration::from_millis(120_000))
        .unwrap_or_else(|err| {
            panic!(
                "a frame behind a steady tone at a preamble line must still be acquired: {err} \
                 (rho rejections {}, condemnations {}, accepted settles {:?})",
                e.rho_rejected_settles(),
                e.settle_condemnations(),
                e.accepted_settle_positions()
            )
        });
    assert_eq!(String::from_utf8_lossy(&got), "refusable interference");

    // TRIPWIRE 1: the veto must have actually refused something. Without this the decode above
    // passes just as well on a build where the veto never runs at all, which is the state this
    // whole test exists to distinguish from working protection.
    assert!(
        e.rho_rejected_settles() > 0,
        "the frame decoded but the correlation veto never refused a single settle — the tone is \
         supposed to be refusable, so either it is not reaching the veto or the veto is inert, and \
         this test proves nothing about protection"
    );

    // TRIPWIRE 2: the settle that led to the decode must sit on the frame, not somewhere in the
    // interferer. A decode reached by some other path would leave this far from the onset.
    let accepted = e.accepted_settle_positions().to_vec();
    let near_frame = accepted
        .iter()
        .any(|&p| p + 4 * 32 >= pad && p <= pad + frame.len());
    assert!(
        near_frame,
        "decoded, but no accepted settle landed on the frame span ({pad}..{}): accepted {accepted:?}. \
         The decode came from somewhere other than acquiring the frame where it actually is.",
        pad + frame.len()
    );
}

/// REPRODUCTION, not a gate: a frame behind interference the veto CANNOT refuse is not acquired.
///
/// The comb's sidebands sit on the template's own spectral lines, so the veto corroborates the
/// interferer instead of refusing it, the recovery crawl starts, and at ~4 symbol periods of
/// lead-in per condemnation it does not reach the frame inside any reasonable budget. Ignored
/// because it documents a defect rather than protecting a behaviour; convert it to a gate when a
/// fix lands — either a faster recovery or a sync word whose veto can refuse this shape.
#[test]
#[ignore = "reproduction for the crawl-rate acquisition defect; see #1062"]
fn a_frame_behind_unrefusable_interference_is_not_acquired() {
    let (backend, mut tx) = engine();
    tx.transmit_with_fec_mode(b"unrefusable interference", MODE, FecMode::Rs, None)
        .expect("transmit");
    let frame = backend.drain_samples();
    let frame_rms = (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt();
    let amp = frame_rms / 10.0 * std::f32::consts::SQRT_2;

    let pad = secs(2.0);
    let total = pad + frame.len() + secs(1.0);
    let mut sig = comb(60.0, 65.0, amp, total);
    for (i, s) in frame.iter().enumerate() {
        sig[pad + i] += s;
    }

    let o = run_cfg(sig, true, true);
    assert!(
        !o.decoded,
        "the comb case now DECODES — the crawl-rate defect this reproduces has been fixed, or the \
         budget changed. Promote this to a gate and update #1062."
    );
}

// ── G8: is the recovery-side fix cheaper than the wire change? ───────────────

/// The measurement owed before a wire-format break can be justified by the acquisition finding.
///
/// Two independent fixes reach a frame sitting behind continuous interference: make the veto able
/// to refuse the interference (a new sync word — wire-format change), or make the recovery cover
/// ground faster. Only the first breaks the format, so the second has to be measured first.
///
/// The stride itself is NOT the lever, and #1040 pins it for a reason: the sweep proves a span of
/// `(SWEEP_OFFSETS-1)*step/2` samples undecodable and recovery resumes exactly past it, so
/// advancing further would skip audio nobody examined. What is loose is the COST of proving that
/// span. `SETTLE_FAILURE_LIMIT` is `2 * SWEEP_OFFSETS`: one full sweep cycle, then a second "to
/// give the anchor a second chance against a grown buffer". Behind continuous interference the
/// buffer is already complete, so the second cycle re-tests what the first just rejected.
///
/// Halving it should halve the work to clear each span without skipping a single sample.
#[test]
#[ignore = "verification"]
fn g8_recovery_cost_vs_condemnation_threshold() {
    let (backend, mut tx) = engine();
    tx.transmit_with_fec_mode(b"recovery cost probe", MODE, FecMode::Rs, None)
        .expect("transmit");
    let frame = backend.drain_samples();
    let frame_rms = (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt();
    let amp = frame_rms / 10.0 * std::f32::consts::SQRT_2;

    println!("\nG8: work to acquire a frame behind continuous interference, vs condemnation cost");
    println!("    interferer: comb fc-60/fc+65 at +20 dB SIR; SETTLE_FAILURE_LIMIT shipped = 18\n");
    println!(
        "{:<10} {:>7} {:>10} {:>8} {:>12} {:>14} {:>9}",
        "lead-in", "limit", "iter cap", "cond", "anchor max", "settled at", "decoded"
    );
    for lead_s in [1.0f32, 2.0] {
        let pad = secs(lead_s);
        let total = pad + frame.len() + secs(1.0);
        let mut sig = comb(60.0, 65.0, amp, total);
        for (i, s) in frame.iter().enumerate() {
            sig[pad + i] += s;
        }
        for limit in [18usize, 9, 4, 2] {
            for iters in [400usize, 800, 1_600] {
                let backend = LoopbackBackend::new();
                let mut e = ModemEngine::new(Box::new(backend.clone_shared()));
                e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
                    .expect("reg");
                e.set_deterministic_scan_positions(Some(SCAN_POSITIONS));
                e.set_deterministic_max_iterations(Some(iters));
                e.set_settle_failure_limit(Some(limit));
                e.enable_notch();
                backend.fill_samples(&sig.clone());
                let decoded = e
                    .receive_with_fec_mode_timeout(
                        MODE,
                        FecMode::Rs,
                        None,
                        Duration::from_millis(120_000),
                    )
                    .is_ok();
                let pos = e.condemned_positions();
                let hi = pos.iter().copied().max().unwrap_or(0);
                let acc = e
                    .accepted_settle_positions()
                    .last()
                    .map(|v| v.to_string())
                    .unwrap_or("-".into());
                println!(
                    "{lead_s:<10} {limit:>7} {iters:>10} {:>8} {hi:>12} {acc:>14} {decoded:>9}",
                    pos.len()
                );
            }
        }
    }
    println!("\n  If a lower limit reaches the same anchor position for proportionally fewer");
    println!("  iterations, the recovery-side fix works and no wire change is needed for THIS.");
}

// ── G9: the recovery fix's COST side, on a real capture ─────────────────────

/// G8 measured only where every anchor is hopeless. This measures where anchors are nearly right.
///
/// Lowering `SETTLE_FAILURE_LIMIT` makes each condemnation cheaper, which is pure gain when the
/// anchor is sitting on interference and no number of attempts would have decoded it. It is a loss
/// when the anchor is on a frame's leading edge, where a later attempt against a grown buffer WOULD
/// have decoded — the receiver then abandons a good anchor and has to walk back to it.
///
/// The #1021 on-air capture has exactly that shape, so it is the counterfactual G8 lacks. Total
/// work is `condemnations x limit`, which is the column that decides.
#[test]
#[ignore = "verification"]
fn g9_recovery_cost_on_the_real_capture() {
    use openpulse_modem::capture_replay::load_corpus;
    let c = load_corpus("ic9700-frame-bpsk250-rs-whitened.wav").expect("corpus");

    println!("\nG9: #1021 on-air capture, condemnations and TOTAL attempts vs condemnation limit");
    println!("    shipped limit = 18; the #1040 gate bounds condemnations at <= 2\n");
    println!(
        "{:>7} {:>14} {:>16} {:>10}",
        "limit", "condemnations", "total attempts", "decoded"
    );
    for limit in [18usize, 12, 9, 6, 4, 2] {
        let mut h = openpulse_modem::channel_sim::ChannelSimHarness::new();
        for eng in [&mut h.tx_engine, &mut h.rx_engine] {
            eng.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
                .expect("reg");
        }
        h.rx_engine.set_settle_failure_limit(Some(limit));
        // Deterministic budgets, or these counts measure the CPU (#1066) -- publishing
        // wall-clock-bound constants in the same series that filed #1066 would be self-refuting.
        h.rx_engine
            .set_deterministic_scan_positions(Some(SCAN_POSITIONS));
        h.rx_engine
            .set_deterministic_max_iterations(Some(MAX_ITERATIONS * 4));
        h.feed_capture(&c);
        let decoded = h
            .rx_engine
            .receive_with_fec_mode_timeout(MODE, FecMode::Rs, None, Duration::from_millis(40_000))
            .is_ok();
        let cond = h.rx_engine.settle_condemnations();
        println!(
            "{limit:>7} {cond:>14} {:>16} {decoded:>10}",
            cond as usize * limit
        );
    }
    println!(
        "\n  Total attempts is the honest cost. If it rises as the limit falls, the receiver is"
    );
    println!(
        "  abandoning anchors that would have decoded, and 'make condemnation cheaper' is not"
    );
    println!("  a free win — it is a trade against anchors that are nearly right.");
}
