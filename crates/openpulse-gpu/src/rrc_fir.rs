//! GPU-accelerated FIR convolution for matched RRC filtering.

use bytemuck::{Pod, Zeroable};

use crate::GpuContext;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FirParams {
    n_samples: u32,
    n_taps: u32,
}

/// FIR convolution `y[n] = Σ coeffs[k] * samples[n-k]` on the GPU.
///
/// Negative-index samples are implicitly zero (causal boundary), so the caller
/// can pre-pad `samples` with `group_delay` trailing zeros and trim the output
/// at `group_delay` to match the CPU `FirFilter` API.
///
/// Returns `None` on any GPU error; caller must fall back to the CPU path.
pub fn gpu_rrc_fir(ctx: &GpuContext, samples: &[f32], coeffs: &[f32]) -> Option<Vec<f32>> {
    if samples.is_empty() {
        return Some(Vec::new());
    }
    let n_samples = samples.len() as u32;
    let n_taps = coeffs.len() as u32;

    let params = FirParams { n_samples, n_taps };

    let in_buf = create_storage_buf_f32(&ctx.device, &ctx.queue, samples, "fir-in");
    let coeffs_buf = create_storage_buf_f32(&ctx.device, &ctx.queue, coeffs, "fir-coeffs");

    let params_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fir-params"),
        size: std::mem::size_of::<FirParams>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    ctx.queue
        .write_buffer(&params_buf, 0, bytemuck::bytes_of(&params));

    let out_size = (n_samples as u64) * 4;
    let out_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fir-out"),
        size: out_size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let out_rb = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fir-out-rb"),
        size: out_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("fir-bg"),
        layout: &ctx.rrc_fir_pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: in_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: coeffs_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: params_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: out_buf.as_entire_binding(),
            },
        ],
    });

    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("fir-encoder"),
        });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("fir-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&ctx.rrc_fir_pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(n_samples.div_ceil(256), 1, 1);
    }
    encoder.copy_buffer_to_buffer(&out_buf, 0, &out_rb, 0, out_size);
    ctx.queue.submit(Some(encoder.finish()));

    readback_f32(&ctx.device, &out_rb)
}

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

fn readback_f32(device: &wgpu::Device, buf: &wgpu::Buffer) -> Option<Vec<f32>> {
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

    fn cpu_fir(samples: &[f32], coeffs: &[f32]) -> Vec<f32> {
        let nc = coeffs.len();
        (0..samples.len())
            .map(|n| {
                let lim = nc.min(n + 1);
                (0..lim).map(|k| coeffs[k] * samples[n - k]).sum()
            })
            .collect()
    }

    #[test]
    fn rrc_fir_gpu_matches_cpu() {
        let Some(ctx) = make_ctx() else {
            eprintln!("no GPU adapter — skipping");
            return;
        };
        let samples: Vec<f32> = (0..200).map(|i| ((i as f32) * 0.1).sin()).collect();
        let coeffs: Vec<f32> = vec![1.0 / 13.0; 13];
        let gpu_out = gpu_rrc_fir(&ctx, &samples, &coeffs).expect("GPU FIR failed");
        let cpu_out = cpu_fir(&samples, &coeffs);
        assert_eq!(gpu_out.len(), cpu_out.len());
        for (i, (&g, &c)) in gpu_out.iter().zip(cpu_out.iter()).enumerate() {
            assert!(
                (g - c).abs() < 1e-4,
                "sample[{i}] GPU={g:.6} CPU={c:.6} diff={:.6}",
                (g - c).abs()
            );
        }
    }
}
