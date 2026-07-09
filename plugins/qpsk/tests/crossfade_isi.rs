//! The rectangular QPSK demod must cancel the transmitter's raised-cosine crossfade ISI.
//!
//! The modulator blends adjacent symbols (sample `i` of slot `k` is `sym_k·w_tail + sym_{k+1}·w_head`),
//! so a one-slot integrate-and-dump recovers `sym_k + ⅓·sym_{k+1}`. Left uncancelled, that floors the
//! recovered-symbol EVM at `(1/3)² = −9.5 dB` **regardless of SNR** — which caps every soft consumer.
//! Hard decisions are unaffected (45° of QPSK margin swallows it), so no BER test catches this.

use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use qpsk_plugin::QpskPlugin;

fn cfg(mode: &str) -> ModulationConfig {
    ModulationConfig {
        mode: mode.into(),
        center_frequency: 1500.0,
        sample_rate: 8000,
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

/// Recovered-symbol EVM (dB, error power / signal power) from the soft LLRs, which for QPSK are the raw
/// I/Q projections. Re-normalised to unit component magnitude so the measure is scale-free.
fn evm_db(mode: &str, snr_db: f32) -> f32 {
    let p = QpskPlugin::new();
    let payload: Vec<u8> = (0..255u32)
        .map(|i| (i.wrapping_mul(37) & 0xff) as u8)
        .collect();
    let tx = p.modulate(&payload, &cfg(mode)).unwrap();
    let mut seed = 11u64;
    let rx = awgn(&tx, snr_db, &mut seed);
    let l = p.demodulate_soft(&rx, &cfg(mode)).unwrap();
    let inv = std::f32::consts::FRAC_1_SQRT_2;
    let mean_abs = l.iter().map(|v| v.abs()).sum::<f32>() / l.len() as f32;
    let k = inv / mean_abs.max(1e-9);
    let mse: f32 = l
        .iter()
        .map(|&v| {
            let x = v * k;
            let d = x - if x >= 0.0 { inv } else { -inv };
            d * d
        })
        .sum::<f32>()
        / l.len() as f32;
    10.0 * (mse / 0.5).log10()
}

/// EVM at high SNR must sit well below the `β² = −9.5 dB` crossfade-ISI floor. Before cancellation every
/// rectangular rung stalled at ≈ −9.7 dB from 16 dB upward; the ISI is a fixed error the noise cannot dip
/// below. A single `−13 dB` bound is enough to prove the floor is gone (cancellation reaches −26/−20/−15
/// dB for QPSK250/500/1000 at 40 dB — the residue at the faster rungs is a separate 8-sps timing effect).
#[test]
fn evm_clears_the_crossfade_floor_at_high_snr() {
    for mode in ["QPSK250", "QPSK500", "QPSK1000"] {
        let evm = evm_db(mode, 40.0);
        assert!(
            evm < -13.0,
            "{mode}: EVM {evm:.1} dB @40 dB is not below the −9.5 dB crossfade-ISI floor — a matched \
             one-slot integrate-and-dump leaves +1/3 of the next symbol, and `cancel_crossfade_isi` \
             must remove it"
        );
    }
}
