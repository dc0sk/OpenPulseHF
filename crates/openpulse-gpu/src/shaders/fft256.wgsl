// Cooley-Tukey radix-2 DIT 256-point complex FFT/IFFT.
//
// Input/output: interleaved [re0, im0, re1, im1, …] for n_symbols symbols,
// total array length n_symbols × 512.  One workgroup per OFDM symbol; 256
// threads per workgroup.  Threads 0–255 load with bit-reversal into workgroup
// shared memory; threads 0–127 execute 128 butterflies per stage (8 stages);
// all 256 threads call workgroupBarrier() to fence each stage.

struct FftParams {
    n_symbols: u32,
    forward:   u32, // 1 = FFT (angle -2π), 0 = IFFT (angle +2π, scale 1/256)
}

@group(0) @binding(0) var<storage, read>       in_data:  array<f32>;
@group(0) @binding(1) var<uniform>             params:   FftParams;
@group(0) @binding(2) var<storage, read_write> out_data: array<f32>;

var<workgroup> wg_re: array<f32, 256>;
var<workgroup> wg_im: array<f32, 256>;

const FFT_N:  u32 = 256u;
const LOG2_N: u32 = 8u;
const PI: f32 = 3.14159265358979323846;

// Reverse the 8 least-significant bits of x.
fn bit_reverse_8(x: u32) -> u32 {
    var v = x & 0xffu;
    v = ((v >> 1u) & 0x55u) | ((v & 0x55u) << 1u);
    v = ((v >> 2u) & 0x33u) | ((v & 0x33u) << 2u);
    v = ((v >> 4u) & 0x0fu) | ((v & 0x0fu) << 4u);
    return v;
}

@compute @workgroup_size(256)
fn main(
    @builtin(workgroup_id)        wgid: vec3<u32>,
    @builtin(local_invocation_id) lid:  vec3<u32>,
) {
    let sym = wgid.x;
    let t   = lid.x;
    if sym >= params.n_symbols { return; }

    // Step 1: all 256 threads load data with bit-reversal permutation.
    let base = sym * FFT_N * 2u;
    let br   = bit_reverse_8(t);
    wg_re[t] = in_data[base + br * 2u];
    wg_im[t] = in_data[base + br * 2u + 1u];
    workgroupBarrier();

    // Step 2: 8 butterfly stages.
    // At each stage threads 0–127 each handle one butterfly; all 256 threads
    // must cross the workgroupBarrier() that separates stages.
    for (var s = 0u; s < LOG2_N; s++) {
        let half_m = 1u << s;
        let m      = half_m << 1u;
        if t < 128u {
            let group = t / half_m;
            let pos   = t % half_m;
            // Forward FFT: W = exp(-j 2π pos/m);  IFFT: W = exp(+j 2π pos/m).
            let angle = select(
                 2.0 * PI * f32(pos) / f32(m),  // IFFT
                -2.0 * PI * f32(pos) / f32(m),  // FFT
                params.forward == 1u,
            );
            let wr = cos(angle);
            let wi = sin(angle);
            let u  = group * m + pos;
            let v  = u + half_m;
            let ur = wg_re[u]; let ui = wg_im[u];
            let vr = wg_re[v]; let vi = wg_im[v];
            let tvr = wr * vr - wi * vi;
            let tvi = wr * vi + wi * vr;
            wg_re[u] = ur + tvr;
            wg_im[u] = ui + tvi;
            wg_re[v] = ur - tvr;
            wg_im[v] = ui - tvi;
        }
        workgroupBarrier();
    }

    // Step 3: all 256 threads store output; IFFT applies 1/N normalisation.
    let scale = select(1.0f / f32(FFT_N), 1.0f, params.forward == 1u);
    out_data[base + t * 2u]      = wg_re[t] * scale;
    out_data[base + t * 2u + 1u] = wg_im[t] * scale;
}
