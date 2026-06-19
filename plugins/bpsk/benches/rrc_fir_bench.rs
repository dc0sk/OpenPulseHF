use criterion::{criterion_group, criterion_main, Criterion};

/// Benchmark a naive 512-tap FIR convolution over 8000 samples to estimate
/// real-time viability on embedded targets (RPi4 ≈ 3× slower than typical
/// desktop host).  Target: < 1 ms/block, which would leave 97% headroom at
/// 8 kHz / 8000-sample blocks even on RPi4.
fn fir_apply_naive(coeffs: &[f32], input: &[f32], output: &mut Vec<f32>) {
    let n_taps = coeffs.len();
    let mut state = vec![0.0f32; n_taps - 1];
    output.clear();
    for &sample in input {
        state.insert(0, sample);
        state.pop();
        let y: f32 = state.iter().zip(coeffs).map(|(s, c)| s * c).sum();
        output.push(y);
    }
}

fn bench_rrc_fir_512tap_8000samples(c: &mut Criterion) {
    use std::f32::consts::PI;

    // Generate 512-tap SRRC coefficients at α = 0.35, Rs = 1000 baud, fs = 8000 Hz.
    let num_taps = 512usize;
    let alpha = 0.35f32;
    let rs = 1000.0f32;
    let fs = 8000.0f32;
    let coeffs: Vec<f32> = (0..num_taps)
        .map(|n| {
            let t = (n as f32 - (num_taps as f32 - 1.0) / 2.0) / (fs / rs);
            if t.abs() < 1e-6 {
                1.0 + alpha * (4.0 / PI - 1.0)
            } else if (1.0 - (4.0 * alpha * t).powi(2)).abs() < 1e-6 {
                alpha / 2.0_f32.sqrt()
                    * ((1.0 + 2.0 / PI) * (PI / (4.0 * alpha)).sin()
                        + (1.0 - 2.0 / PI) * (PI / (4.0 * alpha)).cos())
            } else {
                let num = (PI * t * (1.0 - alpha)).sin()
                    + 4.0 * alpha * t * (PI * t * (1.0 + alpha)).cos();
                let den = PI * t * (1.0 - (4.0 * alpha * t).powi(2));
                num / den
            }
        })
        .collect();

    let input: Vec<f32> = (0..8000u32).map(|i| (i as f32 * 0.001).sin()).collect();
    let mut output = Vec::with_capacity(8000);

    c.bench_function("rrc_fir_512tap_8000samples_naive", |b| {
        b.iter(|| fir_apply_naive(&coeffs, &input, &mut output));
    });
}

fn bench_rrc_fir_64tap_8000samples(c: &mut Criterion) {
    use std::f32::consts::PI;

    let num_taps = 64usize;
    let alpha = 0.35f32;
    let rs = 1000.0f32;
    let fs = 8000.0f32;
    let coeffs: Vec<f32> = (0..num_taps)
        .map(|n| {
            let t = (n as f32 - (num_taps as f32 - 1.0) / 2.0) / (fs / rs);
            if t.abs() < 1e-6 {
                1.0 + alpha * (4.0 / PI - 1.0)
            } else if (1.0 - (4.0 * alpha * t).powi(2)).abs() < 1e-6 {
                alpha / 2.0_f32.sqrt()
                    * ((1.0 + 2.0 / PI) * (PI / (4.0 * alpha)).sin()
                        + (1.0 - 2.0 / PI) * (PI / (4.0 * alpha)).cos())
            } else {
                let num = (PI * t * (1.0 - alpha)).sin()
                    + 4.0 * alpha * t * (PI * t * (1.0 + alpha)).cos();
                let den = PI * t * (1.0 - (4.0 * alpha * t).powi(2));
                num / den
            }
        })
        .collect();

    let input: Vec<f32> = (0..8000u32).map(|i| (i as f32 * 0.001).sin()).collect();
    let mut output = Vec::with_capacity(8000);

    c.bench_function("rrc_fir_64tap_8000samples_naive", |b| {
        b.iter(|| fir_apply_naive(&coeffs, &input, &mut output));
    });
}

criterion_group!(
    benches,
    bench_rrc_fir_512tap_8000samples,
    bench_rrc_fir_64tap_8000samples,
);
criterion_main!(benches);
