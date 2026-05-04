// BPSK IQ demodulation kernel.
// Each workitem demodulates one symbol.
// sym_idx = global_invocation_id.x
//
// Matches the CPU demodulate_iq(): Hann-windowed matched filter with
// factor-of-2 carrier normalisation. `params.offset` is the timing offset
// applied before this slice of samples was captured — used only for carrier
// phase calculation, not for sample indexing (in_samples is already sliced).

struct BpskDemodParams {
    n_syms:          u32,
    samples_per_sym: u32,
    offset:          u32,
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
        let local_idx = sym_start + k;
        if (local_idx >= arrayLength(&in_samples)) {
            break;
        }
        let sample = in_samples[local_idx];

        // Matched filter: same Hann window as the modulator.
        let window = 0.5 * (1.0 - cos(TWO_PI * f32(k) / f32(n)));

        // Global sample index preserving the original timing offset for
        // correct carrier phase.
        let global_n = f32(params.offset + local_idx);
        let t        = global_n / params.sample_rate;
        let ci       =  cos(TWO_PI * params.fc * t);
        let cq       = -sin(TWO_PI * params.fc * t);

        // Factor-of-2 compensates for the ½ in the carrier product.
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
