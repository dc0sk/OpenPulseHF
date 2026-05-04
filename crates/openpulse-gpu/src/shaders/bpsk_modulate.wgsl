// BPSK modulation kernel.
// Each workitem produces one output sample.
// sample_idx = global_invocation_id.x
// sym_idx    = sample_idx / samples_per_sym
// i          = sample_idx % samples_per_sym
//
// out[sample_idx] = amplitude × hann_envelope × carrier
// where amplitude = +1 if symbols[sym_idx]==0, −1 if symbols[sym_idx]==1

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

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let sample_idx = gid.x;
    let total = params.n_syms * params.samples_per_sym;
    if (sample_idx >= total) {
        return;
    }

    let sym_idx = sample_idx / params.samples_per_sym;
    let i       = sample_idx % params.samples_per_sym;

    // symbols[sym_idx] == 1u → phase_neg → amplitude = −1.0
    let amplitude = select(1.0f, -1.0f, symbols[sym_idx] == 1u);

    // Raised-cosine (Hann) amplitude envelope across the symbol period.
    let n_f      = f32(params.samples_per_sym);
    let envelope = 0.5 * (1.0 - cos(TWO_PI * f32(i) / n_f));

    // Carrier phase based on absolute sample position.
    let t       = f32(sample_idx) / params.sample_rate;
    let carrier = cos(TWO_PI * params.fc * t);

    out_samples[sample_idx] = amplitude * envelope * carrier;
}
