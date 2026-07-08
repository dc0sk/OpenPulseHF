//! SC-FDMA's soft LLRs must mean what they say: a bit carrying `|L|` must be wrong about
//! `1/(1 + e^{|L|})` of the time.
//!
//! `mmse_llr_noise_var` used to model only the additive noise through the equalizer, treating the
//! channel estimate as exact and the DFT de-spread as ISI-free. Both are false, and the LLRs were
//! wildly over-confident where it matters — measured at 12 dB on a *flat* channel, bits with `|L| ≈ 12`
//! were wrong **71×** more often than they promised.
//!
//! No single-frame decode metric catches this: soft Viterbi, min-sum LDPC and max-log turbo are all
//! scale-invariant, and the missing terms are close to a per-frame constant. It shows up when LLRs are
//! consumed as probabilities — HARQ soft combining, and any iterative equalizer whose feedback
//! reliability is derived from them (research item P7).

use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use scfdma_plugin::demodulate::scfdma_demodulate_soft_with_metrics;
use scfdma_plugin::ScFdmaPlugin;

fn cfg(mode: &str) -> ModulationConfig {
    ModulationConfig {
        mode: mode.into(),
        center_frequency: 1500.0,
        sample_rate: 8000,
        ..ModulationConfig::default()
    }
}

/// Static two rays inside the cyclic prefix: `y[n] = x[n] + a·x[n−d]`.
fn two_ray(x: &[f32], a: f32, d: usize) -> Vec<f32> {
    (0..x.len())
        .map(|n| x[n] + if n >= d { a * x[n - d] } else { 0.0 })
        .collect()
}

fn awgn(x: &[f32], snr_db: f32, seed: &mut u64) -> Vec<f32> {
    let sp = x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32;
    let sd = (sp / 10f32.powf(snr_db / 10.0)).sqrt();
    x.iter()
        .map(|&s| {
            *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let u1 = (((*seed >> 40) as f32) / ((1u64 << 24) as f32)).max(1e-6);
            *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let u2 = ((*seed >> 40) as f32) / ((1u64 << 24) as f32);
            s + sd * (-2.0 * u1.ln()).sqrt() * (std::f32::consts::TAU * u2).cos()
        })
        .collect()
}

/// Worst `empirical / predicted` bit-error ratio over the `|L|` bins that carry enough samples.
fn worst_overconfidence(mode: &str, a: f32, d: usize, snr_db: f32) -> (f32, f32) {
    let plugin = ScFdmaPlugin::new();
    let payload: Vec<u8> = (0..255u32)
        .map(|i| (i.wrapping_mul(37) & 0xff) as u8)
        .collect();
    let tx = plugin.modulate(&payload, &cfg(mode)).expect("modulate");

    const EDGES: [f32; 4] = [2.0, 4.0, 8.0, 16.0];
    let mut errs = [0u32; 3];
    let mut tot = [0u32; 3];
    let mut lsum = [0.0f64; 3];

    for t in 0..12u64 {
        let mut seed = 7 + t * 977;
        let rx = awgn(&two_ray(&tx, a, d), snr_db, &mut seed);
        let Ok(out) = scfdma_demodulate_soft_with_metrics(&rx, mode) else {
            continue;
        };
        let n = out.llrs.len().min(payload.len() * 8);
        for i in 0..n {
            let bit = (payload[i / 8] >> (i % 8)) & 1 == 1;
            let l = out.llrs[i];
            let m = l.abs();
            for b in 0..3 {
                if m >= EDGES[b] && m < EDGES[b + 1] {
                    tot[b] += 1;
                    lsum[b] += m as f64;
                    if (l < 0.0) != bit {
                        errs[b] += 1;
                    }
                }
            }
        }
    }

    let (mut worst, mut worst_l) = (0.0f32, 0.0f32);
    for b in 0..3 {
        // Need enough samples that the empirical rate means something.
        if tot[b] < 500 {
            continue;
        }
        let empirical = errs[b] as f64 / tot[b] as f64;
        let mean_l = lsum[b] / tot[b] as f64;
        let predicted = 1.0 / (1.0 + mean_l.exp());
        let ratio = ((empirical + 1e-9) / (predicted + 1e-9)) as f32;
        if ratio > worst {
            worst = ratio;
            worst_l = mean_l as f32;
        }
    }
    (worst, worst_l)
}

/// The bound is 4×, not 1×: the max-log-MAP approximation is itself optimistic (it keeps only the
/// nearest constellation point per hypothesis), which no noise-variance term can undo. What the
/// variance terms must remove is the order-of-magnitude error — measured at 9× and 71× before.
#[test]
fn llrs_are_not_wildly_over_confident() {
    for (label, mode, a, d, snr) in [
        ("flat", "SCFDMA52-16QAM", 0.0f32, 0usize, 10.0f32),
        ("flat", "SCFDMA52-16QAM", 0.0, 0, 12.0),
        ("two-ray a=0.9 d=4", "SCFDMA52-16QAM", 0.9, 4, 10.0),
        ("two-ray a=0.9 d=4", "SCFDMA52-16QAM", 0.9, 4, 12.0),
        ("two-ray a=0.5 d=4", "SCFDMA52-8PSK", 0.5, 4, 8.0),
    ] {
        let (ratio, mean_l) = worst_overconfidence(mode, a, d, snr);
        assert!(
            ratio <= 4.0,
            "{mode} {label} @{snr} dB: bits with |L| ≈ {mean_l:.1} are wrong {ratio:.1}× more often \
             than 1/(1+e^|L|) promises — `mmse_llr_noise_var` is under-reporting the post-despread \
             error variance (channel-estimate error, residual ISI)"
        );
    }
}
