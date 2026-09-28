// Parallel timing offset search kernel.
// Each workitem evaluates one timing offset, offset_base + off_idx, where off_idx =
// global_invocation_id.x < n_offsets. offset_base is negative for the widened search
// (#1438 PR2): samples before index 0 read as zero, exactly as the CPU demodulate_iq_at.
//
// For each offset, demodulate preamble_syms symbols and correlate the I channel
// against the expected preamble pattern. Writes correlation energy to out_energy, or -1
// where the preamble span runs past the buffer. The CPU picks both locks from this array
// with the plugin's shared first-max picker.

struct TimingParams {
    n_offsets:       u32,
    samples_per_sym: u32,
    preamble_syms:   u32,
    offset_base:     i32,
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
const PI:     f32 = 3.141592653589793;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let off_idx = gid.x;
    if (off_idx >= params.n_offsets) {
        return;
    }

    let n = params.samples_per_sym;
    let p = params.preamble_syms;
    let off = i32(off_idx) + params.offset_base;

    // Need enough samples for offset + p symbols; the CPU search stops at the first offset that
    // fails this, and the host truncates the array at the first -1.
    if (i32(arrayLength(&in_samples)) < off + i32(p * n)) {
        out_energy[off_idx] = -1.0f;
        return;
    }

    // Accumulate the correlation as a COMPLEX sum, then score its magnitude.
    //
    // This kernel used to sum only the in-phase product and score the signed
    // total. That is the algorithm the CPU path was fixed away from, and its
    // comment says why: |Sum I*e| handles the 180-degree polarity ambiguity but
    // COLLAPSES at a ~90-degree carrier phase, where the preamble energy sits in
    // Q. The CPU uses (Sum I*e)^2 + (Sum Q*e)^2, which is carrier-phase
    // invariant. The fix reached the CPU and not this sibling, so the GPU and
    // CPU receivers disagreed about where a frame starts whenever a carrier
    // offset rotated the preamble out of I (#1080).
    //
    // Measured before this change, BPSK250 through AWGN: identical decode
    // outcomes on 48 on-frequency frames, and disagreement on 21 of 30
    // off-frequency ones -- in both directions, which is the signature of two
    // searches landing on different offsets rather than one being noisier.
    var corr_re = 0.0f;
    var corr_im = 0.0f;

    for (var sym_idx = 0u; sym_idx < p; sym_idx++) {
        let sym_start = sym_idx * n;
        var i_sum     = 0.0f;
        var q_sum     = 0.0f;
        var norm      = 0.0f;

        for (var k = 0u; k < n; k++) {
            let sample_idx = off + i32(sym_start + k);
            var sample     = 0.0f;
            if (sample_idx >= 0) {
                sample = in_samples[u32(sample_idx)];
            }

            let window = 0.5 * (1.0 + cos(PI * f32(k) / f32(n)));

            // Carrier phase uses the absolute sample index.
            let t  = f32(sample_idx) / params.sample_rate;
            let ci = cos(TWO_PI * params.fc * t);
            let cq = -sin(TWO_PI * params.fc * t);

            i_sum += sample * ci * window * 2.0;
            q_sum += sample * cq * window * 2.0;
            norm  += window * window;
        }

        if (norm > 1e-9f) {
            let e = expected_preamble[sym_idx];
            corr_re += (i_sum / norm) * e;
            corr_im += (q_sum / norm) * e;
        }
    }

    out_energy[off_idx] = corr_re * corr_re + corr_im * corr_im;
}
