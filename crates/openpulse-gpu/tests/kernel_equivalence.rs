//! Differential GPU-vs-CPU equivalence for the kernels the in-crate tests exercise weakly.
//!
//! Requires a real wgpu adapter, so it is gated behind `hardware-tests` (off by default):
//! the workspace gate and CI run `--no-default-features` on runners with no adapter, and
//! these tests panic rather than skip. Run with
//! `cargo test -p openpulse-gpu --features hardware-tests --test kernel_equivalence`.
//!
//! The in-crate fixtures compare each shader against a reference written beside it, on an
//! input chosen for tractability: `fft256` uses a single integer-bin real cosine (whose
//! spectrum is real, so the output imaginary path is never checked) for one symbol (so the
//! per-symbol base indexing is never checked); `rrc_fir` uses 13 all-equal taps against a
//! `cpu_fir` local to the test rather than the `FirFilter` production actually runs.
//!
//! These compare against the production references on production-shaped input.

#![cfg(feature = "hardware-tests")]

use openpulse_dsp::filter::FirFilter;
use openpulse_dsp::rrc::generate_rrc_coefficients;
use openpulse_gpu::{gpu_fft256_batch, gpu_rrc_fir, GpuContext};
use rustfft::num_complex::Complex32;
use rustfft::FftPlanner;
use std::sync::Arc;

const FFT_SIZE: usize = 256;

/// Deterministic LCG — a fixed seed keeps a divergence reproducible.
struct Lcg(u64);

impl Lcg {
    fn next_f32(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as f32 / (1u32 << 31) as f32) * 2.0 - 1.0
    }
}

fn ctx() -> Option<Arc<GpuContext>> {
    GpuContext::init()
}

/// The reference SC-FDMA runs when the GPU path is unavailable
/// (`plugins/scfdma/src/demodulate.rs` `FrameFront::new`): rustfft forward, `1/sqrt(N)`
/// applied to the input, no output scaling.
fn cpu_fft256_batch(packed: &[f32]) -> Vec<f32> {
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);
    let n_syms = packed.len() / (FFT_SIZE * 2);
    let mut out = Vec::with_capacity(packed.len());
    for sym in 0..n_syms {
        let base = sym * FFT_SIZE * 2;
        let mut buf: Vec<Complex32> = (0..FFT_SIZE)
            .map(|k| Complex32::new(packed[base + k * 2], packed[base + k * 2 + 1]))
            .collect();
        fft.process(&mut buf);
        for c in buf {
            out.push(c.re);
            out.push(c.im);
        }
    }
    out
}

/// Broadband real input, so every output bin carries a non-trivial imaginary part — the
/// component an integer-bin cosine leaves at zero. Batched, so per-symbol base indexing is
/// exercised. Scaled as `FrameFront::new` scales it.
#[test]
fn fft256_matches_rustfft_on_batched_broadband_input() {
    let Some(ctx) = ctx() else {
        panic!("no GPU adapter — this test cannot report a verdict without one");
    };
    let n_syms = 7;
    let fft_scale = 1.0 / (FFT_SIZE as f32).sqrt();
    let mut rng = Lcg(0x5eed_1234);
    let packed: Vec<f32> = (0..n_syms * FFT_SIZE)
        .flat_map(|_| [rng.next_f32() * fft_scale, 0.0])
        .collect();

    let gpu = gpu_fft256_batch(&ctx, &packed, true).expect("GPU FFT failed");
    let cpu = cpu_fft256_batch(&packed);
    assert_eq!(gpu.len(), cpu.len());

    // Report the worst bin across the whole batch rather than the first mismatch, so a
    // systematic defect is distinguishable from one bad bin.
    let mut worst = 0.0f32;
    let mut worst_at = (0usize, 0usize);
    let mut worst_im = 0.0f32;
    for sym in 0..n_syms {
        for k in 0..FFT_SIZE {
            let i = sym * FFT_SIZE * 2 + k * 2;
            let err = ((gpu[i] - cpu[i]).powi(2) + (gpu[i + 1] - cpu[i + 1]).powi(2)).sqrt();
            if err > worst {
                worst = err;
                worst_at = (sym, k);
                worst_im = (gpu[i + 1] - cpu[i + 1]).abs();
            }
        }
    }
    // A real broadband spectrum has |X[k]| of order 1 here; 1e-3 is far below any
    // structural defect (bit-reversal, twiddle sign, base offset) and far above f32 noise.
    assert!(
        worst < 1e-3,
        "worst bin sym {} k {}: err {worst:.6} (imag component {worst_im:.6})",
        worst_at.0,
        worst_at.1
    );

    // Positive control: the imaginary parts this fixture exists to check are actually
    // non-trivial, so passing means agreement rather than comparing zero against zero.
    let max_im = cpu
        .iter()
        .skip(1)
        .step_by(2)
        .fold(0.0f32, |a, &b| a.max(b.abs()));
    assert!(
        max_im > 0.1,
        "fixture is not exercising the imaginary path: max |im| {max_im}"
    );
}

/// `gpu_rrc_fir` against `FirFilter` — the filter the four calling plugins run when the GPU
/// path returns `None` — with real RRC coefficients rather than a uniform 13-tap kernel.
#[test]
fn rrc_fir_matches_production_fir_filter() {
    let Some(ctx) = ctx() else {
        panic!("no GPU adapter — this test cannot report a verdict without one");
    };
    // BPSK250 at 8 kHz: n = 32 samples/symbol, RRC_SPAN_SYMBOLS = 10 → 321 taps.
    let (fs, baud, alpha, num_taps) = (8000.0f32, 250.0f32, 0.35f32, 321usize);
    let coeffs = generate_rrc_coefficients(fs, baud, alpha, num_taps);

    let mut rng = Lcg(0xc0ff_ee01);
    let samples: Vec<f32> = (0..4096).map(|_| rng.next_f32()).collect();

    let gpu = gpu_rrc_fir(&ctx, &samples, &coeffs).expect("GPU FIR failed");
    let cpu = FirFilter::new(coeffs.clone()).apply(&samples);
    assert_eq!(gpu.len(), cpu.len());

    let mut worst = 0.0f32;
    let mut worst_at = 0usize;
    for (i, (&g, &c)) in gpu.iter().zip(cpu.iter()).enumerate() {
        if (g - c).abs() > worst {
            worst = (g - c).abs();
            worst_at = i;
        }
    }
    // 321 taps accumulated in f32; the GPU may contract to FMA, so exact equality is not
    // the bar. Any tap-order, indexing or boundary defect is orders of magnitude larger.
    assert!(
        worst < 1e-4,
        "worst sample {worst_at}: GPU {} CPU {} diff {worst:.8}",
        gpu[worst_at],
        cpu[worst_at]
    );

    // Positive control: a signal that never leaves the filter's transient would make the
    // comparison vacuous.
    let energy = cpu.iter().map(|c| c * c).sum::<f32>();
    assert!(
        energy > 1.0,
        "fixture output is near-silent: energy {energy}"
    );
}
