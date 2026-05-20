//! GPU-accelerated max-log-MAP soft demodulation.

use bytemuck::{Pod, Zeroable};

use crate::GpuContext;

/// Parameter uniform for the soft-demod kernel (16 bytes).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SoftDemodParams {
    n_symbols: u32,
    n_points: u32,
    bits_per_sym: u32,
    pad: u32,
}

/// Compute max-log-MAP LLRs for a slice of received IQ symbols on the GPU.
///
/// Returns `Some(llrs)` where `llrs.len() == symbols.len() * bits_per_sym as usize`.
/// Returns `None` on any GPU error; callers must fall back to the CPU path.
///
/// `bit_table[p]` encodes the bit pattern of constellation point `p`: for
/// Gray-coded constellations this is simply `p as u32`.
/// Positive LLR → bit=0 is more likely (same sign convention as CPU paths).
pub fn gpu_soft_demod(
    ctx: &GpuContext,
    symbols: &[(f32, f32)],
    constellation: &[(f32, f32)],
    bit_table: &[u32],
    bits_per_sym: u32,
) -> Option<Vec<f32>> {
    if symbols.is_empty() {
        return Some(Vec::new());
    }
    let n_symbols = symbols.len();
    let n_points = constellation.len();

    let params = SoftDemodParams {
        n_symbols: n_symbols as u32,
        n_points: n_points as u32,
        bits_per_sym,
        pad: 0,
    };

    // Flatten (f32, f32) slices to &[f32] for upload.
    let sym_flat: Vec<f32> = symbols.iter().flat_map(|&(i, q)| [i, q]).collect();
    let constel_flat: Vec<f32> = constellation.iter().flat_map(|&(i, q)| [i, q]).collect();

    let out_count = n_symbols * bits_per_sym as usize;

    // ── Buffers ───────────────────────────────────────────────────────────────

    let sym_buf = create_storage_buf_f32(&ctx.device, &ctx.queue, &sym_flat, "sdemod-sym");
    let constel_buf =
        create_storage_buf_f32(&ctx.device, &ctx.queue, &constel_flat, "sdemod-constel");
    let bit_buf = create_storage_buf_u32(&ctx.device, &ctx.queue, bit_table, "sdemod-bittable");

    let params_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("sdemod-params"),
        size: std::mem::size_of::<SoftDemodParams>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    ctx.queue
        .write_buffer(&params_buf, 0, bytemuck::bytes_of(&params));

    let llr_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("sdemod-llr"),
        size: (out_count * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let llr_rb = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("sdemod-llr-rb"),
        size: (out_count * 4) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    // ── Dispatch ──────────────────────────────────────────────────────────────

    let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("sdemod-bg"),
        layout: &ctx.soft_demod_pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: sym_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: constel_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: bit_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: params_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: llr_buf.as_entire_binding(),
            },
        ],
    });

    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("sdemod-encoder"),
        });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("sdemod-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&ctx.soft_demod_pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        // One workgroup per symbol; threads within the workgroup cooperate over
        // constellation points.
        pass.dispatch_workgroups(n_symbols as u32, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&llr_buf, 0, &llr_rb, 0, (out_count * 4) as u64);
    ctx.queue.submit(Some(encoder.finish()));

    // ── Readback ──────────────────────────────────────────────────────────────

    readback_f32(&ctx.device, &llr_rb, out_count)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn create_storage_buf_f32(
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

fn create_storage_buf_u32(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    data: &[u32],
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

fn readback_f32(device: &wgpu::Device, buf: &wgpu::Buffer, _len: usize) -> Option<Vec<f32>> {
    let slice = buf.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device.poll(wgpu::Maintain::Wait);
    rx.recv().ok()?.ok()?;
    let data = slice.get_mapped_range();
    let out: Vec<f32> = bytemuck::cast_slice::<u8, f32>(&data).to_vec();
    drop(data);
    buf.unmap();
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ctx() -> Option<std::sync::Arc<GpuContext>> {
        GpuContext::init()
    }

    #[test]
    fn gpu_soft_demod_64qam_matches_cpu() {
        let Some(ctx) = make_ctx() else {
            eprintln!("no GPU adapter — skipping");
            return;
        };
        // Build 64-point constellation: (0..64).map(|p| gray_map_64qam(p as u8))
        // Replicated here to avoid a plugin dependency.
        let constellation: Vec<(f32, f32)> = (0..64u8)
            .map(|p| {
                let hi = (p >> 3) & 7;
                let lo = p & 7;
                let pam8 = |g: u8| -> f32 {
                    let level: i8 = match g {
                        0b000 => -7,
                        0b001 => -5,
                        0b011 => -3,
                        0b010 => -1,
                        0b110 => 1,
                        0b111 => 3,
                        0b101 => 5,
                        _ => 7,
                    };
                    level as f32 / 7.0 / std::f32::consts::SQRT_2
                };
                (pam8(hi), pam8(lo))
            })
            .collect();
        let bit_table: Vec<u32> = (0..64u32).collect();

        // Single received symbol: the first constellation point (p=0).
        let (ci, cq) = constellation[0];
        let symbols = vec![(ci, cq)];
        let llrs = gpu_soft_demod(&ctx, &symbols, &constellation, &bit_table, 6)
            .expect("GPU soft-demod failed");
        assert_eq!(llrs.len(), 6);

        // CPU reference for the same symbol.
        let cpu_llrs: Vec<f32> = (0..6u8)
            .map(|bit_pos| {
                let mask = 1u8 << bit_pos;
                let mut min0 = f32::MAX;
                let mut min1 = f32::MAX;
                for (idx, &(pi, pq)) in constellation.iter().enumerate() {
                    let d = (ci - pi).powi(2) + (cq - pq).powi(2);
                    if idx as u8 & mask == 0 {
                        if d < min0 {
                            min0 = d;
                        }
                    } else if d < min1 {
                        min1 = d;
                    }
                }
                min1 - min0
            })
            .collect();

        for (i, (&g, &c)) in llrs.iter().zip(cpu_llrs.iter()).enumerate() {
            assert!(
                (g - c).abs() < 1e-3,
                "LLR[{i}] GPU={g} CPU={c} diff={}",
                (g - c).abs()
            );
        }
    }

    #[test]
    fn gpu_soft_demod_8psk_matches_cpu() {
        let Some(ctx) = make_ctx() else {
            eprintln!("no GPU adapter — skipping");
            return;
        };
        // 8-point PSK constellation at unit radius, Gray-coded phases.
        use std::f32::consts::PI;
        let phases = [
            0.0f32,
            PI / 4.0,
            PI / 2.0,
            3.0 * PI / 4.0,
            PI,
            5.0 * PI / 4.0,
            3.0 * PI / 2.0,
            7.0 * PI / 4.0,
        ];
        let constellation: Vec<(f32, f32)> = phases.iter().map(|&a| (a.cos(), a.sin())).collect();
        let bit_table: Vec<u32> = (0..8u32).collect();

        let (ci, cq) = constellation[3];
        let symbols = vec![(ci, cq)];
        let llrs = gpu_soft_demod(&ctx, &symbols, &constellation, &bit_table, 3)
            .expect("GPU 8PSK soft-demod failed");
        assert_eq!(llrs.len(), 3);

        let cpu_llrs: Vec<f32> = (0..3u8)
            .map(|b| {
                let mut min0 = f32::MAX;
                let mut min1 = f32::MAX;
                for (p, &(pi, pq)) in constellation.iter().enumerate() {
                    let d = (ci - pi).powi(2) + (cq - pq).powi(2);
                    if (p >> b as usize) & 1 == 0 {
                        if d < min0 {
                            min0 = d;
                        }
                    } else if d < min1 {
                        min1 = d;
                    }
                }
                min1 - min0
            })
            .collect();

        for (i, (&g, &c)) in llrs.iter().zip(cpu_llrs.iter()).enumerate() {
            assert!((g - c).abs() < 1e-3, "8PSK LLR[{i}] GPU={g} CPU={c}");
        }
    }
}
