use criterion::{criterion_group, criterion_main, Criterion};

use bpsk_plugin::BpskPlugin;
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};

fn modulation_config() -> ModulationConfig {
    ModulationConfig {
        mode: "BPSK250".to_string(),
        sample_rate: 8000,
        center_frequency: 1500.0,
    }
}

fn bench_modulate_cpu(c: &mut Criterion) {
    let plugin = BpskPlugin::new();
    let cfg = modulation_config();
    let payload = vec![0x42u8; 256];

    c.bench_function("modulate_cpu_bpsk250_256b", |b| {
        b.iter(|| plugin.modulate(&payload, &cfg).unwrap())
    });
}

fn bench_demodulate_cpu(c: &mut Criterion) {
    let plugin = BpskPlugin::new();
    let cfg = modulation_config();
    let payload = vec![0x42u8; 256];
    let samples = plugin.modulate(&payload, &cfg).unwrap();

    c.bench_function("demodulate_cpu_bpsk250_256b", |b| {
        b.iter(|| plugin.demodulate(&samples, &cfg).unwrap())
    });
}

#[cfg(feature = "gpu")]
fn bench_modulate_gpu(c: &mut Criterion) {
    let Some(ctx) = openpulse_gpu::GpuContext::init() else {
        eprintln!("skipping GPU bench: no compatible adapter");
        return;
    };
    let plugin = BpskPlugin::with_gpu(ctx);
    let cfg = modulation_config();
    let payload = vec![0x42u8; 256];

    c.bench_function("modulate_gpu_bpsk250_256b", |b| {
        b.iter(|| plugin.modulate(&payload, &cfg).unwrap())
    });
}

#[cfg(feature = "gpu")]
fn bench_demodulate_gpu(c: &mut Criterion) {
    let Some(ctx) = openpulse_gpu::GpuContext::init() else {
        eprintln!("skipping GPU bench: no compatible adapter");
        return;
    };
    let cpu_plugin = BpskPlugin::new();
    let cfg = modulation_config();
    let payload = vec![0x42u8; 256];
    let samples = cpu_plugin.modulate(&payload, &cfg).unwrap();

    let gpu_plugin = BpskPlugin::with_gpu(ctx);
    c.bench_function("demodulate_gpu_bpsk250_256b", |b| {
        b.iter(|| gpu_plugin.demodulate(&samples, &cfg).unwrap())
    });
}

#[cfg(not(feature = "gpu"))]
fn bench_modulate_gpu(_c: &mut Criterion) {}
#[cfg(not(feature = "gpu"))]
fn bench_demodulate_gpu(_c: &mut Criterion) {}

criterion_group!(
    benches,
    bench_modulate_cpu,
    bench_demodulate_cpu,
    bench_modulate_gpu,
    bench_demodulate_gpu,
);
criterion_main!(benches);
