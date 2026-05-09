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
fn scfdma52_papr_below_12db() {
    let plugin = ScFdmaPlugin::new();
    let payload: Vec<u8> = (0u8..128).collect();
    let samples = plugin.modulate(&payload, &cfg("SCFDMA52")).unwrap();
    let papr = measure_papr(&samples);
    // Localized SC-FDMA with 52 of 256 subcarriers achieves ~8-11 dB PAPR without
    // hard clipping.  The benefit over OFDM is that no iterative clipping is applied,
    // so there is no OOB spectral regrowth from amplitude-limiting the time-domain signal.
    assert!(
        papr < 12.0,
        "SC-FDMA PAPR {papr:.2} dB should be below 12 dB (no clipping applied)"
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

// ── AWGN robustness ───────────────────────────────────────────────────────────

#[test]
fn scfdma16_awgn_snr20db_zero_ber() {
    use std::f32::consts::PI;

    let plugin = ScFdmaPlugin::new();
    let payload: Vec<u8> = (0u8..32).collect();
    let samples = plugin.modulate(&payload, &cfg("SCFDMA16")).unwrap();

    // Add AWGN at 20 dB SNR.
    let signal_power = samples.iter().map(|&s| s * s).sum::<f32>() / samples.len() as f32;
    let snr_linear = 10.0_f32.powf(20.0 / 10.0);
    let noise_std = (signal_power / snr_linear).sqrt();

    // Deterministic pseudo-noise: sin-based for reproducibility.
    let noisy: Vec<f32> = samples
        .iter()
        .enumerate()
        .map(|(i, &s)| s + noise_std * (i as f32 * PI * 0.37).sin())
        .collect();

    let rx = plugin.demodulate(&noisy, &cfg("SCFDMA16")).unwrap();
    assert_eq!(rx, payload, "SCFDMA16 should decode correctly at 20 dB SNR");
}
