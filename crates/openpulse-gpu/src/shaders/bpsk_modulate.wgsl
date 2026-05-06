// BPSK modulation kernel — overlapping half-Hann crossfade.
// Each workitem produces one output sample.
// sample_idx = global_invocation_id.x
// sym_idx    = sample_idx / samples_per_sym
// i          = sample_idx % samples_per_sym
//
// out[sample_idx] = (a_curr * w_tail + a_next * w_head) * carrier
// where w_tail = 0.5*(1+cos(π*i/n))  →  1 at i=0, 0 at i=n
//       w_head = 1 - w_tail           →  0 at i=0, 1 at i=n
// Adjacent same-phase symbols give constant amplitude; phase transitions
// dip to zero at the midpoint, suppressing inter-symbol sidelobes.

struct BpskModParams {
    n_syms:          u32,
    samples_per_sym: u32,
    fc:              f32,
    sample_rate:     f32,
};

@group(0) @binding(0) var<storage, read>       symbols:     array<u32>;
@group(0) @binding(1) var<storage, read_write> out_samples: array<f32>;
@group(0) @binding(2) var<uniform>             params:      BpskModParams;

const TWO_PI: f32 = 6.283185307179586;
const PI:     f32 = 3.141592653589793;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let sample_idx = gid.x;
    let total = params.n_syms * params.samples_per_sym;
    if (sample_idx >= total) {
        return;
    }

    let sym_idx = sample_idx / params.samples_per_sym;
    let i       = sample_idx % params.samples_per_sym;

    let a_curr = select(1.0f, -1.0f, symbols[sym_idx] == 1u);
    // Fade to silence after the last symbol.
    let a_next = select(
        select(1.0f, -1.0f, symbols[sym_idx + 1u] == 1u),
        0.0f,
        sym_idx + 1u >= params.n_syms,
    );

    let n_f    = f32(params.samples_per_sym);
    let w_tail = 0.5 * (1.0 + cos(PI * f32(i) / n_f));
    let w_head = 1.0 - w_tail;

    let t       = f32(sample_idx) / params.sample_rate;
    let carrier = cos(TWO_PI * params.fc * t);

    out_samples[sample_idx] = (a_curr * w_tail + a_next * w_head) * carrier;
}
