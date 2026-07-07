//! SC-FDMA integration tests: loopback, PAPR verification.

use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use scfdma_plugin::modulate::measure_papr;
use scfdma_plugin::ScFdmaPlugin;

fn cfg(mode: &str) -> ModulationConfig {
    ModulationConfig {
        mode: mode.into(),
        center_frequency: 1500.0,
        sample_rate: 8000,
        ..ModulationConfig::default()
    }
}

// ── Loopback correctness ───────────────────────────────────────────────────────

#[test]
fn scfdma16_clean_loopback() {
    let plugin = ScFdmaPlugin::new();
    let payload: Vec<u8> = (0u8..64).collect();
    let samples = plugin.modulate(&payload, &cfg("SCFDMA16")).unwrap();
    let rx = plugin.demodulate(&samples, &cfg("SCFDMA16")).unwrap();
    assert_eq!(rx, payload);
}

#[test]
fn scfdma52_clean_loopback() {
    let plugin = ScFdmaPlugin::new();
    let payload: Vec<u8> = (0u8..128).collect();
    let samples = plugin.modulate(&payload, &cfg("SCFDMA52")).unwrap();
    let rx = plugin.demodulate(&samples, &cfg("SCFDMA52")).unwrap();
    assert_eq!(rx, payload);
}

// ── PAPR verification ─────────────────────────────────────────────────────────

#[test]
fn scfdma52_papr_mean_below_12db() {
    // Localized SC-FDMA with 52 of 256 subcarriers achieves ~12 dB mean PAPR
    // without hard clipping.  The benefit over OFDM is that no iterative
    // clipping is applied, so there is no OOB spectral regrowth from
    // amplitude-limiting the time-domain signal.  PAPR is payload-dependent
    // (individual structured payloads reach ~13 dB), so the claim is asserted
    // as a mean over representative pseudo-random payloads, not per frame —
    // the previous single-payload form only passed by payload luck.
    let plugin = ScFdmaPlugin::new();
    let mut state = 1u32;
    let mut sum = 0.0f32;
    let mut max_papr = 0.0f32;
    const TRIALS: usize = 16;
    for _ in 0..TRIALS {
        let payload: Vec<u8> = (0..128)
            .map(|_| {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                (state >> 24) as u8
            })
            .collect();
        let samples = plugin.modulate(&payload, &cfg("SCFDMA52")).unwrap();
        let papr = measure_papr(&samples);
        sum += papr;
        max_papr = max_papr.max(papr);
    }
    let mean = sum / TRIALS as f32;
    assert!(
        mean < 12.0,
        "SC-FDMA mean PAPR {mean:.2} dB should be below 12 dB (no clipping applied)"
    );
    assert!(
        max_papr < 14.0,
        "SC-FDMA worst-case PAPR {max_papr:.2} dB sanity ceiling"
    );
}

#[test]
fn scfdma16_papr_below_12db() {
    let plugin = ScFdmaPlugin::new();
    let payload: Vec<u8> = (0u8..64).collect();
    let samples = plugin.modulate(&payload, &cfg("SCFDMA16")).unwrap();
    let papr = measure_papr(&samples);
    assert!(
        papr < 12.0,
        "SC-FDMA16 PAPR {papr:.2} dB should be below 12 dB"
    );
}

// ── Higher-order QAM loopback ─────────────────────────────────────────────────

#[test]
fn scfdma52_16qam_clean_loopback() {
    let plugin = ScFdmaPlugin::new();
    let payload: Vec<u8> = (0u8..128).collect();
    let samples = plugin.modulate(&payload, &cfg("SCFDMA52-16QAM")).unwrap();
    let rx = plugin.demodulate(&samples, &cfg("SCFDMA52-16QAM")).unwrap();
    assert_eq!(rx, payload);
}

#[test]
fn scfdma52_64qam_clean_loopback() {
    let plugin = ScFdmaPlugin::new();
    let payload: Vec<u8> = (0u8..128).collect();
    let samples = plugin.modulate(&payload, &cfg("SCFDMA52-64QAM")).unwrap();
    let rx = plugin.demodulate(&samples, &cfg("SCFDMA52-64QAM")).unwrap();
    assert_eq!(rx, payload);
}

#[test]
fn scfdma52_64qam_p4_clean_loopback() {
    let plugin = ScFdmaPlugin::new();
    let payload: Vec<u8> = (0u8..128).collect();
    let samples = plugin
        .modulate(&payload, &cfg("SCFDMA52-64QAM-P4"))
        .unwrap();
    let rx = plugin
        .demodulate(&samples, &cfg("SCFDMA52-64QAM-P4"))
        .unwrap();
    assert_eq!(rx, payload);
}

#[test]
fn scfdma52_16qam_awgn_snr25db() {
    let plugin = ScFdmaPlugin::new();
    let payload: Vec<u8> = (0u8..128).collect();
    let samples = plugin.modulate(&payload, &cfg("SCFDMA52-16QAM")).unwrap();
    let noisy = add_awgn(&samples, 25.0, 0xCAFE_BABE_u64);
    let rx = plugin.demodulate(&noisy, &cfg("SCFDMA52-16QAM")).unwrap();
    assert_eq!(
        rx, payload,
        "SCFDMA52-16QAM should decode correctly at 25 dB SNR"
    );
}

#[test]
fn scfdma52_16qam_hard_demod_no_amplitude_bias() {
    // Hard-demod QAM is sensitive to the MMSE attenuation: without the `alpha_avg` correction
    // (which the soft path applies and the hard path now mirrors) the outer 16QAM rings are pushed
    // toward the origin and mis-decode near threshold. This decodes cleanly at a tighter SNR.
    let plugin = ScFdmaPlugin::new();
    let payload: Vec<u8> = (0u8..128).collect();
    let samples = plugin.modulate(&payload, &cfg("SCFDMA52-16QAM")).unwrap();
    for seed in [0x1111_2222_u64, 0x3333_4444, 0x5555_6666] {
        let noisy = add_awgn(&samples, 20.0, seed);
        let rx = plugin.demodulate(&noisy, &cfg("SCFDMA52-16QAM")).unwrap();
        assert_eq!(
            rx, payload,
            "hard-demod SCFDMA52-16QAM should decode at 20 dB SNR (seed {seed:#x})"
        );
    }
}

#[test]
fn scfdma52_64qam_awgn_snr30db() {
    let plugin = ScFdmaPlugin::new();
    let payload: Vec<u8> = (0u8..128).collect();
    let samples = plugin.modulate(&payload, &cfg("SCFDMA52-64QAM")).unwrap();
    let noisy = add_awgn(&samples, 30.0, 0xDEAD_C0DE_u64);
    let rx = plugin.demodulate(&noisy, &cfg("SCFDMA52-64QAM")).unwrap();
    assert_eq!(
        rx, payload,
        "SCFDMA52-64QAM should decode correctly at 30 dB SNR"
    );
}

#[test]
fn scfdma52_64qam_p4_awgn_snr30db() {
    let plugin = ScFdmaPlugin::new();
    let payload: Vec<u8> = (0u8..128).collect();
    let samples = plugin
        .modulate(&payload, &cfg("SCFDMA52-64QAM-P4"))
        .unwrap();
    let noisy = add_awgn(&samples, 30.0, 0xABCD_1234_u64);
    let rx = plugin
        .demodulate(&noisy, &cfg("SCFDMA52-64QAM-P4"))
        .unwrap();
    assert_eq!(
        rx, payload,
        "SCFDMA52-64QAM-P4 should decode correctly at 30 dB SNR"
    );
}

fn add_awgn(samples: &[f32], snr_db: f32, seed: u64) -> Vec<f32> {
    let signal_power = samples.iter().map(|&s| s * s).sum::<f32>() / samples.len() as f32;
    let snr_linear = 10.0_f32.powf(snr_db / 10.0);
    let noise_std = (signal_power / snr_linear).sqrt();
    gaussian_noise_iter(seed, samples.len())
        .zip(samples.iter())
        .map(|(n, &s)| s + noise_std * n)
        .collect()
}

// ── AWGN robustness ───────────────────────────────────────────────────────────

#[test]
fn scfdma16_awgn_snr20db_zero_ber() {
    let plugin = ScFdmaPlugin::new();
    let payload: Vec<u8> = (0u8..32).collect();
    let samples = plugin.modulate(&payload, &cfg("SCFDMA16")).unwrap();
    let noisy = add_awgn(&samples, 20.0, 0xDEAD_BEEF_u64);
    let rx = plugin.demodulate(&noisy, &cfg("SCFDMA16")).unwrap();
    assert_eq!(rx, payload, "SCFDMA16 should decode correctly at 20 dB SNR");
}

/// Deterministic standard-normal samples via Box-Muller + 64-bit LCG.
fn gaussian_noise_iter(seed: u64, count: usize) -> impl Iterator<Item = f32> {
    let mut state = seed;
    let mut buf: Option<f32> = None;
    (0..count).map(move |_| {
        if let Some(v) = buf.take() {
            return v;
        }
        // LCG step (Knuth multiplier)
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let u1 = (state >> 11) as f32 / (1u64 << 53) as f32;
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let u2 = (state >> 11) as f32 / (1u64 << 53) as f32;
        let r = (-2.0 * u1.max(1e-12).ln()).sqrt();
        let theta = 2.0 * std::f32::consts::PI * u2;
        buf = Some(r * theta.sin());
        r * theta.cos()
    })
}
