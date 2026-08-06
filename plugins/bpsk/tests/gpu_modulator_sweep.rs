//! Sample-exact GPU-vs-CPU sweep of the BPSK modulator across the `hpx_hf` BPSK rungs.
//!
//! `bpsk_modulate_gpu` runs on every transmit the linksim ladder gate makes, but its only
//! prior coverage was the in-crate `gpu_modulate_matches_cpu`: one 5-byte payload, one mode
//! (BPSK250), and a silent `return` when no adapter is present. Five bytes is 2 560 samples,
//! and the divergence this file exists to catch grows with sample index — so that fixture
//! sat at ~1e-4 and passed a 1e-3 bar no matter what. The rung the ladder actually starts on
//! (SL2 = BPSK31) was never compared at all.
//!
//! Gated on `gpu`, so the `--no-default-features` workspace gate never builds it; the GPU CI
//! job compiles and lints it but does not run it (no adapter on the runner).

#![cfg(feature = "gpu")]

use bpsk_plugin::modulate::{bpsk_modulate, bpsk_modulate_with_gpu};
use openpulse_core::plugin::ModulationConfig;

/// SL2–SL5 of `hpx_hf` — every BPSK rung the rate ladder can occupy. All non-RRC, which is
/// the only path `bpsk_modulate_with_gpu` handles (it delegates `-RRC` back to the CPU).
const LADDER_MODES: [&str; 4] = ["BPSK31", "BPSK63", "BPSK100", "BPSK250"];

/// Vulkan guarantees cos() to 2^-11 (~4.9e-4) on [-π, π] and says nothing outside it, so two
/// independent conforming implementations may differ by ~1e-3 with no defect present. That
/// is where this bar comes from — not from what any particular driver happens to achieve, so
/// it neither drifts with the fix nor tightens onto one vendor's behaviour. Every real
/// divergence found here has been ≥ 25× this.
const TOLERANCE: f32 = 1e-3;

/// Longest wire frame the modulator sees: a 255-byte payload expands to ~2 RS blocks. The
/// divergence is proportional to sample index, so testing below production length is the
/// same mistake the old fixture made.
const WIRE_LEN: usize = 510;

fn payload(len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| {
            // Varied, with runs of 0x00/0xFF where a phase-continuity or symbol-indexing
            // defect shows up.
            match i % 23 {
                0 | 1 => 0x00,
                2 | 3 => 0xff,
                _ => (i.wrapping_mul(37).wrapping_add(11) & 0xff) as u8,
            }
        })
        .collect()
}

#[test]
fn gpu_modulator_matches_cpu_across_every_ladder_rung() {
    let Some(ctx) = openpulse_gpu::GpuContext::init() else {
        panic!("no GPU adapter — this test cannot report a verdict without one");
    };

    // `bpsk_modulate_with_gpu` silently falls back to the CPU when the kernel returns None,
    // so on a host where the adapter initialises but dispatch or readback fails, every
    // comparison below would be CPU-against-CPU and would pass vacuously. Prove the kernel
    // itself round-trips on this host first. (Symbols are built here rather than reusing the
    // plugin's NRZI so this stays a check on the kernel, not on symbol prep.)
    assert!(
        openpulse_gpu::bpsk_modulate_gpu(&ctx, &[false, true, true, false], 32, 1500.0, 8000.0)
            .is_some(),
        "GPU kernel did not round-trip on this host — every comparison below would silently \
         compare the CPU path against itself"
    );

    let data = payload(WIRE_LEN);

    for mode in LADDER_MODES {
        let cfg = ModulationConfig {
            mode: mode.to_string(),
            sample_rate: 8000,
            center_frequency: 1500.0,
            ..ModulationConfig::default()
        };

        let cpu = bpsk_modulate(&data, &cfg).expect("CPU modulate failed");
        let gpu = bpsk_modulate_with_gpu(&data, &cfg, &ctx).expect("GPU modulate failed");
        assert_eq!(cpu.len(), gpu.len(), "{mode}: sample count mismatch");

        let mut worst = 0.0f32;
        let mut worst_at = 0usize;
        for (i, (&c, &g)) in cpu.iter().zip(gpu.iter()).enumerate() {
            if (c - g).abs() > worst {
                worst = (c - g).abs();
                worst_at = i;
            }
        }
        assert!(
            worst < TOLERANCE,
            "{mode}: sample[{worst_at}] of {} cpu={} gpu={} diff={worst:.8}",
            cpu.len(),
            cpu[worst_at],
            gpu[worst_at]
        );

        // Positive control: a degenerate (silent or truncated) buffer would satisfy the
        // comparison above regardless of what either path computed.
        let energy: f32 = cpu.iter().map(|s| s * s).sum();
        assert!(
            energy > 1.0 && cpu.len() > 20_000,
            "{mode}: fixture is degenerate — {} samples, energy {energy}",
            cpu.len()
        );
    }
}

/// Both modulators must track the true carrier, not merely each other. `cos(2π·fc·k/fs)`
/// evaluated with an f32 argument drifts as the argument grows; at the frame lengths the slow
/// rungs produce that reached ~0.026 on BOTH paths, in opposite directions, so a
/// GPU-against-CPU comparison alone can pass while both are wrong.
///
/// `fc/fs = 1500/8000` puts an exact quarter-cycle at every k ≡ 2 (mod 4)·… — concretely,
/// every sample where `fc·k/fs` has fractional part 0.25 or 0.75 must carry a carrier of
/// exactly zero, so the emitted sample must be zero whatever the envelope is.
#[test]
fn both_modulators_track_the_true_carrier_at_frame_end() {
    let Some(ctx) = openpulse_gpu::GpuContext::init() else {
        panic!("no GPU adapter — this test cannot report a verdict without one");
    };
    let cfg = ModulationConfig {
        mode: "BPSK31".to_string(),
        sample_rate: 8000,
        center_frequency: 1500.0,
        ..ModulationConfig::default()
    };
    let data = payload(WIRE_LEN);
    let cpu = bpsk_modulate(&data, &cfg).expect("CPU modulate failed");
    let gpu = bpsk_modulate_with_gpu(&data, &cfg, &ctx).expect("GPU modulate failed");

    // Check the last tenth of the frame, where the unreduced-phase drift was largest.
    let (mut worst_cpu, mut worst_gpu, mut checked) = (0.0f32, 0.0f32, 0usize);
    for k in (cpu.len() * 9 / 10)..cpu.len() {
        let cycles = 1500.0f64 * k as f64 / 8000.0;
        let frac = cycles - cycles.floor();
        // Carrier is exactly zero at a quarter and three-quarter cycle.
        if (frac - 0.25).abs() < 1e-12 || (frac - 0.75).abs() < 1e-12 {
            worst_cpu = worst_cpu.max(cpu[k].abs());
            worst_gpu = worst_gpu.max(gpu[k].abs());
            checked += 1;
        }
    }
    assert!(
        checked > 100,
        "found only {checked} exact carrier zeros to check"
    );
    assert!(
        worst_cpu < TOLERANCE && worst_gpu < TOLERANCE,
        "carrier zeros are not zero at frame end: worst cpu {worst_cpu:.6}, gpu {worst_gpu:.6} \
         over {checked} samples — the phase argument is not being reduced"
    );
}
