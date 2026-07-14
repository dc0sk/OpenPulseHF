//! MFSK16's soft LLRs must mean what they say: a bit carrying `|L|` must be wrong about `1/(1+e^{|L|})`
//! of the time. The measurement used a *per-symbol* mean of the 15 non-winner tones for the noise scale
//! (~26% relative σ² error, and inflated by tone leakage under Doppler); the production plugin uses a
//! **frame-level median** of all non-winner tone energies (exponential-median corrected) — the calibration
//! this gate verifies. Bound is 4× (the max-log-MAP approximation is itself optimistic; no noise term
//! undoes that). Checked on AWGN *and* one Watterson moderate_f1 point, since this rung lives on fading.

use mfsk16_plugin::Mfsk16Plugin;
use openpulse_channel::{watterson::WattersonChannel, ChannelModel, WattersonConfig};
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};

const FRAME_BYTES: usize = 255;

fn cfg() -> ModulationConfig {
    ModulationConfig {
        mode: "MFSK16".into(),
        sample_rate: 8000,
        center_frequency: 1500.0,
        ..ModulationConfig::default()
    }
}

fn payload() -> Vec<u8> {
    (0..FRAME_BYTES as u32)
        .map(|i| (i.wrapping_mul(37) & 0xff) as u8)
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

/// Worst `empirical / predicted` bit-error ratio over the `|L|` bins with enough samples, applying
/// `channel` to each of `trials` transmissions.
fn worst_overconfidence(trials: u64, channel: impl Fn(&[f32], u64) -> Vec<f32>) -> (f32, f32) {
    let plugin = Mfsk16Plugin::new();
    let pl = payload();
    let tx = plugin.modulate(&pl, &cfg()).expect("modulate");

    const EDGES: [f32; 4] = [2.0, 4.0, 8.0, 16.0];
    let mut errs = [0u32; 3];
    let mut tot = [0u32; 3];
    let mut lsum = [0.0f64; 3];

    for t in 0..trials {
        let rx = channel(&tx, t);
        let Ok(llrs) = plugin.demodulate_soft(&rx, &cfg()) else {
            continue; // acquisition failed at this SNR/seed — no LLRs to score
        };
        let n = llrs.len().min(pl.len() * 8);
        for i in 0..n {
            let bit = (pl[i / 8] >> (i % 8)) & 1 == 1;
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
        if tot[b] < 300 {
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

#[test]
fn llrs_are_calibrated_on_awgn() {
    for snr in [-3.0f32, 0.0, 3.0] {
        let (ratio, mean_l) = worst_overconfidence(24, |tx, t| {
            let mut seed = 11 + t * 977;
            awgn(tx, snr, &mut seed)
        });
        assert!(
            ratio <= 4.0,
            "AWGN @{snr} dB: bits with |L| ≈ {mean_l:.1} are wrong {ratio:.1}× more often than \
             1/(1+e^|L|) promises — the frame-median noise estimate is under-reporting σ²"
        );
    }
}

#[test]
fn llrs_are_calibrated_on_moderate_watterson() {
    // The rung's home turf. At ~0 dB (near the crossing) there are plenty of graded bits to score.
    let (ratio, mean_l) = worst_overconfidence(24, |tx, t| {
        let mut c = WattersonConfig::moderate_f1(Some(400 + t));
        c.snr_db = 0.0;
        WattersonChannel::new(c).expect("watterson").apply(tx)
    });
    assert!(
        ratio <= 4.0,
        "moderate_f1 @0 dB: bits with |L| ≈ {mean_l:.1} wrong {ratio:.1}× more often than promised"
    );
}
