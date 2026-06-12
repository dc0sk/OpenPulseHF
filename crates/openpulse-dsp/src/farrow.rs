//! Interpolating symbol-timing recovery: cubic (Farrow) interpolator driven by
//! a complex Gardner detector with a proportional-plus-integral loop.
//!
//! [`crate::timing::GardnerDetector`] cannot actually adjust the sampling
//! instant: its `mu` is clamped to ±0.49 and the strobe interval
//! `round(sps + mu)` therefore always equals `sps` — a fixed stride from the
//! initial preamble lock.  With two free-running sound-card clocks a 50 ppm
//! sample-rate offset drifts the ISI-free sampling point by one full sample
//! every ~2.5 s at 8 kHz; long frames slip into heavy ISI with no mechanism to
//! recover.  This loop tracks both a fractional phase (proportional term) and
//! the actual samples-per-symbol period (integral term), interpolating sample
//! values at arbitrary fractional positions.
//!
//! The Gardner error is computed on the COMPLEX baseband
//! (`e = Re{z_mid · conj(z_prev − z_cur)}`), which is invariant to a common
//! carrier-phase rotation — unlike the I-channel-only error previously fed to
//! `GardnerDetector`, which is data-dependent for quadrature constellations.

/// Cubic-interpolating timing recovery loop (complex Gardner + PI filter).
pub struct FarrowTimingLoop {
    /// Nominal samples per symbol.
    sps: f32,
    /// Proportional gain (fractional-phase correction per normalised error).
    kp: f32,
    /// Integral gain (period correction per normalised error).
    ki: f32,
    /// Maximum fractional deviation of the tracked period from nominal.
    max_period_dev: f32,
}

impl FarrowTimingLoop {
    /// Create a loop for `sps` samples per symbol (must be ≥ 4 for the cubic
    /// interpolator to have room around the midpoint).
    ///
    /// Defaults: `kp = 0.005`, `ki = 0.0001`, period clamp ±500 ppm.
    ///
    /// The loop only needs to track a sample-rate offset, which is
    /// essentially DC — so its bandwidth sits far below HF fading dynamics
    /// (0.5–2 Hz Doppler) to keep multipath/fade-driven timing jitter from
    /// walking off the preamble lock that the equalizer was trained on.
    /// Steady-state phase lag at a worst-case 150 ppm × 8 sps is
    /// ~0.3 samples (≈ 4 % of the symbol period) carried by kp alone.
    pub fn new(sps: usize) -> Self {
        assert!(sps >= 4, "FarrowTimingLoop requires sps >= 4 (got {sps})");
        Self {
            sps: sps as f32,
            kp: 0.005,
            ki: 0.0001,
            max_period_dev: 500e-6,
        }
    }

    /// Override the loop gains.
    pub fn with_gains(mut self, kp: f32, ki: f32) -> Self {
        self.kp = kp;
        self.ki = ki;
        self
    }

    /// Cubic Lagrange interpolation of `x` at fractional position `pos`.
    ///
    /// Uses the four samples around `pos`; positions too close to either edge
    /// fall back to linear/nearest available.
    fn interp(x: &[f32], pos: f64) -> f32 {
        let base = pos.floor() as isize;
        let t = (pos - base as f64) as f32;
        let n = x.len() as isize;
        if base < 1 || base + 2 >= n {
            // Edge fallback: nearest sample.
            let idx = base.clamp(0, n - 1) as usize;
            return x[idx];
        }
        let (ym1, y0, y1, y2) = (
            x[(base - 1) as usize],
            x[base as usize],
            x[(base + 1) as usize],
            x[(base + 2) as usize],
        );
        // 4-point Lagrange basis for t ∈ [0, 1].
        let c_m1 = -t * (t - 1.0) * (t - 2.0) / 6.0;
        let c_0 = (t + 1.0) * (t - 1.0) * (t - 2.0) / 2.0;
        let c_1 = -(t + 1.0) * t * (t - 2.0) / 2.0;
        let c_2 = (t + 1.0) * t * (t - 1.0) / 6.0;
        ym1 * c_m1 + y0 * c_0 + y1 * c_1 + y2 * c_2
    }

    /// Recover symbol-spaced (I, Q) values from complex baseband, starting at
    /// sample index `start` (the preamble-search timing lock).
    ///
    /// The first output symbol is taken AT `start` (matching the pre-armed
    /// behaviour of the fixed-stride path it replaces).
    pub fn process(&self, i_bb: &[f32], q_bb: &[f32], start: usize) -> (Vec<f32>, Vec<f32>) {
        let len = i_bb.len().min(q_bb.len());
        let mut i_out = Vec::new();
        let mut q_out = Vec::new();
        if len == 0 || start >= len {
            return (i_out, q_out);
        }

        let nominal = self.sps as f64;
        let mut period = nominal;
        let min_period = nominal * (1.0 - self.max_period_dev as f64);
        let max_period = nominal * (1.0 + self.max_period_dev as f64);

        let mut pos = start as f64;
        let mut prev_i = 0.0f32;
        let mut prev_q = 0.0f32;
        let mut have_prev = false;
        // Fast power estimate for error normalisation; slow estimate for the
        // fade gate.
        let mut power = 0.0f32;
        let mut slow_power = 0.0f32;

        while pos < (len - 1) as f64 {
            let cur_i = Self::interp(i_bb, pos);
            let cur_q = Self::interp(q_bb, pos);
            i_out.push(cur_i);
            q_out.push(cur_q);

            let sym_pow = cur_i * cur_i + cur_q * cur_q;
            if power == 0.0 {
                power = sym_pow;
                slow_power = sym_pow;
            } else {
                power = 0.95 * power + 0.05 * sym_pow;
                slow_power = 0.999 * slow_power + 0.001 * sym_pow;
            }

            // Fade coast: during a deep fade the Gardner error is dominated
            // by noise while the normaliser shrinks — updating would let the
            // loop walk off a timing lock that is still valid.  Hold the
            // current period and phase until the signal returns; an actual
            // sample-rate offset persists through the fade and is re-acquired
            // immediately after.
            let faded = sym_pow < 0.1 * slow_power;

            if have_prev && !faded {
                let mid_pos = pos - period / 2.0;
                let mid_i = Self::interp(i_bb, mid_pos);
                let mid_q = Self::interp(q_bb, mid_pos);
                // Complex Gardner: e = Re{z_mid · conj(z_cur − z_prev)}.
                // Positive e ⇒ sampling late ⇒ pull the next instant earlier.
                let e = mid_i * (cur_i - prev_i) + mid_q * (cur_q - prev_q);
                let e_n = e / power.max(1e-9);
                // Clamp a single update so one noise burst cannot slip a symbol.
                let e_n = e_n.clamp(-2.0, 2.0);
                period = (period - (self.ki * e_n) as f64).clamp(min_period, max_period);
                pos += period - (self.kp * e_n) as f64;
            } else {
                pos += period;
            }

            prev_i = cur_i;
            prev_q = cur_q;
            have_prev = true;
        }

        (i_out, q_out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::FirFilter;
    use crate::rrc::generate_rrc_coefficients;

    /// Deterministic ±1 BPSK symbol stream.
    fn symbols(n: usize) -> Vec<f32> {
        let mut state = 0xACE1u32;
        (0..n)
            .map(|_| {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                if (state >> 16) & 1 == 1 {
                    1.0
                } else {
                    -1.0
                }
            })
            .collect()
    }

    /// RRC-shaped baseband from the symbol stream at `sps` samples/symbol.
    fn shaped_baseband(syms: &[f32], sps: usize) -> Vec<f32> {
        let fs = 8000.0;
        let baud = fs / sps as f32;
        let num_taps = 8 * sps + 1;
        let coeffs = generate_rrc_coefficients(fs, baud, 0.35, num_taps);
        let group_delay = (num_taps - 1) / 2;
        let mut impulses = vec![0.0f32; syms.len() * sps + group_delay];
        for (k, &s) in syms.iter().enumerate() {
            impulses[k * sps] = s;
        }
        let mut fir = FirFilter::new(coeffs);
        let filtered = fir.apply(&impulses);
        filtered[group_delay..].to_vec()
    }

    /// Linear resampling by `1 + ppm × 1e-6` (sample-rate offset model).
    fn resample_ppm(x: &[f32], ppm: f32) -> Vec<f32> {
        let ratio = 1.0f64 + ppm as f64 * 1e-6;
        let out_len = (x.len() as f64 / ratio) as usize;
        (0..out_len)
            .map(|k| {
                let pos = k as f64 * ratio;
                let base = pos.floor() as usize;
                let t = (pos - base as f64) as f32;
                if base + 1 < x.len() {
                    x[base] * (1.0 - t) + x[base + 1] * t
                } else {
                    x[x.len() - 1]
                }
            })
            .collect()
    }

    fn decision_errors(recovered: &[f32], expected: &[f32], skip: usize) -> usize {
        recovered
            .iter()
            .zip(expected.iter())
            .skip(skip)
            .filter(|(&r, &e)| (r >= 0.0) != (e >= 0.0))
            .count()
    }

    #[test]
    fn tracks_static_signal_without_drift() {
        let sps = 8;
        let syms = symbols(400);
        let bb = shaped_baseband(&syms, sps);
        let q = vec![0.0f32; bb.len()];
        let (i_out, _) = FarrowTimingLoop::new(sps).process(&bb, &q, 0);
        assert!(i_out.len() >= 380, "got {} symbols", i_out.len());
        let errs = decision_errors(&i_out, &syms, 8);
        assert_eq!(errs, 0, "{errs} decision errors on a clean static signal");
    }

    #[test]
    fn tracks_150ppm_sample_rate_offset() {
        // 150 ppm at 8 kHz / sps=8: the ISI-free instant drifts one full
        // sample every ~830 symbols.  Over 2000 symbols the fixed-stride path
        // drifts 2.4 samples (30% of the symbol period) into heavy ISI; the
        // loop's integral term must absorb it.
        let sps = 8;
        let syms = symbols(2000);
        let bb = shaped_baseband(&syms, sps);
        let bb = resample_ppm(&bb, 150.0);
        let q = vec![0.0f32; bb.len()];
        let (i_out, _) = FarrowTimingLoop::new(sps).process(&bb, &q, 0);
        assert!(i_out.len() >= 1900, "got {} symbols", i_out.len());
        let errs = decision_errors(&i_out, &syms, 32);
        assert_eq!(
            errs, 0,
            "{errs} decision errors under 150 ppm sample-rate offset"
        );
    }

    #[test]
    fn carrier_rotation_does_not_break_timing() {
        // Rotate the baseband by 60°: the complex Gardner error is invariant
        // to a common rotation, so timing must still lock (the I-only error
        // this loop replaces was data-dependent under rotation).
        let sps = 8;
        let syms = symbols(800);
        let bb = shaped_baseband(&syms, sps);
        let (s, c) = 60.0f32.to_radians().sin_cos();
        let i_rot: Vec<f32> = bb.iter().map(|&x| x * c).collect();
        let q_rot: Vec<f32> = bb.iter().map(|&x| x * s).collect();
        let bb_resampled_i = resample_ppm(&i_rot, 100.0);
        let bb_resampled_q = resample_ppm(&q_rot, 100.0);
        let (i_out, q_out) =
            FarrowTimingLoop::new(sps).process(&bb_resampled_i, &bb_resampled_q, 0);
        // De-rotate outputs and slice.
        let derot: Vec<f32> = i_out
            .iter()
            .zip(q_out.iter())
            .map(|(&i, &q)| i * c + q * s)
            .collect();
        let errs = decision_errors(&derot, &syms, 32);
        assert_eq!(errs, 0, "{errs} decision errors under rotation + 100 ppm");
    }
}
