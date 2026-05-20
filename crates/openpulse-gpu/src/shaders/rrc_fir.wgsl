// FIR convolution kernel for matched RRC filtering.
//
// Computes y[n] = Σ_{k=0}^{tap_limit-1} coeffs[k] * in_samples[n-k]
// where tap_limit = min(n_taps, n+1) implements implicit zero-padding
// for n < k (causal boundary condition).  One thread per output sample.

struct FirParams {
    n_samples: u32,
    n_taps:    u32,
}

@group(0) @binding(0) var<storage, read>       in_samples: array<f32>;
@group(0) @binding(1) var<storage, read>       coeffs:     array<f32>;
@group(0) @binding(2) var<uniform>             params:     FirParams;
@group(0) @binding(3) var<storage, read_write> out:        array<f32>;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let n = gid.x;
    if n >= params.n_samples { return; }
    var acc: f32 = 0.0;
    // tap_limit guards n-k >= 0 so the subtraction never underflows (u32).
    let tap_limit = min(params.n_taps, n + 1u);
    for (var k = 0u; k < tap_limit; k++) {
        acc += coeffs[k] * in_samples[n - k];
    }
    out[n] = acc;
}
