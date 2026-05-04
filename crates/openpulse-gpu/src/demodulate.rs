//! GPU-accelerated BPSK demodulation helpers.

use bytemuck::{Pod, Zeroable};

use crate::GpuContext;

/// Parameter uniform for the BPSK IQ demodulation kernel (32 bytes).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BpskDemodParams {
    n_syms: u32,
    samples_per_sym: u32,
    offset: u32,
    pad0: u32,
    fc: f32,
    sample_rate: f32,
    pad1: f32,
    pad2: f32,
}

/// Parameter uniform for the timing offset search kernel (32 bytes).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct TimingParams {
    n_offsets: u32,
    samples_per_sym: u32,
    preamble_syms: u32,
    pad0: u32,
    fc: f32,
    sample_rate: f32,
    pad1: f32,
    pad2: f32,
}

/// IQ demodulation of `samples` (pre-sliced at timing offset) on the GPU.
///
/// Returns `(i_values, q_values)` — one value per symbol. The `offset` parameter
/// is the original timing offset used only for carrier phase calculation.
pub fn bpsk_iq_demod_gpu(
    ctx: &GpuContext,
    samples: &[f32],
    samples_per_sym: usize,
    fc: f32,
    sample_rate: f32,
    offset: usize,
) -> (Vec<f32>, Vec<f32>) {
    if samples.is_empty() || samples_per_sym == 0 {
        return (Vec::new(), Vec::new());
    }
    let n_syms = samples.len() / samples_per_sym;
    if n_syms == 0 {
        return (Vec::new(), Vec::new());
    }

    let params = BpskDemodParams {
        n_syms: n_syms as u32,
        samples_per_sym: samples_per_sym as u32,
        offset: offset as u32,
        pad0: 0,
        fc,
        sample_rate,
        pad1: 0.0,
        pad2: 0.0,
    };

    // ── Buffers ───────────────────────────────────────────────────────────────

    let in_buf = create_storage_buf_with_data(&ctx.device, &ctx.queue, samples, "bpsk-demod-in");

    let i_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("bpsk-demod-i"),
        size: (n_syms * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let q_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("bpsk-demod-q"),
        size: (n_syms * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let i_rb = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("bpsk-demod-i-rb"),
        size: (n_syms * 4) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let q_rb = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("bpsk-demod-q-rb"),
        size: (n_syms * 4) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let params_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("bpsk-demod-params"),
        size: std::mem::size_of::<BpskDemodParams>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    ctx.queue
        .write_buffer(&params_buf, 0, bytemuck::bytes_of(&params));

    // ── Dispatch ──────────────────────────────────────────────────────────────

    let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("bpsk-demod-bg"),
        layout: &ctx.bpsk_demod_pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: in_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: i_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: q_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: params_buf.as_entire_binding(),
            },
        ],
    });

    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("bpsk-demod-encoder"),
        });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("bpsk-demod-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&ctx.bpsk_demod_pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        let workgroups = (n_syms as u32).div_ceil(64);
        pass.dispatch_workgroups(workgroups, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&i_buf, 0, &i_rb, 0, (n_syms * 4) as u64);
    encoder.copy_buffer_to_buffer(&q_buf, 0, &q_rb, 0, (n_syms * 4) as u64);
    ctx.queue.submit(Some(encoder.finish()));

    // ── Readback ──────────────────────────────────────────────────────────────

    let i_out = readback_f32(&ctx.device, &i_rb, n_syms);
    let q_out = readback_f32(&ctx.device, &q_rb, n_syms);
    (i_out, q_out)
}

/// Parallel timing offset search: returns the offset (0..`samples_per_sym`)
/// that maximises preamble correlation energy.
pub fn timing_offset_search_gpu(
    ctx: &GpuContext,
    samples: &[f32],
    samples_per_sym: usize,
    preamble_syms: usize,
    expected_preamble: &[f32],
    fc: f32,
    sample_rate: f32,
) -> usize {
    let n_offsets = samples_per_sym;
    if samples.is_empty() || n_offsets == 0 {
        return 0;
    }

    let params = TimingParams {
        n_offsets: n_offsets as u32,
        samples_per_sym: samples_per_sym as u32,
        preamble_syms: preamble_syms as u32,
        pad0: 0,
        fc,
        sample_rate,
        pad1: 0.0,
        pad2: 0.0,
    };

    // ── Buffers ───────────────────────────────────────────────────────────────

    let in_buf = create_storage_buf_with_data(&ctx.device, &ctx.queue, samples, "timing-in");
    let preamble_buf = create_storage_buf_with_data(
        &ctx.device,
        &ctx.queue,
        expected_preamble,
        "timing-preamble",
    );

    let energy_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("timing-energy"),
        size: (n_offsets * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let energy_rb = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("timing-energy-rb"),
        size: (n_offsets * 4) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let params_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("timing-params"),
        size: std::mem::size_of::<TimingParams>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    ctx.queue
        .write_buffer(&params_buf, 0, bytemuck::bytes_of(&params));

    // ── Dispatch ──────────────────────────────────────────────────────────────

    let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("timing-bg"),
        layout: &ctx.timing_search_pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: in_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: preamble_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: energy_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: params_buf.as_entire_binding(),
            },
        ],
    });

    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("timing-encoder"),
        });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("timing-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&ctx.timing_search_pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        let workgroups = (n_offsets as u32).div_ceil(64);
        pass.dispatch_workgroups(workgroups, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&energy_buf, 0, &energy_rb, 0, (n_offsets * 4) as u64);
    ctx.queue.submit(Some(encoder.finish()));

    // ── Readback + argmax ─────────────────────────────────────────────────────

    let energies = readback_f32(&ctx.device, &energy_rb, n_offsets);
    energies
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
        .unwrap_or(0)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn create_storage_buf_with_data(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    data: &[f32],
    label: &str,
) -> wgpu::Buffer {
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: (data.len() * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&buf, 0, bytemuck::cast_slice(data));
    buf
}

fn readback_f32(device: &wgpu::Device, buf: &wgpu::Buffer, len: usize) -> Vec<f32> {
    let slice = buf.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device.poll(wgpu::Maintain::Wait);
    if rx.recv().ok().and_then(|r| r.ok()).is_none() {
        return vec![0.0; len];
    }
    let data = slice.get_mapped_range();
    let out: Vec<f32> = bytemuck::cast_slice::<u8, f32>(&data).to_vec();
    drop(data);
    buf.unmap();
    out
}
