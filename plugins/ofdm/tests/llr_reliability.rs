//! OFDM's soft LLRs must mean what they say: a bit carrying `|L|` must be wrong about
//! `1/(1 + e^{|L|})` of the time.
//!
//! Two defects made them over-confident:
//!
//! 1. **Decision-directed σ² under-read.** The per-block noise was `estimate_decision_noise_var`
//!    (mean squared distance to the *nearest* constellation point). On a dense QAM grid that
//!    saturates at moderate SNR — a symbol past a decision boundary is measured to the wrong-but-near
//!    point — so σ² reads too low and the LLRs come out over-confident.
//! 2. **ZF noise-enhancement double-count.** That block noise is measured *after* ZF equalization, so
//!    it already carries the per-SC `1/|H_k|²` blow-up in aggregate; the code then rescaled each SC by
//!    `mean|H|²/|H_k|²` a second time.
//!
//! No single-frame decode metric catches either: soft Viterbi, min-sum LDPC and max-log turbo are all
//! scale-invariant. It bites when LLRs are consumed as probabilities — MAP HARQ combining, where an
//! over-confident deep-fade attempt out-votes a clean one.

use ofdm_plugin::OfdmPlugin;
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};

fn cfg(mode: &str) -> ModulationConfig {
    ModulationConfig {
        mode: mode.into(),
        center_frequency: 1500.0,
        sample_rate: 8000,
        ..ModulationConfig::default()
    }
}

/// Static two rays inside the cyclic prefix (`CP = 32`): `y[n] = x[n] + a·x[n−d]`.
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
    let plugin = OfdmPlugin::new();
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
        let rx = awgn(&two_ray(&tx, a, d), snr_db, &mut seed);
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

/// The bound is 4×, not 1×: the max-log-MAP approximation is itself optimistic (nearest point per
/// hypothesis), which no noise-variance term can undo. What the noise model must remove is the
/// order-of-magnitude over-confidence the decision-directed estimate + double-counted ZF enhancement
/// produced. Over-confidence is the dangerous direction (it corrupts HARQ); under-confidence is safe.
#[test]
fn llrs_are_not_wildly_over_confident() {
    for (label, mode, a, d, snr) in [
        ("flat", "OFDM52-16QAM", 0.0f32, 0usize, 10.0f32),
        ("flat", "OFDM52-16QAM", 0.0, 0, 12.0),
        ("flat", "OFDM52-64QAM", 0.0, 0, 16.0),
        ("two-ray a=0.7 d=8", "OFDM52-16QAM", 0.7, 8, 12.0),
        ("two-ray a=0.7 d=8", "OFDM52-16QAM", 0.7, 8, 14.0),
    ] {
        let (ratio, mean_l) = worst_overconfidence(mode, a, d, snr);
        assert!(
            ratio <= 4.0,
            "{mode} {label} @{snr} dB: bits with |L| ≈ {mean_l:.1} are wrong {ratio:.1}× more often \
             than 1/(1+e^|L|) promises — the OFDM soft-demod noise model is over-reporting confidence"
        );
    }
}
