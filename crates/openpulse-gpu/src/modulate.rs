//! GPU-accelerated BPSK modulation.

use bytemuck::{Pod, Zeroable};

use crate::GpuContext;

/// Parameter uniform for the BPSK modulation kernel (16 bytes, multiple of 16).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BpskModParams {
    n_syms: u32,
    samples_per_sym: u32,
    fc: f32,
    sample_rate: f32,
}

/// Render NRZI symbols to PCM samples on the GPU.
///
/// `symbols` are NRZI-encoded phase values: `false` → +1 (0°), `true` → −1 (180°).
/// The output length is `symbols.len() × samples_per_sym`.
///
/// Falls back silently to an empty `Vec` if the GPU dispatch fails.
pub fn bpsk_modulate_gpu(
    ctx: &GpuContext,
    symbols: &[bool],
    samples_per_sym: usize,
    fc: f32,
    sample_rate: f32,
) -> Vec<f32> {
    if symbols.is_empty() {
        return Vec::new();
    }
    let n_syms = symbols.len();
    let total_samples = n_syms * samples_per_sym;

    let sym_u32: Vec<u32> = symbols.iter().map(|&p| u32::from(p)).collect();

    let params = BpskModParams {
        n_syms: n_syms as u32,
        samples_per_sym: samples_per_sym as u32,
        fc,
        sample_rate,
    };

    // ── Buffers ───────────────────────────────────────────────────────────────

    let sym_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("bpsk-mod-symbols"),
        size: (sym_u32.len() * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    ctx.queue
        .write_buffer(&sym_buf, 0, bytemuck::cast_slice(&sym_u32));

    let out_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("bpsk-mod-output"),
        size: (total_samples * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });

    let readback_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("bpsk-mod-readback"),
        size: (total_samples * 4) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let params_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("bpsk-mod-params"),
        size: std::mem::size_of::<BpskModParams>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    ctx.queue
        .write_buffer(&params_buf, 0, bytemuck::bytes_of(&params));

    // ── Dispatch ──────────────────────────────────────────────────────────────

    let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("bpsk-mod-bg"),
        layout: &ctx.bpsk_mod_pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: sym_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: out_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: params_buf.as_entire_binding(),
            },
        ],
    });

    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("bpsk-mod-encoder"),
        });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("bpsk-mod-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&ctx.bpsk_mod_pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        let workgroups = (total_samples as u32).div_ceil(64);
        pass.dispatch_workgroups(workgroups, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&out_buf, 0, &readback_buf, 0, (total_samples * 4) as u64);
    ctx.queue.submit(Some(encoder.finish()));

    // ── Readback ──────────────────────────────────────────────────────────────

    let slice = readback_buf.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    ctx.device.poll(wgpu::Maintain::Wait);
    if rx.recv().ok().and_then(|r| r.ok()).is_none() {
        return Vec::new();
    }

    let data = slice.get_mapped_range();
    let out: Vec<f32> = bytemuck::cast_slice::<u8, f32>(&data).to_vec();
    drop(data);
    readback_buf.unmap();
    out
}
