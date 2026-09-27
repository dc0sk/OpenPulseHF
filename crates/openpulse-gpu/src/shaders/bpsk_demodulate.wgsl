// BPSK IQ demodulation kernel.
// Each workitem demodulates one symbol.
// sym_idx = global_invocation_id.x
//
// Matches the CPU demodulate_iq_at(): half-Hann (w_tail) matched filter with
// factor-of-2 carrier normalisation. `params.offset` is the SIGNED timing offset into the
// whole buffer (#1438 PR2): symbol k integrates samples offset + k*n .., samples before
// index 0 read as zero, and the carrier uses the absolute index.

struct BpskDemodParams {
    n_syms:          u32,
    samples_per_sym: u32,
    offset:          i32,
    pad0:            u32,
    fc:              f32,
    sample_rate:     f32,
    pad1:            f32,
    pad2:            f32,
};

@group(0) @binding(0) var<storage, read>       in_samples: array<f32>;
@group(0) @binding(1) var<storage, read_write> out_i:      array<f32>;
@group(0) @binding(2) var<storage, read_write> out_q:      array<f32>;
@group(0) @binding(3) var<uniform>             params:     BpskDemodParams;

const TWO_PI: f32 = 6.283185307179586;
const PI:     f32 = 3.141592653589793;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let sym_idx = gid.x;
    if (sym_idx >= params.n_syms) {
        return;
    }

    let n         = params.samples_per_sym;
    let sym_start = sym_idx * n;
    var i_sum     = 0.0f;
    var q_sum     = 0.0f;
    var norm      = 0.0f;

    for (var k = 0u; k < n; k++) {
        let idx = params.offset + i32(sym_start + k);
        if (idx >= i32(arrayLength(&in_samples))) {
            break;
        }
        var sample = 0.0f;
        if (idx >= 0) {
            sample = in_samples[u32(idx)];
        }

        // Matched filter: decreasing half-Hann (w_tail), matching the overlapping
        // crossfade modulator.  w_tail = 0.5*(1+cos(π*k/n)) → 1 at k=0, 0 at k=n.
        let window = 0.5 * (1.0 + cos(PI * f32(k) / f32(n)));

        let global_n = f32(idx);
        let t        = global_n / params.sample_rate;
        let ci       =  cos(TWO_PI * params.fc * t);
        let cq       = -sin(TWO_PI * params.fc * t);

        i_sum += sample * ci * window * 2.0;
        q_sum += sample * cq * window * 2.0;
        norm  += window * window;
    }

    if (norm > 1e-9f) {
        out_i[sym_idx] = i_sum / norm;
        out_q[sym_idx] = q_sum / norm;
    } else {
        out_i[sym_idx] = 0.0f;
        out_q[sym_idx] = 0.0f;
    }
}

