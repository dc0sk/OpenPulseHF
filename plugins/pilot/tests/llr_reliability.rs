//! The pilot-framed waveform's soft LLRs must mean what they say: a bit carrying `|L|` must be wrong
//! about `1/(1 + e^{|L|})` of the time.
//!
//! `symbols_to_llrs` measured `noise_var` with a decision-directed estimator (mean squared distance
//! to the *nearest* constellation point) over the recovered data symbols. On the dense 16QAM/32APSK
//! grids that saturates at moderate SNR — a symbol past a decision boundary is measured to the
//! wrong-but-near point — so σ² reads too low and the LLRs come out over-confident.
//!
//! The waveform already carries a fully-known BPSK preamble and sparse BPSK data-region pilots; their
//! residual measures the additive noise directly, without decisions. No single-frame decode metric
//! catches the miscalibration (soft Viterbi / LDPC / turbo are scale-invariant); it bites MAP HARQ
//! combining, where an over-confident deep-fade attempt out-votes a clean one.

use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use pilot_plugin::PilotPlugin;

fn cfg(mode: &str) -> ModulationConfig {
    ModulationConfig {
        center_frequency: 1500.0,
        sample_rate: 8000,
        mode: mode.to_string(),
        ..Default::default()
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
    let plugin = PilotPlugin::new();
    let payload: Vec<u8> = (0..255u32)
        .map(|i| (i.wrapping_mul(37) & 0xff) as u8)
        .collect();
    let tx = plugin.modulate(&payload, &cfg(mode)).expect("modulate");

    // Wide bins: a decision-directed σ² on the dense grid drives `|L|` into the hundreds, so the
    // meaningful over-confidence sits far above the usual [2,16) window — a calibrated demod keeps it
    // in the low tens. The reliability check holds across the whole range.
    const EDGES: [f32; 5] = [2.0, 8.0, 32.0, 128.0, 512.0];
    const NBINS: usize = EDGES.len() - 1;
    let mut errs = [0u32; NBINS];
    let mut tot = [0u32; NBINS];
    let mut lsum = [0.0f64; NBINS];

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
            for b in 0..NBINS {
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
    for b in 0..NBINS {
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
/// hypothesis), which no noise-variance term can undo. What the noise estimate must remove is the
/// order-of-magnitude over-confidence the decision-directed estimator produced on the dense grid.
#[test]
fn llrs_are_not_wildly_over_confident() {
    for (mode, snr) in [
        ("PILOT-16QAM500", 6.0f32),
        ("PILOT-16QAM500", 8.0),
        ("PILOT-32APSK500", 8.0),
        ("PILOT-32APSK500", 10.0),
    ] {
        let (ratio, mean_l) = worst_overconfidence(mode, snr);
        assert!(
            ratio <= 4.0,
            "{mode} @{snr} dB: bits with |L| ≈ {mean_l:.1} are wrong {ratio:.1}× more often than \
             1/(1+e^|L|) promises — the pilot-plugin soft-demod noise-var is under-reporting σ²"
        );
    }
}
