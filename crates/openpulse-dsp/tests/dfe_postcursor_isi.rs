//! The LMS decision-feedback section must CANCEL post-cursor ISI, not amplify it.
//!
//! `LmsEqualizer::filter()` subtracts the DFE output (`fwd − dfe`), so the DFE tap update must carry the
//! opposite sign to the forward taps (`w_dfe −= μ·e·conj(d)`). With the wrong sign the feedback section
//! is anti-adaptive: on a pure post-cursor ISI channel — exactly what a DFE cancels perfectly — it drives
//! steady-state MSE far ABOVE the forward-only result until the tap-energy guard clamps it. That defect
//! is invisible to a clean (identity-channel) loopback (zero error ⇒ zero update) and it silently poisoned
//! every DFE-enabled profile; this test pins the sign.

use openpulse_dsp::equalizer::LmsEqualizer;

const INV_SQRT2: f32 = std::f32::consts::FRAC_1_SQRT_2;

fn qpsk(a: bool, b: bool) -> (f32, f32) {
    (
        if a { INV_SQRT2 } else { -INV_SQRT2 },
        if b { INV_SQRT2 } else { -INV_SQRT2 },
    )
}

fn slice(i: f32, q: f32) -> (f32, f32) {
    (
        if i >= 0.0 { INV_SQRT2 } else { -INV_SQRT2 },
        if q >= 0.0 { INV_SQRT2 } else { -INV_SQRT2 },
    )
}

/// Steady-state (back-half) MSE of the equalized symbols vs the true symbols on a post-cursor ISI channel
/// `h = [1.0, 0.5]` (y[k] = x[k] + 0.5·x[k−1]).
fn residual_mse(dfe_len: usize) -> f32 {
    let n = 600usize;
    let mut seed = 0x1234_5678u64;
    let mut sym: Vec<(f32, f32)> = Vec::with_capacity(n);
    for _ in 0..n {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let a = (seed >> 33) & 1 == 1;
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let b = (seed >> 33) & 1 == 1;
        sym.push(qpsk(a, b));
    }
    let mut rx: Vec<(f32, f32)> = Vec::with_capacity(n);
    for k in 0..n {
        let (pi, pq) = if k > 0 { sym[k - 1] } else { (0.0, 0.0) };
        rx.push((sym[k].0 + 0.5 * pi, sym[k].1 + 0.5 * pq));
    }
    let (i_syms, q_syms): (Vec<f32>, Vec<f32>) = rx.iter().copied().unzip();
    let train = 16usize;
    let (ti, tq): (Vec<f32>, Vec<f32>) = sym[..train].iter().copied().unzip();

    let mut eq = LmsEqualizer::new(9, dfe_len, 0.02);
    let (i_eq, q_eq) = eq.process_frame(&i_syms, &q_syms, &ti, &tq, slice);

    let start = n / 2;
    let mut mse = 0.0f32;
    for k in start..n {
        mse += (i_eq[k] - sym[k].0).powi(2) + (q_eq[k] - sym[k].1).powi(2);
    }
    mse / (n - start) as f32
}

#[test]
fn dfe_cancels_postcursor_isi_it_does_not_amplify() {
    let mse0 = residual_mse(0);
    let mse1 = residual_mse(1);
    let mse2 = residual_mse(2);
    // A correctly-signed DFE cancels the post-cursor tap: adding feedback taps must not increase the
    // steady-state MSE. (With the sign inverted, mse1/mse2 ran to ~16 / ~29 vs mse0 ≈ 0.0001.)
    assert!(
        mse1 <= mse0 + 1e-3 && mse2 <= mse0 + 1e-3,
        "DFE amplified post-cursor ISI (mse0={mse0:.5}, mse1={mse1:.5}, mse2={mse2:.5}) — feedback update sign is wrong"
    );
}
