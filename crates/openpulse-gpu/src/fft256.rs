//! GPU-accelerated 256-point complex FFT/IFFT.

use bytemuck::{Pod, Zeroable};

use crate::GpuContext;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FftParams {
    n_symbols: u32,
    forward: u32,
}

/// Batch 256-point complex FFT or IFFT on the GPU.
///
/// `data` is interleaved `[re0, im0, re1, im1, …]` for `n_symbols` symbols,
/// total length `n_symbols × 512`.  `forward = true` → FFT (angle −2π);
/// `forward = false` → IFFT with 1/256 normalisation.
///
/// Returns `None` on any GPU error or if `data.len()` is not a multiple of 512.
pub fn gpu_fft256_batch(ctx: &GpuContext, data: &[f32], forward: bool) -> Option<Vec<f32>> {
    // Account GPU dispatch+wait time toward the process-wide GPU-busy counter.
    let _gpu_busy = crate::GpuBusyTimer::start();
    if data.is_empty() {
        return Some(Vec::new());
    }
    if !data.len().is_multiple_of(512) {
        return None;
    }
    let n_symbols = (data.len() / 512) as u32;

    let params = FftParams {
        n_symbols,
        forward: u32::from(forward),
    };

    let in_buf = create_storage_buf_f32(&ctx.device, &ctx.queue, data, "fft256-in");

    let params_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fft256-params"),
        size: std::mem::size_of::<FftParams>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    ctx.queue
        .write_buffer(&params_buf, 0, bytemuck::bytes_of(&params));

    let out_size = data.len() as u64 * 4;
    let out_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fft256-out"),
        size: out_size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let out_rb = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fft256-out-rb"),
        size: out_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("fft256-bg"),
        layout: &ctx.fft256_pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: in_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: params_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: out_buf.as_entire_binding(),
            },
        ],
    });

    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("fft256-encoder"),
        });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("fft256-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&ctx.fft256_pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(n_symbols, 1, 1);
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
    use std::f32::consts::PI;

    fn make_ctx() -> Option<std::sync::Arc<GpuContext>> {
        GpuContext::init()
    }

    // Naive O(N²) DFT reference — correct but slow; only used for small N tests.
    fn naive_dft(re: &[f32], im: &[f32], forward: bool) -> (Vec<f32>, Vec<f32>) {
        let n = re.len();
        let sign = if forward { -1.0f32 } else { 1.0 };
        let scale = if forward { 1.0f32 } else { 1.0 / n as f32 };
        let mut out_re = vec![0.0f32; n];
        let mut out_im = vec![0.0f32; n];
        for k in 0..n {
            for j in 0..n {
                let angle = sign * 2.0 * PI * (k * j) as f32 / n as f32;
                out_re[k] += re[j] * angle.cos() - im[j] * angle.sin();
                out_im[k] += re[j] * angle.sin() + im[j] * angle.cos();
            }
            out_re[k] *= scale;
            out_im[k] *= scale;
        }
        (out_re, out_im)
    }

    #[test]
    fn fft256_gpu_matches_reference_dft() {
        let Some(ctx) = make_ctx() else {
            eprintln!("no GPU adapter — skipping");
            return;
        };
        let n = 256usize;
        let re_in: Vec<f32> = (0..n)
            .map(|i| (2.0 * PI * 10.0 * i as f32 / n as f32).cos())
            .collect();
        let im_in = vec![0.0f32; n];
        let flat: Vec<f32> = re_in
            .iter()
            .zip(im_in.iter())
            .flat_map(|(&r, &im)| [r, im])
            .collect();

        let gpu_out = gpu_fft256_batch(&ctx, &flat, true).expect("GPU FFT failed");
        assert_eq!(gpu_out.len(), n * 2);

        // Only compare a subset of bins to keep naive O(N²) DFT manageable.
        let (ref_re, ref_im) = naive_dft(&re_in, &im_in, true);
        for k in [0usize, 1, 9, 10, 11, 127, 128, 245, 246, 255] {
            let gr = gpu_out[k * 2];
            let gi = gpu_out[k * 2 + 1];
            let err = ((gr - ref_re[k]).powi(2) + (gi - ref_im[k]).powi(2)).sqrt();
            assert!(
                err < 1.0,
                "bin {k}: GPU=({gr:.4},{gi:.4}) ref=({:.4},{:.4}) err={err:.6}",
                ref_re[k],
                ref_im[k]
            );
        }
    }

    #[test]
    fn ifft256_roundtrip() {
        let Some(ctx) = make_ctx() else {
            eprintln!("no GPU adapter — skipping");
            return;
        };
        let n = 256usize;
        let re: Vec<f32> = (0..n)
            .map(|i| (2.0 * PI * 7.0 * i as f32 / n as f32).sin() * 0.5)
            .collect();
        let im = vec![0.0f32; n];
        let flat: Vec<f32> = re
            .iter()
            .zip(im.iter())
            .flat_map(|(&r, &i)| [r, i])
            .collect();

        let fwd = gpu_fft256_batch(&ctx, &flat, true).expect("FFT failed");
        let inv = gpu_fft256_batch(&ctx, &fwd, false).expect("IFFT failed");

        for i in 0..n {
            let err = ((inv[i * 2] - re[i]).powi(2) + inv[i * 2 + 1].powi(2)).sqrt();
            assert!(
                err < 1e-3,
                "sample {i}: recovered=({},{}) orig={} err={err:.6}",
                inv[i * 2],
                inv[i * 2 + 1],
                re[i]
            );
        }
    }
}
