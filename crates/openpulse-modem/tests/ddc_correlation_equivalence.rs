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
