//! GPU-vs-CPU equivalence for 8PSK, on inputs the receiver actually sees.
//!
//! Companion to the BPSK sweep that found #1080, where `timing_search.wgsl` had
//! never received the phase-invariance fix its CPU sibling got. 8PSK does not use
//! that kernel — it uses `gpu_rrc_fir` and `gpu_soft_demod` — so those are
//! separately unverified against realistic input.
//!
//! **The soft path is the reason this file exists.** This repo's LLR history is
//! explicit that a wrong LLR *magnitude* is invisible to every frame-success
//! metric in the tree: soft Viterbi, min-sum LDPC and max-log turbo are all
//! scale-invariant, and SC-FDMA once emitted LLRs whose bits were wrong 71x more
//! often than their magnitude promised while every decode test stayed green. A
//! GPU soft demodulator that diverges in magnitude but not in sign would pass a
//! hard-decision comparison and corrupt HARQ combining and any iterative
//! equalizer downstream.
//!
//! So this compares LLRs numerically, not just their signs.

#![cfg(feature = "gpu")]

use openpulse_core::plugin::{ModulationConfig, PulseShape};
use psk8_plugin::demodulate::{
    psk8_demodulate, psk8_demodulate_gpu, psk8_demodulate_soft, psk8_demodulate_soft_gpu,
};
use psk8_plugin::modulate::psk8_modulate;

// MUST be an RRC mode. `psk8_demodulate_gpu` returns `None` for any other pulse
// shape ("only accelerate RRC modes; non-RRC path has no FIR to offload"), so a
// Hann mode would silently compare the CPU against itself — the GPU path never
// running while the test reports equivalence. `the_gpu_path_actually_engages`
// pins that.
const MODE: &str = "8PSK1000-RRC";

fn config(fc: f32) -> ModulationConfig {
    ModulationConfig {
        mode: MODE.to_string(),
        sample_rate: 8000,
        center_frequency: fc,
        pulse_shape: PulseShape::Rrc { alpha: 0.35 },
        ..Default::default()
    }
}

/// Deterministic Gaussian noise. Box-Muller over an LCG — a uniform difference
/// generator in an earlier probe in this series carried a DC bias.
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
    assert!(
        ctx().is_some(),
        "no GPU adapter — every test in this file is inert. 8PSK GPU/CPU equivalence is \
         UNMEASURED rather than confirmed."
    );
}

#[test]
fn the_gpu_path_actually_engages() {
    // Anti-vacuity. `psk8_demodulate_gpu` returns `None` for non-RRC modes, and a
    // `None` compared against the CPU reads as perfect agreement while proving
    // nothing. If this fails, every comparison below is CPU-vs-CPU.
    let Some(c) = ctx() else { return };
    let cfg = config(1500.0);
    let tx = psk8_modulate(b"engage probe", &cfg).expect("modulate");
    assert!(
        psk8_demodulate_gpu(&tx, &cfg, &c).is_some(),
        "the GPU demod declined mode {MODE} — the comparisons in this file would be CPU vs CPU"
    );
}

#[test]
fn hard_decisions_agree_under_noise() {
    let Some(c) = ctx() else { return };
    let cfg = config(1500.0);
    let payload = b"8psk gpu equivalence";
    let tx = psk8_modulate(payload, &cfg).expect("modulate");

    let mut compared = 0u32;
    let (mut cpu_only, mut gpu_only) = (0u32, 0u32);
    for snr_db in [24.0f32, 18.0, 14.0, 10.0] {
        for seed in 0..8u64 {
            let rx = add_awgn(&tx, snr_db, seed * 17 + snr_db as u64);
            let cpu_ok = psk8_demodulate(&rx, &cfg)
                .map(|b| b.starts_with(payload))
                .unwrap_or(false);
            let gpu_ok = psk8_demodulate_gpu(&rx, &cfg, &c)
                .and_then(|r| r.ok())
                .map(|b| b.starts_with(payload))
                .unwrap_or(false);
            compared += 1;
            if cpu_ok && !gpu_ok {
                cpu_only += 1;
            }
            if gpu_ok && !cpu_ok {
                gpu_only += 1;
            }
        }
    }
    println!("hard: compared {compared}, CPU-only {cpu_only}, GPU-only {gpu_only}");
    assert_eq!(
        cpu_only, 0,
        "the 8PSK GPU path lost {cpu_only} of {compared} frames the CPU decoded"
    );
}

/// LLR **magnitudes** must agree, not just their signs.
///
/// A sign-only comparison is the trap: every FEC decoder in this tree is
/// scale-invariant, so a GPU soft demodulator could be uniformly wrong in
/// magnitude and no decode test would notice — while HARQ combining, which sums
/// LLRs across attempts, would be silently mis-weighted.
#[test]
fn soft_llrs_agree_numerically_under_noise() {
    let Some(c) = ctx() else { return };
    let cfg = config(1500.0);
    let tx = psk8_modulate(b"8psk soft llr equivalence", &cfg).expect("modulate");

    let mut worst_abs = 0.0f32;
    let mut worst_rel = 0.0f32;
    let mut sign_flips = 0u32;
    let mut total = 0u32;
    // 8PSK carries 3 bits per symbol. If the divergence is confined to one bit
    // POSITION, the fault is in the bit mapping rather than in the LLR maths.
    let mut per_bit_flip = [0u32; 3];
    let mut per_bit_total = [0u32; 3];

    for snr_db in [24.0f32, 16.0, 10.0] {
        for seed in 0..6u64 {
            let rx = add_awgn(&tx, snr_db, seed * 23 + 5);
            let cpu = psk8_demodulate_soft(&rx, &cfg).expect("cpu soft");
            let gpu = psk8_demodulate_soft_gpu(&rx, &cfg, &c).expect("gpu soft");
            assert_eq!(
                cpu.len(),
                gpu.len(),
                "GPU soft demod returned {} LLRs, CPU returned {}",
                gpu.len(),
                cpu.len()
            );
            for (idx, (a, b)) in cpu.iter().zip(gpu.iter()).enumerate() {
                total += 1;
                per_bit_total[idx % 3] += 1;
                if a.signum() != b.signum() && a.abs() > 0.05 && b.abs() > 0.05 {
                    per_bit_flip[idx % 3] += 1;
                }
                let d = (a - b).abs();
                worst_abs = worst_abs.max(d);
                let scale = a.abs().max(b.abs()).max(1e-3);
                worst_rel = worst_rel.max(d / scale);
                if a.signum() != b.signum() && a.abs() > 0.05 && b.abs() > 0.05 {
                    sign_flips += 1;
                }
            }
        }
    }

    println!(
        "soft: {total} LLRs compared, worst |delta| {worst_abs:.4}, worst relative {worst_rel:.4}, \
         confident sign flips {sign_flips}"
    );
    for b in 0..3 {
        println!(
            "  bit {b} of 3: {} flips of {} ({:.1}%)",
            per_bit_flip[b],
            per_bit_total[b],
            100.0 * per_bit_flip[b] as f32 / per_bit_total[b].max(1) as f32
        );
    }

    // Hypothesis check, stated before the numbers: bit 1 agreeing perfectly while
    // bits 0 and 2 each disagree ~50% is the signature of the three bits being
    // emitted in REVERSED order within each symbol — reversal swaps 0 and 2 and
    // fixes 1. If re-pairing under that permutation makes the streams agree, the
    // fault is a bit-order convention, not the LLR maths.
    {
        let cfg2 = config(1500.0);
        let tx2 = psk8_modulate(b"8psk soft llr equivalence", &cfg2).expect("modulate");
        let rx2 = add_awgn(&tx2, 16.0, 5);
        let cpu2 = psk8_demodulate_soft(&rx2, &cfg2).expect("cpu");
        let gpu2 = psk8_demodulate_soft_gpu(&rx2, &cfg2, &c).expect("gpu");
        let mut rev_flips = 0u32;
        let mut rev_total = 0u32;
        for k in 0..(cpu2.len() / 3) {
            for j in 0..3 {
                let a = cpu2[k * 3 + j];
                let b = gpu2[k * 3 + (2 - j)];
                if a.abs() > 0.05 && b.abs() > 0.05 {
                    rev_total += 1;
                    if a.signum() != b.signum() {
                        rev_flips += 1;
                    }
                }
            }
        }
        println!(
            "  under REVERSED per-symbol bit order: {rev_flips} flips of {rev_total} ({:.1}%)",
            100.0 * rev_flips as f32 / rev_total.max(1) as f32
        );
    }

    assert_eq!(
        sign_flips, 0,
        "{sign_flips} of {total} LLRs disagree in SIGN between GPU and CPU while both are confident"
    );
    // f32 across two implementations will not be bit-identical; 1% relative is far
    // tighter than anything a decoder cares about while still catching a genuine
    // formula difference (the SC-FDMA case was 71x, not 1%).
    assert!(
        worst_rel < 0.01,
        "GPU and CPU soft LLRs differ by up to {:.1}% — a magnitude divergence is invisible to \
         every FEC decoder here (all scale-invariant) but mis-weights HARQ combining",
        worst_rel * 100.0
    );
}
