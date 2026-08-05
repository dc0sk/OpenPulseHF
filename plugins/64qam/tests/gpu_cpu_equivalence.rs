//! GPU-vs-CPU soft-LLR equivalence for 64QAM.
//!
//! 64QAM shares `gpu_soft_demod` with 8PSK, where feeding an identity `bit_table`
//! made the GPU emit each symbol's LLRs in reversed bit order — invisible to every
//! decode test, because the hard path maps symbols to bits on the CPU and never
//! consults that table.
//!
//! 64QAM should be unaffected: its CPU soft path goes through
//! `openpulse_dsp::constellation::symbol_llrs`, which indexes bits LSB-first —
//! the same convention as the shader. This file exists to **measure** that rather
//! than infer it from reading two functions, and to keep it measured: 6 bits per
//! symbol means a future ordering change has more places to go wrong than 8PSK's
//! three, and reversal there does not leave a fixed point to notice it by.

#![cfg(feature = "gpu")]

use openpulse_core::plugin::{ModulationConfig, PulseShape};
use qam64_plugin::demodulate::{qam64_demodulate_soft, qam64_demodulate_soft_gpu};
use qam64_plugin::modulate::qam64_modulate;

const MODE: &str = "64QAM2000-RRC";
const BITS_PER_SYM: usize = 6;

fn config() -> ModulationConfig {
    ModulationConfig {
        mode: MODE.to_string(),
        sample_rate: 8000,
        center_frequency: 1500.0,
        pulse_shape: PulseShape::Rrc { alpha: 0.35 },
        ..Default::default()
    }
}

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
        "no GPU adapter — 64QAM GPU/CPU soft equivalence is UNMEASURED, not confirmed"
    );
}

#[test]
fn soft_llrs_agree_numerically_and_per_bit_position() {
    let Some(c) = ctx() else { return };
    let cfg = config();
    let tx = qam64_modulate(b"64qam soft llr equivalence probe", &cfg).expect("modulate");

    let mut worst_rel = 0.0f32;
    let mut per_bit_flip = [0u32; BITS_PER_SYM];
    let mut per_bit_total = [0u32; BITS_PER_SYM];
    let mut total = 0u32;

    for snr_db in [34.0f32, 28.0, 22.0] {
        for seed in 0..5u64 {
            let rx = add_awgn(&tx, snr_db, seed * 19 + 3);
            let cpu = qam64_demodulate_soft(&rx, &cfg).expect("cpu soft");
            let gpu = qam64_demodulate_soft_gpu(&rx, &cfg, &c).expect("gpu soft");
            assert_eq!(
                cpu.len(),
                gpu.len(),
                "GPU returned {} LLRs, CPU returned {}",
                gpu.len(),
                cpu.len()
            );
            for (idx, (a, b)) in cpu.iter().zip(gpu.iter()).enumerate() {
                total += 1;
                let scale = a.abs().max(b.abs()).max(1e-3);
                worst_rel = worst_rel.max((a - b).abs() / scale);
                let pos = idx % BITS_PER_SYM;
                per_bit_total[pos] += 1;
                if a.signum() != b.signum() && a.abs() > 0.05 && b.abs() > 0.05 {
                    per_bit_flip[pos] += 1;
                }
            }
        }
    }

    println!("64QAM soft: {total} LLRs, worst relative {worst_rel:.6}");
    for (b, (f, t)) in per_bit_flip.iter().zip(per_bit_total.iter()).enumerate() {
        println!(
            "  bit {b} of {BITS_PER_SYM}: {f} flips of {t} ({:.1}%)",
            100.0 * *f as f32 / (*t).max(1) as f32
        );
    }

    // Per-position, not just in aggregate: an ordering fault concentrates in
    // specific positions, and an aggregate rate can hide which. On 8PSK that
    // breakdown is what identified reversal — bit 1 agreeing perfectly while the
    // outer two each disagreed ~53%.
    let flipped: u32 = per_bit_flip.iter().sum();
    assert_eq!(
        flipped, 0,
        "{flipped} of {total} LLRs disagree in sign between GPU and CPU (per position: {per_bit_flip:?})"
    );
    assert!(
        worst_rel < 0.01,
        "GPU and CPU soft LLRs differ by up to {:.2}% — invisible to every FEC decoder here \
         (all scale-invariant) but mis-weights HARQ combining",
        worst_rel * 100.0
    );
}
