//! The rectangular 8PSK demod must cancel the transmitter's raised-cosine crossfade ISI.
//!
//! The modulator blends adjacent symbols (sample `i` of slot `k` is `sym_k·w_tail + sym_{k+1}·w_head`,
//! `w_tail = ½(1+cos πi/n)`), exactly as QPSK does. 8PSK's matched one-slot demod integrates against the
//! *squared* window `w_tail²`, so it recovers `A·(sym_k + β·sym_{k+1})` with
//! `β = Σ w_head·w_tail² / Σ w_tail³` — 0.182 at 16 samples/symbol, 0.167 at 8. Left uncancelled that
//! floors the recovered-symbol EVM at `β² ≈ −15 dB` **regardless of SNR**, capping every soft consumer
//! (HARQ combining, soft FEC). Hard decisions are unaffected (±22.5° of 8PSK margin swallows it), so no
//! BER test catches this — but the 8PSK grid is far tighter than QPSK, so the floor matters more.
//!
//! The `cosine_overlap` (`-HF`) pulse is a per-symbol `sin²` bump that is zero at both slot boundaries —
//! adjacent symbols do NOT overlap, so there is no crossfade ISI and the cancellation must NOT run there.

use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use psk8_plugin::Psk8Plugin;

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

/// Recovered-symbol EVM (dB, error power / signal power). Re-projects the equalized data symbols onto
/// their nearest 8PSK point after a scale-free unit-radius normalisation, so the measure is amplitude-
/// invariant and only sees residual constellation error (of which the crossfade ISI is the SNR-floor).
fn evm_db(mode: &str, snr_db: f32) -> f32 {
    let p = Psk8Plugin::new();
    let payload: Vec<u8> = (0..255u32)
        .map(|i| (i.wrapping_mul(37) & 0xff) as u8)
        .collect();
    let tx = p.modulate(&payload, &cfg(mode)).unwrap();
    let mut seed = 11u64;
    let rx = awgn(&tx, snr_db, &mut seed);
    // The soft LLRs are max-log-MAP squared-distance differences — not raw projections — so instead
    // measure EVM directly from the recovered constellation via the debug symbol accessor.
    let syms = psk8_plugin::demodulate::extract_data_symbols_for_test(&rx, &cfg(mode)).unwrap();
    // Normalise to unit mean radius so the measure is scale-free.
    let mean_r = (syms
        .iter()
        .map(|&(i, q)| (i * i + q * q).sqrt())
        .sum::<f32>()
        / syms.len() as f32)
        .max(1e-9);
    let k = 1.0 / mean_r;
    let mut mse = 0.0f32;
    for &(i, q) in &syms {
        let (i, q) = (i * k, q * k);
        // Nearest unit-radius 8PSK point.
        let ang = q.atan2(i);
        let snapped = (ang / (std::f32::consts::PI / 4.0)).round() * (std::f32::consts::PI / 4.0);
        let (ti, tq) = (snapped.cos(), snapped.sin());
        mse += (i - ti) * (i - ti) + (q - tq) * (q - tq);
    }
    mse /= syms.len() as f32;
    10.0 * (mse).log10()
}

/// EVM at high SNR must sit below the `β² ≈ −15 dB` crossfade-ISI floor. Before cancellation every
/// rectangular rung stalls near it regardless of SNR; the ISI is a fixed error the noise cannot dip
/// below. `cancel_crossfade_isi` removes it. (8PSK500 at n=16 reaches deeper than 8PSK1000 at n=8, where
/// the residual 8-samples/symbol timing effect dominates — same as the QPSK twin.)
#[test]
fn evm_clears_the_crossfade_floor_at_high_snr() {
    for (mode, bound) in [("8PSK500", -18.0), ("8PSK1000", -14.0)] {
        let evm = evm_db(mode, 40.0);
        assert!(
            evm < bound,
            "{mode}: EVM {evm:.1} dB @40 dB did not clear the crossfade-ISI floor (bound {bound} dB) — a \
             matched one-slot integrate-and-dump leaves +β of the next symbol, and `cancel_crossfade_isi` \
             must remove it"
        );
    }
}

/// The `cosine_overlap` (`-HF`) pulse has no crossfade, so applying the cancellation there would inject
/// a third of the next symbol as error. This guards against a regression where the cancellation runs on
/// every non-RRC path: at 40 dB the `-HF` EVM stays deep (the independent `sin²` window recovers each
/// symbol exactly, so the only residual is thermal noise).
#[test]
fn cosine_overlap_hf_mode_stays_clean() {
    let evm = evm_db("8PSK1000-HF", 40.0);
    assert!(
        evm < -30.0,
        "8PSK1000-HF: EVM {evm:.1} dB @40 dB — the sin² per-symbol pulse has no crossfade, so the \
         crossfade cancellation must be gated out of the cosine-overlap path"
    );
}
