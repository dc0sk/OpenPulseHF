//! BPSK modulator.
//!
//! The modulation pipeline is:
//!
//! ```text
//! bytes → bits (LSB-first) → NRZI encode → symbols (+1/−1)
//!       → raised-cosine pulse shaping → carrier mix → audio samples
//! ```

use std::f32::consts::PI;

use openpulse_core::error::ModemError;
use openpulse_core::plugin::ModulationConfig;

use crate::parse_baud_rate;

/// Number of preamble symbols prepended to every transmission.
pub const PREAMBLE_SYMS: usize = 32;
/// Number of tail symbols appended after data to let the signal decay.
pub const TAIL_SYMS: usize = 8;

// ── Public entry point ────────────────────────────────────────────────────────

/// Modulate `data` bytes to a vector of normalised PCM samples.
pub fn bpsk_modulate(data: &[u8], config: &ModulationConfig) -> Result<Vec<f32>, ModemError> {
    let baud = parse_baud_rate(&config.mode)?;
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    let n = samples_per_symbol(fs, baud)?;

    // Build the bit stream: preamble (all 1s → alternating phases) + data + tail
    let mut bits: Vec<bool> = Vec::new();
    // Preamble: alternating 1/0 bits so NRZI gives +1,-1,+1,−1 …
    for i in 0..PREAMBLE_SYMS {
        bits.push(i % 2 == 0); // 1,0,1,0,...
    }
    bits.extend(bytes_to_bits(data));
    // Tail: all zeros (no phase change) so signal fades smoothly
    bits.extend(std::iter::repeat_n(false, TAIL_SYMS));

    // NRZI encode
    let symbols = nrzi_encode(&bits);

    // Render samples
    let total = symbols.len() * n;
    let mut out = vec![0.0f32; total];
    let two_pi = 2.0 * PI;

    for (sym_idx, &phase_neg) in symbols.iter().enumerate() {
        let amplitude = if phase_neg { -1.0f32 } else { 1.0f32 };
        let sym_start = sym_idx * n;

        for i in 0..n {
            // Raised-cosine (Hann) amplitude envelope – smoothly ramps 0→1→0
            // across the symbol period, eliminating abrupt phase-change clicks.
            let envelope = 0.5 * (1.0 - (two_pi * i as f32 / n as f32).cos());

            // The global sample index determines the carrier phase.
            let t = (sym_start + i) as f32 / fs;
            let carrier = (two_pi * fc * t).cos();

            out[sym_start + i] = amplitude * envelope * carrier;
        }
    }

    Ok(out)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Convert bytes to LSB-first bits.
pub(crate) fn bytes_to_bits(bytes: &[u8]) -> Vec<bool> {
    let mut bits = Vec::with_capacity(bytes.len() * 8);
    for &b in bytes {
        for shift in 0..8u8 {
            bits.push((b >> shift) & 1 == 1);
        }
    }
    bits
}

/// NRZI encoding: bit `true` ("1") → flip phase; `false` ("0") → keep phase.
/// Returns `true` for negative phase (180°), `false` for positive (0°).
pub(crate) fn nrzi_encode(bits: &[bool]) -> Vec<bool> {
    let mut phase_neg = false;
    bits.iter()
        .map(|&flip| {
            if flip {
                phase_neg = !phase_neg;
            }
            phase_neg
        })
        .collect()
}

/// Compute integer samples-per-symbol, returning an error when the ratio
/// would be less than 4 (DSP cannot work reliably below that).
pub(crate) fn samples_per_symbol(sample_rate: f32, baud: f32) -> Result<usize, ModemError> {
    let n = (sample_rate / baud).round() as usize;
    if n < 4 {
        return Err(ModemError::Configuration(format!(
            "sample rate {sample_rate} Hz is too low for {baud} baud \
             (need at least 4 samples/symbol)"
        )));
    }
    Ok(n)
}

/// GPU-accelerated modulation: byte→bit→NRZI on CPU, sample rendering on GPU.
#[cfg(feature = "gpu")]
pub fn bpsk_modulate_with_gpu(
    data: &[u8],
    config: &ModulationConfig,
    ctx: &openpulse_gpu::GpuContext,
) -> Result<Vec<f32>, ModemError> {
    let baud = parse_baud_rate(&config.mode)?;
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    let n = samples_per_symbol(fs, baud)?;

    let mut bits: Vec<bool> = Vec::new();
    for i in 0..PREAMBLE_SYMS {
        bits.push(i % 2 == 0);
    }
    bits.extend(bytes_to_bits(data));
    bits.extend(std::iter::repeat_n(false, TAIL_SYMS));

    let symbols = nrzi_encode(&bits);
    let out = openpulse_gpu::bpsk_modulate_gpu(ctx, &symbols, n, fc, fs);
    Ok(out)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use bytemuck::{Pod, Zeroable};
    use openpulse_core::plugin::ModulationConfig;

    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    struct Params {
        input_len: u32,
        output_len: u32,
        _pad0: u32,
        _pad1: u32,
    }

    async fn gpu_bits_lsb_from_bytes(bytes: &[u8]) -> Option<Vec<u32>> {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await?;

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("bpsk-gpu-bits-device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::downlevel_defaults(),
                },
                None,
            )
            .await
            .ok()?;

        let input_u32: Vec<u32> = bytes.iter().map(|b| *b as u32).collect();
        let output_len = input_u32.len() * 8;

        let input_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bits-input"),
            size: (input_u32.len() * std::mem::size_of::<u32>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&input_buf, 0, bytemuck::cast_slice(&input_u32));

        let output_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bits-output"),
            size: (output_len * std::mem::size_of::<u32>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let readback_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bits-readback"),
            size: (output_len * std::mem::size_of::<u32>()) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let params = Params {
            input_len: input_u32.len() as u32,
            output_len: output_len as u32,
            _pad0: 0,
            _pad1: 0,
        };
        let params_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bits-params"),
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&params_buf, 0, bytemuck::bytes_of(&params));

        let shader_src = r#"
struct Params {
    input_len: u32,
    output_len: u32,
    _pad0: u32,
    _pad1: u32,
};

@group(0) @binding(0) var<storage, read> in_bytes: array<u32>;
@group(0) @binding(1) var<storage, read_write> out_bits: array<u32>;
@group(0) @binding(2) var<uniform> params: Params;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if (idx >= params.output_len) {
        return;
    }
    let byte_idx = idx / 8u;
    let bit_idx = idx % 8u;
    out_bits[idx] = (in_bytes[byte_idx] >> bit_idx) & 1u;
}
"#;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bits-kernel"),
            source: wgpu::ShaderSource::Wgsl(shader_src.into()),
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("bits-pipeline"),
            layout: None,
            module: &shader,
            entry_point: "main",
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bits-bind-group"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: input_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: output_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: params_buf.as_entire_binding(),
                },
            ],
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("bits-encoder"),
        });

        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("bits-pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            let workgroups = (output_len as u32).div_ceil(64);
            pass.dispatch_workgroups(workgroups, 1, 1);
        }

        encoder.copy_buffer_to_buffer(
            &output_buf,
            0,
            &readback_buf,
            0,
            (output_len * std::mem::size_of::<u32>()) as u64,
        );
        queue.submit(Some(encoder.finish()));

        let slice = readback_buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        device.poll(wgpu::Maintain::Wait);
        rx.recv().ok()?.ok()?;

        let data = slice.get_mapped_range();
        let out: Vec<u32> = bytemuck::cast_slice::<u8, u32>(&data).to_vec();
        drop(data);
        readback_buf.unmap();
        Some(out)
    }

    async fn gpu_symbols_from_bits(bits: &[u32]) -> Option<Vec<f32>> {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await?;

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("bpsk-gpu-syms-device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::downlevel_defaults(),
                },
                None,
            )
            .await
            .ok()?;

        let input_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("syms-input"),
            size: (bits.len() * std::mem::size_of::<u32>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&input_buf, 0, bytemuck::cast_slice(bits));

        let output_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("syms-output"),
            size: (bits.len() * std::mem::size_of::<f32>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let readback_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("syms-readback"),
            size: (bits.len() * std::mem::size_of::<f32>()) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let params = Params {
            input_len: bits.len() as u32,
            output_len: bits.len() as u32,
            _pad0: 0,
            _pad1: 0,
        };
        let params_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("syms-params"),
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&params_buf, 0, bytemuck::bytes_of(&params));

        let shader_src = r#"
struct Params {
    input_len: u32,
    output_len: u32,
    _pad0: u32,
    _pad1: u32,
};

@group(0) @binding(0) var<storage, read> in_bits: array<u32>;
@group(0) @binding(1) var<storage, read_write> out_syms: array<f32>;
@group(0) @binding(2) var<uniform> params: Params;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if (idx >= params.output_len) {
        return;
    }
    out_syms[idx] = select(1.0, -1.0, in_bits[idx] == 1u);
}
"#;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("syms-kernel"),
            source: wgpu::ShaderSource::Wgsl(shader_src.into()),
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("syms-pipeline"),
            layout: None,
            module: &shader,
            entry_point: "main",
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("syms-bind-group"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: input_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: output_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: params_buf.as_entire_binding(),
                },
            ],
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("syms-encoder"),
        });

        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("syms-pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            let workgroups = (bits.len() as u32).div_ceil(64);
            pass.dispatch_workgroups(workgroups, 1, 1);
        }

        encoder.copy_buffer_to_buffer(
            &output_buf,
            0,
            &readback_buf,
            0,
            (bits.len() * std::mem::size_of::<f32>()) as u64,
        );
        queue.submit(Some(encoder.finish()));

        let slice = readback_buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        device.poll(wgpu::Maintain::Wait);
        rx.recv().ok()?.ok()?;

        let data = slice.get_mapped_range();
        let out: Vec<f32> = bytemuck::cast_slice::<u8, f32>(&data).to_vec();
        drop(data);
        readback_buf.unmap();
        Some(out)
    }

    #[test]
    fn bytes_to_bits_lsb_first() {
        let bits = bytes_to_bits(&[0b10110001]);
        assert_eq!(
            bits,
            vec![true, false, false, false, true, true, false, true]
        );
    }

    #[test]
    fn nrzi_flip_on_one() {
        // bits: 1,0,1,1 → phases: flip, same, flip, flip
        let phases = nrzi_encode(&[true, false, true, true]);
        assert_eq!(phases, vec![true, true, false, true]);
    }

    #[test]
    fn modulate_produces_correct_length() {
        let cfg = ModulationConfig {
            mode: "BPSK100".to_string(),
            sample_rate: 8000,
            center_frequency: 1500.0,
        };
        let data = b"Hi";
        let samples = bpsk_modulate(data, &cfg).unwrap();
        let n = samples_per_symbol(8000.0, 100.0).unwrap(); // 80
        let expected_syms = PREAMBLE_SYMS + data.len() * 8 + TAIL_SYMS;
        assert_eq!(samples.len(), expected_syms * n);
    }

    #[test]
    fn samples_within_range() {
        let cfg = ModulationConfig::default();
        let samples = bpsk_modulate(b"test", &cfg).unwrap();
        for &s in &samples {
            assert!(s >= -1.0 && s <= 1.0, "sample {s} out of range");
        }
    }

    #[test]
    fn cpu_gpu_bits_kernel_equivalence() {
        let payload = [0xB1u8, 0x02, 0xFF, 0x00, 0x73];
        let cpu_bits: Vec<u32> = bytes_to_bits(&payload)
            .iter()
            .map(|bit| if *bit { 1 } else { 0 })
            .collect();

        let maybe_gpu_bits = pollster::block_on(gpu_bits_lsb_from_bytes(&payload));
        let Some(gpu_bits) = maybe_gpu_bits else {
            eprintln!("skipping GPU equivalence test: no compatible adapter/device");
            return;
        };

        assert_eq!(gpu_bits, cpu_bits);
    }

    #[test]
    fn cpu_gpu_symbol_map_kernel_equivalence() {
        let bits = [
            true, false, true, true, false, false, true, false, true, false,
        ];
        let nrzi = nrzi_encode(&bits);
        let cpu_syms: Vec<f32> = nrzi
            .iter()
            .map(|phase_neg| if *phase_neg { -1.0 } else { 1.0 })
            .collect();
        let nrzi_u32: Vec<u32> = nrzi
            .iter()
            .map(|phase_neg| if *phase_neg { 1 } else { 0 })
            .collect();

        let maybe_gpu_syms = pollster::block_on(gpu_symbols_from_bits(&nrzi_u32));
        let Some(gpu_syms) = maybe_gpu_syms else {
            eprintln!("skipping GPU equivalence test: no compatible adapter/device");
            return;
        };

        assert_eq!(gpu_syms.len(), cpu_syms.len());
        for (cpu, gpu) in cpu_syms.iter().zip(gpu_syms.iter()) {
            assert!((cpu - gpu).abs() <= 1e-6, "cpu={cpu}, gpu={gpu}");
        }
    }
}
