// Parallel timing offset search kernel.
// Each workitem evaluates one timing offset (0..n_offsets = samples_per_sym).
// off_idx = global_invocation_id.x
//
// For each offset, demodulate preamble_syms symbols and correlate the I channel
// against the expected preamble pattern. Writes correlation energy to out_energy.
// CPU picks the max-energy offset as the symbol timing.

struct TimingParams {
    n_offsets:       u32,
    samples_per_sym: u32,
    preamble_syms:   u32,
    pad0:            u32,
    fc:              f32,
    sample_rate:     f32,
    pad1:            f32,
    pad2:            f32,
};

@group(0) @binding(0) var<storage, read>       in_samples:        array<f32>;
@group(0) @binding(1) var<storage, read>       expected_preamble: array<f32>;
@group(0) @binding(2) var<storage, read_write> out_energy:        array<f32>;
@group(0) @binding(3) var<uniform>             params:            TimingParams;

const TWO_PI: f32 = 6.283185307179586;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let off_idx = gid.x;
    if (off_idx >= params.n_offsets) {
        return;
    }

    let n = params.samples_per_sym;
    let p = params.preamble_syms;

    // Need enough samples for offset + p symbols.
    if (arrayLength(&in_samples) < off_idx + p * n) {
        out_energy[off_idx] = 0.0f;
        return;
    }

    var total_energy = 0.0f;

    for (var sym_idx = 0u; sym_idx < p; sym_idx++) {
        let sym_start = sym_idx * n;
        var i_sum     = 0.0f;
        var norm      = 0.0f;

        for (var k = 0u; k < n; k++) {
            let sample_idx = off_idx + sym_start + k;
            let sample     = in_samples[sample_idx];

            let window = 0.5 * (1.0 - cos(TWO_PI * f32(k) / f32(n)));

            // Carrier phase uses the absolute sample index.
            let t  = f32(sample_idx) / params.sample_rate;
            let ci = cos(TWO_PI * params.fc * t);

            i_sum += sample * ci * window * 2.0;
            norm  += window * window;
        }

        if (norm > 1e-9f) {
            let i_val       = i_sum / norm;
            total_energy   += i_val * expected_preamble[sym_idx];
        }
    }

    out_energy[off_idx] = total_energy;
}
