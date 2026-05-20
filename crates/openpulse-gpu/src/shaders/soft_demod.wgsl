// Max-log-MAP soft LLR kernel for arbitrary QAM/PSK constellations.
//
// One workgroup per received symbol; 64 threads per workgroup cooperate over
// constellation points. Thread p computes dist²(y, constel[p]) and stores it
// in workgroup shared memory. Thread 0 then does the per-bit min-reduction.
//
// Buffer layout
//   symbols:       [i0, q0, i1, q1, ...]   (n_symbols × 2 f32)
//   constellation: [ci0,cq0,ci1,cq1, ...]  (n_points  × 2 f32)
//   bit_table:     bit_table[p] = bit pattern of constellation point p
//                  (for Gray-coded: bit_table[p] = p)
//   params:        uniform n_symbols, n_points, bits_per_sym
//   out_llr:       [b0,b1,...,b_{K-1}, b0,b1,...] per symbol, K = bits_per_sym
//                  positive LLR → bit=0 is more likely

struct SoftDemodParams {
    n_symbols:    u32,
    n_points:     u32,
    bits_per_sym: u32,
    pad:          u32,
};

@group(0) @binding(0) var<storage, read>       symbols:      array<f32>;
@group(0) @binding(1) var<storage, read>       constellation: array<f32>;
@group(0) @binding(2) var<storage, read>       bit_table:    array<u32>;
@group(0) @binding(3) var<uniform>             params:       SoftDemodParams;
@group(0) @binding(4) var<storage, read_write> out_llr:      array<f32>;

// Workgroup-shared squared distances — one slot per constellation point (≤ 64).
var<workgroup> wg_dist2: array<f32, 64>;

@compute @workgroup_size(64)
fn main(
    @builtin(workgroup_id)       wgid: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
) {
    let sym_idx = wgid.x;
    let p       = lid.x;

    if sym_idx >= params.n_symbols {
        wg_dist2[p] = 1e9;
        workgroupBarrier();
        return;
    }

    let yi = symbols[sym_idx * 2u];
    let yq = symbols[sym_idx * 2u + 1u];

    // Each thread computes squared Euclidean distance for one constellation point.
    if p < params.n_points {
        let ci = constellation[p * 2u];
        let cq = constellation[p * 2u + 1u];
        let di = yi - ci;
        let dq = yq - cq;
        wg_dist2[p] = di * di + dq * dq;
    } else {
        // Idle thread: write sentinel that never wins a min-reduction.
        wg_dist2[p] = 1e9;
    }

    workgroupBarrier();

    // Thread 0 performs the per-bit min-reduction and writes LLRs.
    if p == 0u {
        let llr_base = sym_idx * params.bits_per_sym;
        for (var b = 0u; b < params.bits_per_sym; b++) {
            var min0 = 1e9f;
            var min1 = 1e9f;
            for (var pt = 0u; pt < params.n_points; pt++) {
                let d = wg_dist2[pt];
                if ((bit_table[pt] >> b) & 1u) == 0u {
                    if d < min0 { min0 = d; }
                } else {
                    if d < min1 { min1 = d; }
                }
            }
            // Positive LLR → bit=0 more likely (matches CPU sign convention).
            out_llr[llr_base + b] = min1 - min0;
        }
    }
}
