//! GPU-vs-CPU equivalence on inputs the receiver actually sees.
//!
//! The in-crate equivalence tests (`gpu_demodulate_matches_cpu`, the two kernel
//! ones) compare a **clean, noiseless, perfectly-aligned** frame: modulate two
//! bytes, demodulate them back. They run, they assert, and they pass — while the
//! GPU path costs `openpulse-linksim`'s rate-ladder gate its climb (#1080:
//! passes in 14 s with `OPENPULSE_GPU_DISABLE=1`, fails in 54 s without).
//!
//! A pristine waveform is the easiest case that exists. These tests add the two
//! things a real receiver always has — noise and a carrier offset — and compare
//! decode outcomes over a seed sweep, so a divergence that only appears off the
//! ideal operating point has somewhere to show up.
//!
//! Requires `--features gpu` and a working adapter; skips loudly otherwise, and
//! the skip is asserted against rather than silently reported as success.

#![cfg(feature = "gpu")]

use bpsk_plugin::demodulate::{bpsk_demodulate, bpsk_demodulate_with_gpu};
use bpsk_plugin::modulate::bpsk_modulate;
use openpulse_core::plugin::{ModulationConfig, PulseShape};

fn config(mode: &str, fc: f32) -> ModulationConfig {
    ModulationConfig {
        mode: mode.to_string(),
        sample_rate: 8000,
        center_frequency: fc,
        pulse_shape: PulseShape::Hann,
        ..Default::default()
    }
}

/// Deterministic Gaussian noise at a given SNR. Box-Muller over an LCG — a
/// uniform difference generator in an earlier probe carried a DC bias.
fn add_awgn(signal: &[f32], snr_db: f32, seed: u64) -> Vec<f32> {
    let p: f32 = signal.iter().map(|s| s * s).sum::<f32>() / signal.len().max(1) as f32;
    let sigma = (p / 10f32.powf(snr_db / 10.0)).sqrt();
    let mut st = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1);
    let mut u = || -> f32 {
        st = st
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((st >> 11) as f32 / (1u64 << 53) as f32).clamp(1e-9, 1.0 - 1e-9)
    };
    signal
        .iter()
        .map(|&s| {
            let (a, b) = (u(), u());
            s + sigma * (-2.0 * a.ln()).sqrt() * (std::f32::consts::TAU * b).cos()
        })
        .collect()
}

fn ctx() -> Option<std::sync::Arc<openpulse_gpu::GpuContext>> {
    openpulse_gpu::GpuContext::init()
}

#[test]
fn the_adapter_is_available_or_this_file_proves_nothing() {
    // The in-crate equivalence tests return early with an eprintln when no adapter
    // is present, which libtest still reports as `ok`. A skipped equivalence test
    // that reads as a pass is the vacuous-gate shape; this one says so out loud.
    assert!(
        ctx().is_some(),
        "no GPU adapter — every test in this file is inert. Run with an adapter, or \
         accept that GPU/CPU equivalence is UNMEASURED rather than confirmed."
    );
}

#[test]
fn gpu_and_cpu_agree_on_a_clean_frame() {
    // The existing coverage, restated here as the control. If this fails the
    // divergence is not subtle and the sweep below is not needed to find it.
    let Some(c) = ctx() else { return };
    let cfg = config("BPSK250", 1500.0);
    let tx = bpsk_modulate(b"clean frame control", &cfg).expect("modulate");
    let cpu = bpsk_demodulate(&tx, &cfg).expect("cpu");
    let gpu = bpsk_demodulate_with_gpu(&tx, &cfg, &c).expect("gpu");
    assert_eq!(cpu, gpu, "GPU and CPU disagree on a NOISELESS frame");
}

#[test]
fn gpu_and_cpu_agree_under_noise() {
    let Some(c) = ctx() else { return };
    let cfg = config("BPSK250", 1500.0);
    let payload = b"gpu equivalence under noise";
    let tx = bpsk_modulate(payload, &cfg).expect("modulate");

    let mut compared = 0u32;
    let mut disagreed = 0u32;
    let mut cpu_only = 0u32;
    let mut gpu_only = 0u32;

    for snr_db in [20.0f32, 12.0, 8.0, 4.0] {
        for seed in 0..12u64 {
            let rx = add_awgn(&tx, snr_db, seed * 31 + snr_db as u64);
            let cpu = bpsk_demodulate(&rx, &cfg).ok();
            let gpu = bpsk_demodulate_with_gpu(&rx, &cfg, &c).ok();
            compared += 1;
            let cpu_ok = cpu.as_ref().is_some_and(|b| b.starts_with(payload));
            let gpu_ok = gpu.as_ref().is_some_and(|b| b.starts_with(payload));
            if cpu != gpu {
                disagreed += 1;
            }
            if cpu_ok && !gpu_ok {
                cpu_only += 1;
            }
            if gpu_ok && !cpu_ok {
                gpu_only += 1;
            }
        }
    }

    println!(
        "compared {compared}: byte-level disagreements {disagreed}, \
         CPU-decoded-only {cpu_only}, GPU-decoded-only {gpu_only}"
    );

    // The load-bearing assertion is the DECODE OUTCOME, not byte equality: two
    // implementations may differ in the tail of a failed frame without either
    // being wrong. A frame that decodes on one path and not the other is a
    // genuine divergence, and it is what costs the rate ladder its evidence.
    assert_eq!(
        (cpu_only, gpu_only),
        (0, 0),
        "GPU and CPU disagree on whether a noisy frame decodes \
         ({cpu_only} CPU-only, {gpu_only} GPU-only of {compared}) — the clean-frame \
         equivalence tests cannot see this"
    );
}

#[test]
fn gpu_and_cpu_agree_under_a_carrier_offset() {
    let Some(c) = ctx() else { return };
    let payload = b"gpu equivalence under offset";

    let mut compared = 0u32;
    let mut cpu_only = 0u32;
    let mut gpu_only = 0u32;

    for offset_hz in [-40.0f32, -12.0, 0.0, 12.0, 40.0] {
        // Transmit off-frequency, receive at nominal — the acquisition case.
        let tx_cfg = config("BPSK250", 1500.0 + offset_hz);
        let rx_cfg = config("BPSK250", 1500.0);
        let tx = bpsk_modulate(payload, &tx_cfg).expect("modulate");
        for seed in 0..6u64 {
            let rx = add_awgn(&tx, 16.0, seed + 900);
            let cpu = bpsk_demodulate(&rx, &rx_cfg).ok();
            let gpu = bpsk_demodulate_with_gpu(&rx, &rx_cfg, &c).ok();
            compared += 1;
            let cpu_ok = cpu.as_ref().is_some_and(|b| b.starts_with(payload));
            let gpu_ok = gpu.as_ref().is_some_and(|b| b.starts_with(payload));
            if cpu_ok && !gpu_ok {
                cpu_only += 1;
            }
            if gpu_ok && !cpu_ok {
                gpu_only += 1;
            }
        }
    }

    println!("compared {compared}: CPU-decoded-only {cpu_only}, GPU-decoded-only {gpu_only}");

    // The SAFETY property, and the one that gates: enabling the GPU must never LOSE
    // a frame the CPU would have decoded. That is the direction that cost the rate
    // ladder its climb evidence in #1080 (measured 12 of 30 before the shader fix,
    // 0 after).
    assert_eq!(
        cpu_only, 0,
        "the GPU path failed to decode {cpu_only} of {compared} frames the CPU decoded — \
         enabling GPU acceleration must not lose frames"
    );

    // The residual is deliberately REPORTED, not asserted away. After the fix the
    // GPU decodes some frames the CPU does not (measured 6 of 30). That is not a
    // regression and may mean the GPU search is the better one — but it is still a
    // divergence between two receivers that are supposed to agree, and which one is
    // right is unresolved. Asserting equality here would either fail on an
    // improvement or tempt someone to weaken the safety check above.
    if gpu_only > 0 {
        println!(
            "NOTE: GPU decoded {gpu_only} of {compared} frames the CPU did not. \
             Not a regression; the two searches still disagree off-frequency. See #1080."
        );
    }
}
