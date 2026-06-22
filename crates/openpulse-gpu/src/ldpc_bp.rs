//! Prototype GPU min-sum belief-propagation LDPC decoder.
//!
//! Flooding-schedule min-sum BP mirroring the CPU decoder in
//! `openpulse-core::ldpc`, with two compute kernels per iteration:
//! a variable-node accumulate and a check-node min-sum update. Batches `B`
//! independent codewords through one dispatch so the (otherwise overhead-bound)
//! single-block work can be amortised. Built to MEASURE whether GPU LDPC is
//! worth productising for HF — see the ignored benchmark in the tests module.

use std::sync::Arc;
use wgpu::util::DeviceExt;

use crate::GpuContext;

/// Maximum supported check-node degree (local snapshot array size in the shader).
const MAX_DEG: usize = 32;

const ACCUMULATE_WGSL: &str = r#"
struct Params { b: u32, n: u32, m: u32, e: u32 };
@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> ch: array<f32>;
@group(0) @binding(2) var<storage, read_write> total: array<f32>;
@group(0) @binding(3) var<storage, read> c2v: array<f32>;
// Packed var graph: [var_off (n+1 entries)] ++ [var_edge (e entries)].
@group(0) @binding(4) var<storage, read> var_graph: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if (idx >= p.b * p.n) { return; }
    let blk = idx / p.n;
    let v = idx % p.n;
    var acc = ch[idx];
    let cbase = blk * p.e;
    let voff = p.n + 1u; // var_edge base within var_graph
    let start = var_graph[v];
    let end = var_graph[v + 1u];
    for (var j = start; j < end; j = j + 1u) {
        acc = acc + c2v[cbase + var_graph[voff + j]];
    }
    total[idx] = acc;
}
"#;

const CHECK_WGSL: &str = r#"
struct Params { b: u32, n: u32, m: u32, e: u32 };
@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> total: array<f32>;
@group(0) @binding(2) var<storage, read_write> c2v: array<f32>;
@group(0) @binding(3) var<storage, read> edge_var: array<u32>;
@group(0) @binding(4) var<storage, read> check_off: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if (idx >= p.b * p.m) { return; }
    let blk = idx / p.m;
    let c = idx % p.m;
    let start = check_off[c];
    let end = check_off[c + 1u];
    let deg = end - start;
    let nbase = blk * p.n;
    let cbase = blk * p.e;

    var ext: array<f32, 32>;
    for (var t = 0u; t < deg; t = t + 1u) {
        let e = start + t;
        ext[t] = total[nbase + edge_var[e]] - c2v[cbase + e];
    }
    for (var ti = 0u; ti < deg; ti = ti + 1u) {
        var prod_sign = 1.0;
        var min_abs = 3.4e38;
        for (var tj = 0u; tj < deg; tj = tj + 1u) {
            if (tj == ti) { continue; }
            let e2 = ext[tj];
            if (e2 < 0.0) { prod_sign = -prod_sign; }
            let a = abs(e2);
            if (a < min_abs) { min_abs = a; }
        }
        c2v[cbase + start + ti] = prod_sign * min_abs;
    }
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    b: u32,
    n: u32,
    m: u32,
    e: u32,
}

/// A GPU min-sum BP decoder pre-built for one Tanner graph.
///
/// Pipelines and the static graph buffers are created once; [`decode`](Self::decode)
/// runs the message-passing iterations for `blocks` codewords at a time.
pub struct GpuLdpcDecoder {
    ctx: Arc<GpuContext>,
    accumulate: wgpu::ComputePipeline,
    check: wgpu::ComputePipeline,
    n: u32,
    m: u32,
    e: u32,
    // Static graph buffers (shared by every block).
    edge_var: wgpu::Buffer,
    check_off: wgpu::Buffer,
    var_graph: wgpu::Buffer,
}

impl GpuLdpcDecoder {
    /// Build a decoder for the Tanner graph `check_to_vars` over `n` variables.
    ///
    /// Returns `None` if any check exceeds [`MAX_DEG`] (the shader's local snapshot size).
    pub fn new(ctx: Arc<GpuContext>, check_to_vars: &[Vec<usize>], n: usize) -> Option<Self> {
        let m = check_to_vars.len();

        // Flatten the check->var edges (edge order: contiguous per check).
        let mut edge_var: Vec<u32> = Vec::new();
        let mut check_off: Vec<u32> = Vec::with_capacity(m + 1);
        check_off.push(0);
        for vars in check_to_vars {
            if vars.len() > MAX_DEG {
                return None;
            }
            for &v in vars {
                edge_var.push(v as u32);
            }
            check_off.push(edge_var.len() as u32);
        }
        let e = edge_var.len();

        // Invert to var->edge adjacency.
        let mut var_edges: Vec<Vec<u32>> = vec![Vec::new(); n];
        for (ei, &v) in edge_var.iter().enumerate() {
            var_edges[v as usize].push(ei as u32);
        }
        // Pack as [var_off (n+1)] ++ [var_edge (e)] so accumulate needs only one
        // storage binding (downlevel_defaults caps storage buffers at 4/stage).
        let mut var_off: Vec<u32> = Vec::with_capacity(n + 1);
        let mut var_edge: Vec<u32> = Vec::with_capacity(e);
        var_off.push(0);
        for ve in &var_edges {
            var_edge.extend_from_slice(ve);
            var_off.push(var_edge.len() as u32);
        }
        let mut var_graph = var_off;
        var_graph.extend_from_slice(&var_edge);

        let device = &ctx.device;
        let mk = |wgsl: &str, label: &str| {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(wgsl.into()),
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: None,
                module: &module,
                entry_point: "main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            })
        };
        let accumulate = mk(ACCUMULATE_WGSL, "ldpc-accumulate");
        let check = mk(CHECK_WGSL, "ldpc-check");

        let store = |data: &[u32], label: &str| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::cast_slice(data),
                usage: wgpu::BufferUsages::STORAGE,
            })
        };

        Some(Self {
            accumulate,
            check,
            n: n as u32,
            m: m as u32,
            e: e as u32,
            edge_var: store(&edge_var, "edge_var"),
            check_off: store(&check_off, "check_off"),
            var_graph: store(&var_graph, "var_graph"),
            ctx,
        })
    }

    /// Decode `blocks` codewords (channel LLRs laid out block-major, `blocks * n` floats).
    ///
    /// Runs `iters` flooding iterations, then a final accumulate; returns hard bits
    /// (`blocks * n` booleans, `true` = 1) via the sign of the accumulated LLR.
    pub fn decode(&self, ch_llrs: &[f32], blocks: usize, iters: u32) -> Vec<bool> {
        let _gpu_busy = crate::GpuBusyTimer::start();
        let device = &self.ctx.device;
        let queue = &self.ctx.queue;
        let n = self.n as usize;
        let e = self.e as usize;
        assert_eq!(ch_llrs.len(), blocks * n);

        let params = Params {
            b: blocks as u32,
            n: self.n,
            m: self.m,
            e: self.e,
        };
        let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ldpc-params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let ch_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ldpc-ch"),
            contents: bytemuck::cast_slice(ch_llrs),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let total_bytes = (blocks * n * 4) as u64;
        let total_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ldpc-total"),
            size: total_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let c2v_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ldpc-c2v"),
            size: (blocks * e * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // c2v starts at 0 (mapped_at_creation:false zero-inits via clear below).
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ldpc-staging"),
            size: total_bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let acc_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ldpc-acc-bg"),
            layout: &self.accumulate.get_bind_group_layout(0),
            entries: &[
                bind(0, &params_buf),
                bind(1, &ch_buf),
                bind(2, &total_buf),
                bind(3, &c2v_buf),
                bind(4, &self.var_graph),
            ],
        });
        let chk_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ldpc-chk-bg"),
            layout: &self.check.get_bind_group_layout(0),
            entries: &[
                bind(0, &params_buf),
                bind(1, &total_buf),
                bind(2, &c2v_buf),
                bind(3, &self.edge_var),
                bind(4, &self.check_off),
            ],
        });

        let acc_groups = (blocks as u32 * self.n).div_ceil(64);
        let chk_groups = (blocks as u32 * self.m).div_ceil(64);

        let mut enc =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        enc.clear_buffer(&c2v_buf, 0, None);
        let dispatch = |enc: &mut wgpu::CommandEncoder, pipe, bg, groups| {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(pipe);
            pass.set_bind_group(0, bg, &[]);
            pass.dispatch_workgroups(groups, 1, 1);
        };
        for _ in 0..iters {
            dispatch(&mut enc, &self.accumulate, &acc_bg, acc_groups);
            dispatch(&mut enc, &self.check, &chk_bg, chk_groups);
        }
        // Final accumulate so `total` reflects the last check update.
        dispatch(&mut enc, &self.accumulate, &acc_bg, acc_groups);
        enc.copy_buffer_to_buffer(&total_buf, 0, &staging, 0, total_bytes);
        queue.submit(Some(enc.finish()));

        let slice = staging.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::Maintain::Wait);
        let data = slice.get_mapped_range();
        let totals: &[f32] = bytemuck::cast_slice(&data);
        let bits: Vec<bool> = totals.iter().map(|&l| l < 0.0).collect();
        drop(data);
        staging.unmap();
        bits
    }
}

fn bind(binding: u32, buf: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buf.as_entire_binding(),
    }
}
