//! 64QAM's soft LLRs must mean what they say: a bit carrying `|L|` must be wrong about
//! `1/(1 + e^{|L|})` of the time.
//!
//! The soft demodulator measured `noise_var` with a *decision-directed* estimator (mean squared
//! distance to the nearest constellation point). On the dense 64QAM grid that saturates badly at
//! moderate SNR — once a symbol lands past a decision boundary its distance is measured to the
//! wrong-but-near point, so the estimate reads σ² far too low and the LLRs come out over-confident.
//! Measured at 6–14 dB the under-read was 2–4.8×, so bits with a given `|L|` were wrong several
//! times more often than they promised.
//!
//! No single-frame decode metric catches this: soft Viterbi, min-sum LDPC and max-log turbo are all
//! scale-invariant, and the bias is nearly a per-frame constant. It shows up when LLRs are consumed
//! as probabilities — HARQ soft combining across receive attempts, where a deep-fade attempt with
//! over-confident LLRs out-votes a clean one.

use openpulse_core::plugin::{ModulationConfig, ModulationPlugin, PulseShape};
use qam64_plugin::Qam64Plugin;

fn cfg(mode: &str) -> ModulationConfig {
    ModulationConfig {
        mode: mode.to_string(),
        sample_rate: 8000,
        center_frequency: 1500.0,
        pulse_shape: if mode.ends_with("-RRC") {
            PulseShape::Rrc { alpha: 0.35 }
        } else {
            PulseShape::Hann
        },
        ..ModulationConfig::default()
    }
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
fn worst_overconfidence(mode: &str, snr_db: f32) -> (f32, f32) {
    let plugin = Qam64Plugin::new();
    let payload: Vec<u8> = (0..255u32)
        .map(|i| (i.wrapping_mul(37) & 0xff) as u8)
        .collect();
    let tx = plugin.modulate(&payload, &cfg(mode)).expect("modulate");

    const EDGES: [f32; 4] = [2.0, 4.0, 8.0, 16.0];
    let mut errs = [0u32; 3];
    let mut tot = [0u32; 3];
    let mut lsum = [0.0f64; 3];

    for t in 0..16u64 {
        let mut seed = 11 + t * 977;
        let rx = awgn(&tx, snr_db, &mut seed);
        let Ok(llrs) = plugin.demodulate_soft(&rx, &cfg(mode)) else {
            continue;
        };
        let n = llrs.len().min(payload.len() * 8);
        for i in 0..n {
            let bit = (payload[i / 8] >> (i % 8)) & 1 == 1;
            let l = llrs[i];
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
/// noise estimate must remove is the order-of-magnitude over-confidence the decision-directed
/// estimator produced on the dense grid.
#[test]
fn llrs_are_not_wildly_over_confident() {
    for (mode, snr) in [
        ("64QAM500", 10.0f32),
        ("64QAM500", 12.0),
        ("64QAM500", 14.0),
        ("64QAM2000-RRC", 12.0),
        ("64QAM2000-RRC", 14.0),
    ] {
        let (ratio, mean_l) = worst_overconfidence(mode, snr);
        assert!(
            ratio <= 4.0,
            "{mode} @{snr} dB: bits with |L| ≈ {mean_l:.1} are wrong {ratio:.1}× more often than \
             1/(1+e^|L|) promises — the soft-demod noise-var estimate is under-reporting σ²"
        );
    }
}
