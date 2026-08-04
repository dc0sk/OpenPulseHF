//! PHASE 0 BENCH — is a decimated correlator equivalent to the passband one?
//!
//! #1062's design pass proposes lifting `MAX_PREAMBLE_CORRELATION_SAMPLES` for the slow rungs by
//! correlating at complex baseband: mix to `fc + settled offset`, lowpass, decimate by `D`. That
//! would extend the #1049 veto to BPSK31/63/100, which are exempt today **only** because their
//! templates exceed the cap — and it needs no wire-format change, which is why it is phase 0.
//!
//! The design says this measurement kills the whole correlation-cost section if it fails. It is run
//! before anything is built on top of it.
//!
//! **Two cases, and only the first is an equivalence test.** The DDC's lowpass removes out-of-band
//! energy from the correlation window, which changes ρ's denominator. On an in-band signal that is a
//! no-op and ρ must match. On a signal carrying out-of-band interference it must NOT match — ρ_DDC
//! should be *higher*, because the interference has been filtered out before the correlation. That
//! second case is the design's claimed interference-rejection benefit, and measuring it as a
//! deviation rather than an error is the difference between validating the claim and refuting it.

use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use openpulse_dsp::acquisition::IqMatchedFilter;
use std::f32::consts::PI;

const FS: f32 = 8_000.0;
const FC: f32 = 1_500.0;

fn cfg(mode: &str) -> ModulationConfig {
    ModulationConfig {
        mode: mode.into(),
        sample_rate: 8_000,
        center_frequency: FC,
        ..Default::default()
    }
}

/// Windowed-sinc lowpass, cutoff in Hz at the pre-decimation rate.
fn lowpass_taps(cutoff_hz: f32, n: usize) -> Vec<f32> {
    let fc_norm = cutoff_hz / FS;
    let m = n as f32 - 1.0;
    (0..n)
        .map(|i| {
            let x = i as f32 - m / 2.0;
            let sinc = if x.abs() < 1e-6 {
                2.0 * fc_norm
            } else {
                (2.0 * PI * fc_norm * x).sin() / (PI * x)
            };
            // Hamming
            let w = 0.54 - 0.46 * (2.0 * PI * i as f32 / m).cos();
            sinc * w
        })
        .collect()
}

/// Mix to complex baseband at `f_hz`, lowpass, decimate by `d`.
fn ddc(x: &[f32], f_hz: f32, cutoff_hz: f32, d: usize, taps: &[f32]) -> Vec<(f32, f32)> {
    let mixed: Vec<(f32, f32)> = x
        .iter()
        .enumerate()
        .map(|(n, &s)| {
            let ph = -2.0 * PI * f_hz * n as f32 / FS;
            (s * ph.cos(), s * ph.sin())
        })
        .collect();
    let _ = cutoff_hz;
    let mut out = Vec::with_capacity(mixed.len() / d + 1);
    let ntap = taps.len();
    let mut n = 0usize;
    while n < mixed.len() {
        if n + 1 >= ntap {
            let (mut i, mut q) = (0.0f32, 0.0f32);
            for (k, &t) in taps.iter().enumerate() {
                let s = mixed[n - k];
                i += t * s.0;
                q += t * s.1;
            }
            if (n - ntap + 1).is_multiple_of(d) {
                out.push((i, q));
            }
        }
        n += 1;
    }
    out
}

/// Normalised complex correlation of `t` against the window of `w` starting at 0.
fn rho_complex(w: &[(f32, f32)], t: &[(f32, f32)]) -> f32 {
    let n = t.len().min(w.len());
    let (mut ri, mut rq, mut we, mut te) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for k in 0..n {
        let (wi, wq) = w[k];
        let (ti, tq) = t[k];
        ri += (wi * ti + wq * tq) as f64;
        rq += (wq * ti - wi * tq) as f64;
        we += (wi * wi + wq * wq) as f64;
        te += (ti * ti + tq * tq) as f64;
    }
    if we <= 0.0 || te <= 0.0 {
        return 0.0;
    }
    ((ri * ri + rq * rq).sqrt() / (we * te).sqrt()) as f32
}

/// Best rho over onsets, DDC path.
fn rho_ddc_best(sig: &[f32], template: &[f32], d: usize, cutoff: f32) -> f32 {
    let taps = lowpass_taps(cutoff, 129);
    let tb = ddc(template, FC, cutoff, d, &taps);
    let sb = ddc(sig, FC, cutoff, d, &taps);
    if sb.len() <= tb.len() || tb.is_empty() {
        return f32::NAN;
    }
    let mut best = 0.0f32;
    for off in 0..(sb.len() - tb.len()) {
        best = best.max(rho_complex(&sb[off..], &tb));
    }
    best
}

/// Best rho over onsets, shipped passband path.
fn rho_passband_best(sig: &[f32], template: &[f32]) -> f32 {
    let mf = IqMatchedFilter::new(template.to_vec());
    if sig.len() <= mf.len() {
        return f32::NAN;
    }
    mf.search_normalized(sig, sig.len() - mf.len(), 0.05)
        .map(|r| r.rho)
        .unwrap_or(f32::NAN)
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

#[test]
#[ignore = "verification"]
fn p1_ddc_matches_passband_on_an_in_band_signal() {
    let p = bpsk_plugin::BpskPlugin::new();
    let template = p
        .preamble_template(&cfg("BPSK250"))
        .expect("BPSK250 template")
        .samples;
    let frame = p
        .modulate(b"ddc equivalence bench", &cfg("BPSK250"))
        .unwrap();

    println!(
        "\nP1: rho, passband correlator vs DDC, BPSK250 template ({} samples)",
        template.len()
    );
    println!(
        "    in-band cases: these must MATCH. cutoff 400 Hz (signal ~250 Hz + grid + skirt)\n"
    );
    println!(
        "{:<34} {:>7} {:>12} {:>12} {:>10}",
        "signal", "D", "passband", "DDC", "delta"
    );

    let cases: Vec<(String, Vec<f32>)> = vec![
        ("clean frame".into(), frame.clone()),
        ("frame + in-band noise (20 dB)".into(), {
            let n = noise(0.03, frame.len(), 7);
            frame.iter().zip(n).map(|(a, b)| a + b).collect()
        }),
        ("frame + in-band noise (6 dB)".into(), {
            let n = noise(0.15, frame.len(), 11);
            frame.iter().zip(n).map(|(a, b)| a + b).collect()
        }),
    ];

    for (name, sig) in &cases {
        let pb = rho_passband_best(sig, &template);
        for d in [2usize, 4, 8, 16, 32] {
            let dd = rho_ddc_best(sig, &template, d, 400.0);
            println!("{name:<34} {d:>7} {pb:>12.4} {dd:>12.4} {:>10.4}", dd - pb);
        }
    }
    println!("\n  A delta near zero is the claim. A delta that grows with D locates where the");
    println!("  decimation starts destroying the correlation, which sets the real budget.");
}

/// Band-limited noise via FFT masking, so "in-band" means what it says.
fn band_noise(n: usize, lo: f32, hi: f32, a: f32, seed: u64) -> Vec<f32> {
    use rustfft::{num_complex::Complex, FftPlanner};
    let mut buf: Vec<Complex<f32>> = noise(1.0, n, seed)
        .into_iter()
        .map(|v| Complex::new(v, 0.0))
        .collect();
    let mut pl = FftPlanner::new();
    pl.plan_fft_forward(n).process(&mut buf);
    let bin = FS / n as f32;
    for (k, v) in buf.iter_mut().enumerate() {
        let f = if k <= n / 2 {
            k as f32 * bin
        } else {
            (n - k) as f32 * bin
        };
        if f < lo || f > hi {
            *v = Complex::new(0.0, 0.0);
        }
    }
    pl.plan_fft_inverse(n).process(&mut buf);
    let out: Vec<f32> = buf.iter().map(|c| c.re).collect();
    let rms = (out.iter().map(|s| s * s).sum::<f32>() / n as f32)
        .sqrt()
        .max(1e-12);
    out.iter().map(|s| s / rms * a).collect()
}

/// P2 separates the two things P1's mislabelled cases ran together.
///
/// P1 called white noise "in-band". It is not — the DDC's lowpass removes most of it, so P1's
/// small positive deltas were the rejection benefit appearing inside what was presented as an
/// equivalence test. Here the two are measured apart:
///
/// * **Truly in-band** noise, confined to the DDC passband: ρ must MATCH. Any deviation is the
///   decimation damaging the correlation, which is what would kill the design.
/// * **Out-of-band** interference outside the cutoff: ρ_DDC must be HIGHER. That is the claimed
///   interference-rejection benefit, and it is only a benefit if it can be shown as a deviation in
///   the right direction rather than assumed.
#[test]
#[ignore = "verification"]
fn p2_in_band_equivalence_and_out_of_band_benefit() {
    let p = bpsk_plugin::BpskPlugin::new();
    let template = p
        .preamble_template(&cfg("BPSK250"))
        .expect("template")
        .samples;
    let frame = p
        .modulate(b"ddc equivalence bench", &cfg("BPSK250"))
        .unwrap();
    let n = frame.len();
    let cutoff = 400.0f32;

    println!("\nP2: equivalence vs benefit, separated. DDC cutoff {cutoff} Hz around fc={FC}\n");
    println!(
        "{:<40} {:>7} {:>11} {:>11} {:>10} {:>9}",
        "signal", "D", "passband", "DDC", "delta", "expect"
    );

    let mix =
        |a: &[f32], b: Vec<f32>| -> Vec<f32> { a.iter().zip(b).map(|(x, y)| x + y).collect() };
    let cases: Vec<(String, Vec<f32>, &str)> = vec![
        (
            "frame + IN-BAND noise (1300-1700 Hz)".into(),
            mix(&frame, band_noise(n, 1_300.0, 1_700.0, 0.10, 21)),
            "match",
        ),
        (
            "frame + OUT-OF-BAND tone (fc+1200)".into(),
            mix(
                &frame,
                (0..n)
                    .map(|k| 0.30 * (2.0 * PI * (FC + 1_200.0) * k as f32 / FS).cos())
                    .collect(),
            ),
            "DDC>pb",
        ),
        (
            "frame + OUT-OF-BAND noise (2600-3800)".into(),
            mix(&frame, band_noise(n, 2_600.0, 3_800.0, 0.20, 33)),
            "DDC>pb",
        ),
    ];

    for (name, sig, expect) in &cases {
        let pb = rho_passband_best(sig, &template);
        for d in [4usize, 16, 32] {
            let dd = rho_ddc_best(sig, &template, d, cutoff);
            println!(
                "{name:<40} {d:>7} {pb:>11.4} {dd:>11.4} {:>10.4} {expect:>9}",
                dd - pb
            );
        }
    }
    println!(
        "\n  Row 1 near zero = decimation preserves the correlation. Rows 2-3 strongly positive"
    );
    println!(
        "  = the anti-alias stage is rejecting interference before it reaches the correlator,"
    );
    println!("  which is the design's claim that it is a benefit rather than a cost.");
}

/// P3: does a steady tone actually score high at BPSK31 with the shipped ±20 Hz grid?
///
/// The grid finding is arithmetic on a model — the preamble's energy sits in lines at odd multiples
/// of `baud/4`, so adjacent lines are `baud/2` apart, and a grid wider than half that spacing can
/// rotate some line onto any frequency. At BPSK31 the spacing is 15.6 Hz against a ±20 Hz grid.
///
/// A model is not a measurement, and this one is load-bearing: the veto's entire frequency bound
/// rests on it. So sweep tones against BPSK31's own template and see whether ρ really is elevated
/// everywhere, rather than filing the prediction.
///
/// BPSK31 publishes no template (its constants are not derived — that is the point of the finding),
/// so the template is built directly from the modulator, which is what a derived-constants version
/// of that mode would publish.
#[test]
#[ignore = "verification"]
fn p3_does_the_grid_reach_a_line_at_every_frequency() {
    for (mode, grid_hz) in [("BPSK31", 20.0f32), ("BPSK63", 20.0), ("BPSK250", 20.0)] {
        let template = match bpsk_plugin::modulate::bpsk_preamble_template(&cfg(mode)) {
            Ok(t) => t,
            Err(e) => {
                println!("{mode}: no template ({e})");
                continue;
            }
        };
        let baud: f32 = mode.trim_start_matches("BPSK").parse().unwrap_or(31.25);
        let baud = if mode == "BPSK31" { 31.25 } else { baud };
        let mf = IqMatchedFilter::new(template.clone());
        let tlen = template.len();
        let step = (0.25 * FS / tlen as f32).max(0.5);
        let n = (grid_hz / step).round() as i32;
        let grid: Vec<f32> = (-n..=n).map(|k| k as f32 * step).collect();

        // Sweep FOUR line-spacing periods, not one. Line POSITIONS are periodic; line STRENGTHS
        // are not -- they decay as the odd-harmonic envelope (0 / -14 / -31 dB). A one-period sweep
        // samples only the band where the strong first-order line is in grid reach, so
        // extrapolating from it would claim "every frequency" when the honest claim is "every
        // frequency in the first-order band". Beyond it the nearest reachable line is -14 dB and rho
        // should collapse -- the envelope's own prediction, making the far periods a second model
        // check alongside the BPSK250 negative control.
        let spacing = baud / 2.0;
        let mut worst: f32 = 0.0;
        let mut best_case: f32 = 1.0;
        let mut far_max: f32 = 0.0;
        let mut f = FC;
        let end = FC + spacing * 12.0;
        let mut steps = 0;
        while f <= end {
            let tone: Vec<f32> = (0..tlen + 200)
                .map(|k| (2.0 * PI * f * k as f32 / FS).cos())
                .collect();
            if let Some((r, _)) = mf.search_normalized_over_frequency(&tone, 200, 0.05, FS, &grid) {
                // Band by FIRST-ORDER LINE REACH, not by period. The strong line sits at baud/4
                // and the grid reaches grid_hz either side of a tone, so any tone within
                // baud/4 + grid_hz of the carrier can be rotated onto it. Splitting by period
                // instead put much of the "beyond" sample still inside that reach, which is a
                // property of my banding rather than of the signal.
                if (f - FC) <= baud / 4.0 + grid_hz {
                    worst = worst.max(r.rho);
                    best_case = best_case.min(r.rho);
                } else {
                    far_max = far_max.max(r.rho);
                }
                steps += 1;
            }
            f += spacing / 24.0;
        }
        println!(
            "{:<9} baud {:>7.2}  1st-line reach +/-{:>6.2} Hz  grid +/-{:.0}  WITHIN reach: \
             min {:.3} max {:.3} | OUTSIDE reach: max {:.3}  ({} pts)",
            mode,
            baud,
            baud / 4.0 + grid_hz,
            grid_hz,
            best_case,
            worst,
            far_max,
            steps
        );
    }
    println!("\n  The model predicts: where the grid exceeds HALF the line spacing, the MINIMUM");
    println!(
        "  should already be high — no tone frequency escapes. Where it does not, the minimum"
    );
    println!("  should be low, because tones between lines cannot be rotated onto one.");
}

/// P4: does the SETTLE still rescue a lone tone at the slow rungs? (It cannot.)
///
/// A component-level tone verdict is not the deployed verdict — this repo learned that the hard
/// way. For BPSK250 the chain rescues the component hole: the AFC settle lands on a lone tone, the
/// grid re-centres there, and the template's lines end up `baud/4` = 62.5 Hz away, outside the
/// ±20 Hz grid, so the veto refuses. Measured end-to-end at 5/5 decodes.
///
/// That rescue is a coincidence of magnitudes, and it does not scale. The parking distance IS
/// `baud/4`, so at BPSK31 it is 7.8 Hz — **inside** the same ±20 Hz grid. The mechanism that saves
/// the fast rung cannot save the slow one, by the same arithmetic that describes it.
///
/// Modelled here as a perfect settle: the grid is centred on the tone's own offset, which is where
/// `preamble_search_plan` puts it once the settle has locked. That isolates the geometry from
/// estimator noise, which is the quantity in question.
#[test]
#[ignore = "verification"]
fn p4_the_settle_rescue_does_not_scale_to_the_slow_rungs() {
    println!("\nP4: tone rho with the grid centred where a locked settle would put it");
    println!("    (deployed geometry, not a bench: grid follows the settle onto the tone)\n");
    println!(
        "{:<9} {:>7} {:>10} {:>9} {:>12} {:>26}",
        "mode", "baud", "park dist", "grid", "tone rho", "chain rescue?"
    );
    for (mode, baud) in [
        ("BPSK31", 31.25f32),
        ("BPSK63", 62.5),
        ("BPSK100", 100.0),
        ("BPSK250", 250.0),
    ] {
        let Ok(template) = bpsk_plugin::modulate::bpsk_preamble_template(&cfg(mode)) else {
            continue;
        };
        let grid_hz = 20.0f32;
        let tlen = template.len();
        let mf = IqMatchedFilter::new(template);
        let step = (0.25 * FS / tlen as f32).max(0.5);
        let n = (grid_hz / step).round() as i32;

        // Tone somewhere off carrier; a locked settle estimates its offset, so the grid centres there.
        let offset = 37.0f32;
        let grid: Vec<f32> = (-n..=n).map(|k| offset + k as f32 * step).collect();
        let tone: Vec<f32> = (0..tlen + 200)
            .map(|k| (2.0 * PI * (FC + offset) * k as f32 / FS).cos())
            .collect();
        let rho = mf
            .search_normalized_over_frequency(&tone, 200, 0.05, FS, &grid)
            .map(|(r, _)| r.rho)
            .unwrap_or(f32::NAN);
        let park = baud / 4.0;
        let rescued = if park > grid_hz {
            "yes - park outside grid"
        } else {
            "NO - park inside grid"
        };
        println!(
            "{:<9} {:>7.2} {:>10.2} {:>9.0} {:>12.3} {:>26}",
            mode, baud, park, grid_hz, rho, rescued
        );
    }
    println!("\n  The rescue needs the parking distance (baud/4) to exceed the grid half-width.");
    println!("  Where it does not, a locked settle hands the correlator a tone sitting within");
    println!("  grid reach of a line -- the protection inverts into the hazard.");
}

/// The engine's own cutoff derivation, so the bench measures what ships.
///
/// Mirrors `PreambleVeto::new`: cutoff from the SIGNAL (occupied bandwidth, grid, shaping margin),
/// then decimation chosen so the decimated Nyquist clears it. Deriving cutoff *from* the decimation
/// — which the first version did — has no baud term and fails in both directions.
fn engine_cutoff_and_decim(occupied_bw: f32, grid_hz: f32, tmpl_len: usize) -> (f32, usize) {
    let cutoff = occupied_bw / 2.0 + grid_hz + occupied_bw * 0.15;
    let by_budget = tmpl_len.div_ceil(2_048);
    let by_bandwidth = ((FS / 2.0) / (cutoff * 1.25)).floor().max(1.0) as usize;
    (cutoff, by_budget.min(by_bandwidth).max(1))
}

/// P5: equivalence at BPSK31 — the mode the whole slow-rung story is about.
///
/// P1/P2 measured BPSK250 only, which is the one mode that needs no decimation at all (992 samples,
/// under the budget). So the bench had never exercised the path it exists to justify: BPSK31's
/// template is 7936 samples and *must* decimate. It is also the mode whose numbers the old
/// cutoff formula got wrong in the generous direction, which a BPSK250-only bench could not see.
///
/// BPSK31 publishes no template — that is the grid finding — so this builds one from the modulator,
/// which is byte-for-byte what a derived-constants BPSK31 would publish.
#[test]
#[ignore = "verification"]
fn p5_equivalence_at_the_slow_rungs() {
    println!("\nP5: passband vs DDC at the modes that actually need decimation\n");
    println!(
        "{:<10} {:>9} {:>7} {:>9} {:>6} {:>10} {:>9} {:>11} {:>11} {:>9}",
        "mode",
        "template",
        "occ BW",
        "cutoff",
        "D",
        "post-DDC",
        "clean",
        "pb+noise",
        "DDC+noise",
        "delta"
    );

    for (mode, baud) in [
        ("BPSK31", 31.25f32),
        ("BPSK63", 62.5),
        ("BPSK100", 100.0),
        ("BPSK250", 250.0),
    ] {
        let Ok(template) = bpsk_plugin::modulate::bpsk_preamble_template(&cfg(mode)) else {
            continue;
        };
        let frame = bpsk_plugin::BpskPlugin::new()
            .modulate(b"slow rung equivalence", &cfg(mode))
            .expect("modulate");
        // Same over-estimate the plugin publishes: null-to-null main lobe = 2x baud.
        let occ = 2.0 * baud;
        let (cutoff, d) = engine_cutoff_and_decim(occ, 20.0, template.len());

        // A CLEAN frame is degenerate here: the signal contains the template exactly, so rho is
        // 1.0000 by construction on both paths and any correlator that can find a template in
        // itself "passes". The equivalence only means something once the correlation is stressed,
        // so the frame carries in-band noise scaled to the mode's own occupied band.
        let noisy: Vec<f32> = {
            let n = band_noise(
                frame.len(),
                FC - occ / 2.0,
                FC + occ / 2.0,
                0.12,
                mode.len() as u64 * 7 + 3,
            );
            frame.iter().zip(n).map(|(a, b)| a + b).collect()
        };
        let pb = rho_passband_best(&noisy, &template);
        let dd = rho_ddc_best(&noisy, &template, d, cutoff);
        let pb_clean = rho_passband_best(&frame, &template);
        println!(
            "{:<10} {:>9} {:>7.0} {:>9.1} {:>6} {:>10} {:>9.4} {:>11.4} {:>11.4} {:>9.4}",
            mode,
            template.len(),
            occ,
            cutoff,
            d,
            template.len() / d,
            pb_clean,
            pb,
            dd,
            dd - pb
        );
    }
    println!("\n  'clean' is the degenerate control (signal contains the template, so rho = 1 on");
    println!(
        "  both paths). The pb/DDC columns carry in-band noise, which is where equivalence is"
    );
    println!("  actually tested. post-DDC is the length the budget is measured against, <= 2048.");
    println!("  delta near zero is the equivalence claim, now measured where decimation is real");
    println!("  rather than only at BPSK250, where D = 1 and nothing is decimated at all.");
}
