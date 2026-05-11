//! LMS adaptive equalizer with optional Decision Feedback (DFE).
//!
//! Operates at symbol rate on complex I/Q pairs.  Two modes:
//!
//! - **Supervised** (training): caller supplies known reference symbols;
//!   error is `desired - output`.
//! - **Decision-directed**: equalizer makes a hard decision, uses that as the
//!   desired symbol.  Switches after the training window.
//!
//! ## DFE
//!
//! Set `dfe_len = 0` for a pure forward LMS equalizer.  Non-zero `dfe_len`
//! enables the feedback section: past hard decisions are convolved with the DFE
//! taps and subtracted from the forward output before the decision is made.

use std::collections::VecDeque;

/// LMS adaptive equalizer with optional DFE feedback section.
pub struct LmsEqualizer {
    fwd_len: usize,
    dfe_len: usize,
    mu: f32,
    // Forward tap weights (complex: (re, im) per tap)
    w_fwd_re: Vec<f32>,
    w_fwd_im: Vec<f32>,
    // DFE tap weights (complex)
    w_dfe_re: Vec<f32>,
    w_dfe_im: Vec<f32>,
    // Input delay line
    buf_re: VecDeque<f32>,
    buf_im: VecDeque<f32>,
    // Decision delay line for DFE
    dec_re: VecDeque<f32>,
    dec_im: VecDeque<f32>,
}

impl LmsEqualizer {
    /// Create a new equalizer.
    ///
    /// - `fwd_len`: number of forward (causal) taps; must be ≥ 1
    /// - `dfe_len`: DFE feedback taps; 0 = pure forward LMS
    /// - `mu`: LMS step size (typical: 0.01–0.05 for HF)
    pub fn new(fwd_len: usize, dfe_len: usize, mu: f32) -> Self {
        assert!(fwd_len >= 1, "fwd_len must be >= 1");
        let mut w_fwd_re = vec![0.0f32; fwd_len];
        // Initialise the first (causal) tap to 1.0 so the filter starts as
        // a pass-through with zero group delay.  Remaining taps learn the
        // ISI cancellation terms during the training phase.
        w_fwd_re[0] = 1.0;
        Self {
            fwd_len,
            dfe_len,
            mu,
            w_fwd_re,
            w_fwd_im: vec![0.0f32; fwd_len],
            w_dfe_re: vec![0.0f32; dfe_len],
            w_dfe_im: vec![0.0f32; dfe_len],
            buf_re: VecDeque::from(vec![0.0f32; fwd_len]),
            buf_im: VecDeque::from(vec![0.0f32; fwd_len]),
            dec_re: VecDeque::from(vec![0.0f32; dfe_len.max(1)]),
            dec_im: VecDeque::from(vec![0.0f32; dfe_len.max(1)]),
        }
    }

    /// Filter one symbol and return `(y_re, y_im)` without updating weights.
    fn filter(&self) -> (f32, f32) {
        let fwd_re: f32 = self
            .buf_re
            .iter()
            .zip(&self.w_fwd_re)
            .zip(self.buf_im.iter().zip(&self.w_fwd_im))
            .map(|((x_re, w_re), (x_im, w_im))| x_re * w_re - x_im * w_im)
            .sum();
        let fwd_im: f32 = self
            .buf_re
            .iter()
            .zip(&self.w_fwd_re)
            .zip(self.buf_im.iter().zip(&self.w_fwd_im))
            .map(|((x_re, w_re), (x_im, w_im))| x_re * w_im + x_im * w_re)
            .sum();

        if self.dfe_len == 0 {
            return (fwd_re, fwd_im);
        }

        let dfe_re: f32 = self
            .dec_re
            .iter()
            .zip(&self.w_dfe_re)
            .zip(self.dec_im.iter().zip(&self.w_dfe_im))
            .map(|((d_re, w_re), (d_im, w_im))| d_re * w_re - d_im * w_im)
            .sum();
        let dfe_im: f32 = self
            .dec_re
            .iter()
            .zip(&self.w_dfe_re)
            .zip(self.dec_im.iter().zip(&self.w_dfe_im))
            .map(|((d_re, w_re), (d_im, w_im))| d_re * w_im + d_im * w_re)
            .sum();

        (fwd_re - dfe_re, fwd_im - dfe_im)
    }

    /// LMS weight update given error `(e_re, e_im)`.
    fn lms_update(&mut self, e_re: f32, e_im: f32) {
        for (i, (w_re, w_im)) in self
            .w_fwd_re
            .iter_mut()
            .zip(self.w_fwd_im.iter_mut())
            .enumerate()
        {
            let x_re = self.buf_re[i];
            let x_im = self.buf_im[i];
            // w += μ * e * conj(x)
            *w_re += self.mu * (e_re * x_re + e_im * x_im);
            *w_im += self.mu * (e_im * x_re - e_re * x_im);
        }
        for (i, (w_re, w_im)) in self
            .w_dfe_re
            .iter_mut()
            .zip(self.w_dfe_im.iter_mut())
            .enumerate()
        {
            let d_re = self.dec_re[i];
            let d_im = self.dec_im[i];
            *w_re += self.mu * (e_re * d_re + e_im * d_im);
            *w_im += self.mu * (e_im * d_re - e_re * d_im);
        }
    }

    /// Push a new input sample into the delay lines.
    fn push_input(&mut self, in_re: f32, in_im: f32) {
        self.buf_re.push_front(in_re);
        self.buf_re.pop_back();
        self.buf_im.push_front(in_im);
        self.buf_im.pop_back();
    }

    /// Push a decision into the DFE delay line.
    fn push_decision(&mut self, d_re: f32, d_im: f32) {
        if self.dfe_len > 0 {
            self.dec_re.push_front(d_re);
            self.dec_re.pop_back();
            self.dec_im.push_front(d_im);
            self.dec_im.pop_back();
        }
    }

    /// Process one symbol with a **known** training symbol (supervised update).
    ///
    /// Returns the equalizer output `(y_re, y_im)` before the update.
    pub fn train(
        &mut self,
        in_re: f32,
        in_im: f32,
        desired_re: f32,
        desired_im: f32,
    ) -> (f32, f32) {
        self.push_input(in_re, in_im);
        let (y_re, y_im) = self.filter();
        let e_re = desired_re - y_re;
        let e_im = desired_im - y_im;
        self.lms_update(e_re, e_im);
        self.push_decision(desired_re, desired_im);
        (y_re, y_im)
    }

    /// Process one symbol in **decision-directed** mode.
    ///
    /// `decide` maps `(y_re, y_im)` to the nearest constellation point.
    /// Returns `(y_re, y_im)` — the raw equalizer output (before decision).
    pub fn equalize(
        &mut self,
        in_re: f32,
        in_im: f32,
        decide: impl Fn(f32, f32) -> (f32, f32),
    ) -> (f32, f32) {
        self.push_input(in_re, in_im);
        let (y_re, y_im) = self.filter();
        let (d_re, d_im) = decide(y_re, y_im);
        let e_re = d_re - y_re;
        let e_im = d_im - y_im;
        self.lms_update(e_re, e_im);
        self.push_decision(d_re, d_im);
        (y_re, y_im)
    }

    /// Apply the equalizer to a full frame.
    ///
    /// The first `training_re.len()` symbols are processed in supervised mode
    /// using the provided training sequence; remaining symbols are
    /// decision-directed.
    ///
    /// Returns `(out_re, out_im)` for the entire input.
    pub fn process_frame(
        &mut self,
        in_re: &[f32],
        in_im: &[f32],
        training_re: &[f32],
        training_im: &[f32],
        decide: impl Fn(f32, f32) -> (f32, f32),
    ) -> (Vec<f32>, Vec<f32>) {
        let n = in_re.len().min(in_im.len());
        let train_n = training_re.len().min(training_im.len()).min(n);
        let mut out_re = Vec::with_capacity(n);
        let mut out_im = Vec::with_capacity(n);

        for k in 0..n {
            if k < train_n {
                let (y_re, y_im) = self.train(in_re[k], in_im[k], training_re[k], training_im[k]);
                out_re.push(y_re);
                out_im.push(y_im);
            } else {
                let (y_re, y_im) = self.equalize(in_re[k], in_im[k], &decide);
                out_re.push(y_re);
                out_im.push(y_im);
            }
        }

        (out_re, out_im)
    }

    /// Reset all internal state (delay lines and tap weights).
    ///
    /// Centre tap is re-initialised to 1.0 to preserve the pass-through
    /// behaviour at the start of each new frame.
    pub fn reset(&mut self) {
        self.w_fwd_re.fill(0.0);
        self.w_fwd_im.fill(0.0);
        self.w_fwd_re[0] = 1.0;
        self.w_dfe_re.fill(0.0);
        self.w_dfe_im.fill(0.0);
        self.buf_re.iter_mut().for_each(|x| *x = 0.0);
        self.buf_im.iter_mut().for_each(|x| *x = 0.0);
        self.dec_re.iter_mut().for_each(|x| *x = 0.0);
        self.dec_im.iter_mut().for_each(|x| *x = 0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bpsk_decide(i: f32, _q: f32) -> (f32, f32) {
        (if i >= 0.0 { 1.0 } else { -1.0 }, 0.0)
    }

    #[test]
    fn passthrough_with_no_channel() {
        let mut eq = LmsEqualizer::new(5, 0, 0.01);
        let syms_i: Vec<f32> = (0..64)
            .map(|k| if k % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        let syms_q = vec![0.0f32; 64];
        let training_i = syms_i[..32].to_vec();
        let training_q = syms_q[..32].to_vec();

        let (out_i, _out_q) =
            eq.process_frame(&syms_i, &syms_q, &training_i, &training_q, bpsk_decide);

        // After training, decisions should match the input exactly.
        for (k, (&y, &x)) in out_i[32..].iter().zip(syms_i[32..].iter()).enumerate() {
            let d = bpsk_decide(y, 0.0).0;
            assert_eq!(
                d, x,
                "decision mismatch at symbol {k}: got {d}, expected {x}"
            );
        }
    }

    #[test]
    fn converges_on_flat_fade_plus_phase_rotation() {
        // Simulate a flat-fading channel: multiply all symbols by a fixed complex gain.
        let gain_re = 0.6f32;
        let gain_im = 0.4f32; // ~36° rotation
        let syms_i: Vec<f32> = (0..128)
            .map(|k| if k % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        let syms_q = vec![0.0f32; 128];

        // Apply channel.
        let ch_i: Vec<f32> = syms_i
            .iter()
            .zip(&syms_q)
            .map(|(&i, &q)| i * gain_re - q * gain_im)
            .collect();
        let ch_q: Vec<f32> = syms_i
            .iter()
            .zip(&syms_q)
            .map(|(&i, &q)| i * gain_im + q * gain_re)
            .collect();

        let mut eq = LmsEqualizer::new(5, 0, 0.02);
        let train_n = 32;
        let (out_i, _out_q) = eq.process_frame(
            &ch_i,
            &ch_q,
            &syms_i[..train_n],
            &syms_q[..train_n],
            bpsk_decide,
        );

        // After training, at least 80% of decision-directed symbols must be correct.
        let n_correct = out_i[train_n..]
            .iter()
            .zip(&syms_i[train_n..])
            .filter(|(&y, &x)| bpsk_decide(y, 0.0).0 == x)
            .count();
        let total = out_i.len() - train_n;
        assert!(
            n_correct >= (total * 8) / 10,
            "only {n_correct}/{total} correct after training"
        );
    }

    #[test]
    fn dfe_variant_compiles_and_runs() {
        let mut eq = LmsEqualizer::new(7, 2, 0.01);
        let syms_i: Vec<f32> = (0..64)
            .map(|k| if k % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        let syms_q = vec![0.0f32; 64];
        let train_i = syms_i[..32].to_vec();
        let train_q = syms_q[..32].to_vec();
        let (out_i, _) = eq.process_frame(&syms_i, &syms_q, &train_i, &train_q, bpsk_decide);
        assert_eq!(out_i.len(), 64);
    }

    #[test]
    fn reset_restores_initial_state() {
        let mut eq = LmsEqualizer::new(5, 0, 0.02);
        // Run some data through to perturb the weights.
        let syms_i: Vec<f32> = vec![1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0];
        let syms_q = vec![0.0f32; 8];
        eq.process_frame(&syms_i, &syms_q, &syms_i, &syms_q, bpsk_decide);

        eq.reset();
        // After reset, w[0] = 1.0 (causal pass-through tap), all others = 0.0.
        assert!((eq.w_fwd_re[0] - 1.0).abs() < 1e-6);
        let off_first: f32 = eq
            .w_fwd_re
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != 0)
            .map(|(_, &w)| w.abs())
            .sum();
        assert!(
            off_first < 1e-6,
            "non-first taps should be zero after reset"
        );
    }

    #[test]
    fn train_reduces_error() {
        let mut eq = LmsEqualizer::new(5, 0, 0.1);
        // Apply a fixed gain of 0.5 to the input — train to invert it.
        let desired = vec![1.0f32; 32];
        let input = vec![0.5f32; 32]; // channel gain = 0.5
        let zeros = vec![0.0f32; 32];

        let (out, _) = eq.process_frame(&input, &zeros, &desired, &zeros, |i, _| {
            (if i >= 0.0 { 1.0 } else { -1.0 }, 0.0)
        });

        // First sample: output ≈ 0.5 * 1.0 (centre tap only).
        // Last sample: should be converging toward 1.0.
        let first_err = (1.0 - out[0]).abs();
        let last_err = (1.0 - out[31]).abs();
        assert!(
            last_err < first_err,
            "training should reduce error: first={first_err:.4} last={last_err:.4}"
        );
    }
}
