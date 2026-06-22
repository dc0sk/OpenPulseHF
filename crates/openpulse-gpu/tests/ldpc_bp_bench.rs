//! Prototype benchmark: GPU min-sum BP LDPC decode vs the CPU decoder.
//!
//! Both tests are `#[ignore]` — they need a real wgpu adapter (absent on CI
//! runners). Run locally with:
//!   cargo test -p openpulse-gpu --test ldpc_bp_bench -- --ignored --nocapture

use std::time::Instant;

use openpulse_core::ldpc::{IterativeDecoder, LdpcCodec};
use openpulse_gpu::ldpc_bp::GpuLdpcDecoder;
use openpulse_gpu::GpuContext;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

const ITERS: u32 = 50;

/// Encode random info, push it through an AWGN channel, return (info_bits, codeword_llrs).
fn make_llrs(codec: &LdpcCodec, rng: &mut StdRng, sigma: f32) -> (Vec<bool>, Vec<f32>) {
    let k = codec.k();
    let n = k + codec.m();
    let info_bytes: Vec<u8> = (0..k / 8).map(|_| rng.gen()).collect();
    let cw = codec.encode(&info_bytes);

    let mut info_bits = vec![false; k];
    for (i, b) in info_bits.iter_mut().enumerate() {
        *b = (info_bytes[i / 8] >> (i % 8)) & 1 == 1;
    }

    let mut llrs = vec![0.0f32; n];
    for (i, l) in llrs.iter_mut().enumerate() {
        let bit = (cw[i / 8] >> (i % 8)) & 1 == 1;
        let tx = if bit { -1.0 } else { 1.0 }; // BPSK: 0->+1, 1->-1
                                               // Box-Muller gaussian.
        let u1: f32 = rng.gen::<f32>().max(1e-7);
        let u2: f32 = rng.gen();
        let g = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f32::consts::PI * u2).cos();
        let rx = tx + sigma * g;
        *l = 2.0 * rx / (sigma * sigma);
    }
    (info_bits, llrs)
}

#[test]
#[ignore = "requires a GPU adapter"]
fn gpu_ldpc_matches_cpu() {
    let Some(ctx) = GpuContext::init() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    let codec = LdpcCodec::new();
    let k = codec.k();
    let n = k + codec.m();
    let gpu = GpuLdpcDecoder::new(ctx, codec.check_to_vars(), n).expect("max-deg within bound");

    // Correctness property we control: the GPU min-sum must agree with the CPU
    // min-sum on identical input whenever the CPU converges (both are the same
    // algorithm; whether the *code* recovers truth at a given SNR is separate).
    let mut rng = StdRng::seed_from_u64(7);
    let mut compared = 0;
    let mut mismatches = 0;
    let trials = 24;
    for _ in 0..trials {
        let (_info, llrs) = make_llrs(&codec, &mut rng, 0.5);
        let Ok(cpu_bytes) = codec.decode_soft(&llrs) else {
            continue; // CPU did not converge — nothing to compare against
        };
        let mut cpu_info = vec![false; k];
        for (i, b) in cpu_info.iter_mut().enumerate() {
            *b = (cpu_bytes[i / 8] >> (i % 8)) & 1 == 1;
        }
        let gpu_bits = gpu.decode(&llrs, 1, ITERS);
        compared += 1;
        if gpu_bits[..k] != cpu_info[..] {
            mismatches += 1;
        }
    }
    println!(
        "GPU agreed with CPU on {}/{compared} converged blocks",
        compared - mismatches
    );
    assert!(
        compared >= trials / 2,
        "too few CPU convergences to validate"
    );
    assert_eq!(
        mismatches, 0,
        "{mismatches}/{compared} GPU decodes disagreed with CPU"
    );
}

#[test]
#[ignore = "requires a GPU adapter"]
fn bench_gpu_vs_cpu_ldpc() {
    let Some(ctx) = GpuContext::init() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    let codec = LdpcCodec::new();
    let k = codec.k();
    let n = k + codec.m();
    let gpu = GpuLdpcDecoder::new(ctx, codec.check_to_vars(), n).expect("max-deg within bound");

    let mut rng = StdRng::seed_from_u64(42);
    let sigma = 0.7;

    // Build a pool of independent codewords.
    let pool: Vec<(Vec<bool>, Vec<f32>)> = (0..256)
        .map(|_| make_llrs(&codec, &mut rng, sigma))
        .collect();

    // ---- CPU: decode each block, early-terminating min-sum (its real behaviour). ----
    let cpu_n = 64;
    let t = Instant::now();
    for (_, llrs) in pool.iter().take(cpu_n) {
        let _ = codec.decode_soft(llrs);
    }
    let cpu_per = t.elapsed().as_secs_f64() * 1e3 / cpu_n as f64;

    // ---- GPU warm-up (lazy pipeline/buffer init out of the timed path). ----
    let _ = gpu.decode(&pool[0].1, 1, ITERS);

    println!("\nLDPC GPU min-sum BP prototype  (k={k}, n={n}, {ITERS} iters fixed)");
    println!("CPU min-sum (early-term, avg over {cpu_n}): {cpu_per:.3} ms/block");
    println!(
        "{:>8} | {:>12} | {:>14} | {:>10}",
        "blocks", "GPU total", "GPU per-block", "vs CPU"
    );
    for &b in &[1usize, 8, 64, 256] {
        let mut ch = Vec::with_capacity(b * n);
        for i in 0..b {
            ch.extend_from_slice(&pool[i % pool.len()].1);
        }
        // Average a few runs for stability.
        let runs = 5;
        let t = Instant::now();
        for _ in 0..runs {
            let _ = gpu.decode(&ch, b, ITERS);
        }
        let total_ms = t.elapsed().as_secs_f64() * 1e3 / runs as f64;
        let per = total_ms / b as f64;
        let ratio = cpu_per / per;
        println!("{b:>8} | {total_ms:>9.3} ms | {per:>11.4} ms | {ratio:>7.2}x");
    }
    println!("(vs CPU > 1.0x means GPU is faster per block)");
}
