//! Shared, carrier-phase-insensitive frame acquisition primitives.
//!
//! A bare real cross-correlation (`Σ a·b`) against a known passband template is
//! carrier-phase sensitive: an arbitrary carrier phase (async sound-card clocks,
//! multi-second capture latency) rotates the received waveform, and at ~90° the
//! real correlation collapses to near zero so the search locks to a wrong
//! offset.  This failure was independently found and fixed in the QPSK, OFDM
//! (#385), and SCFDMA (#386) plugins.  This module is the single shared
//! implementation of the fixed pattern: correlate against BOTH the template and
//! its quadrature (Hilbert) companion and use the I/Q magnitude, which is
//! invariant to the carrier phase.

use num_complex::Complex32;
use rustfft::FftPlanner;

/// Quadrature (90°-shifted) companion of a real signal via the FFT Hilbert
/// transform: the imaginary part of the analytic signal.
pub fn quadrature(x: &[f32]) -> Vec<f32> {
    let n = x.len();
    if n == 0 {
        return vec![];
    }
    let mut planner = FftPlanner::<f32>::new();
    let fwd = planner.plan_fft_forward(n);
    let inv = planner.plan_fft_inverse(n);
    let mut buf: Vec<Complex32> = x.iter().map(|&v| Complex32::new(v, 0.0)).collect();
    fwd.process(&mut buf);
    let half = n / 2;
    for (k, c) in buf.iter_mut().enumerate() {
        if k == 0 || (n.is_multiple_of(2) && k == half) {
            // DC and Nyquist unchanged.
        } else if k < half {
            *c *= 2.0; // positive frequencies doubled
        } else {
            *c = Complex32::new(0.0, 0.0); // negative frequencies zeroed
        }
    }
    inv.process(&mut buf);
    let scale = 1.0 / n as f32;
    buf.iter().map(|c| c.im * scale).collect()
}

/// Result of an [`IqMatchedFilter`] search.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IqSearchResult {
    /// Best-scoring sample offset into the searched slice.
    pub offset: usize,
    /// Raw (unnormalised) squared correlation magnitude at `offset`.
    pub score: f32,
    /// Normalised correlation magnitude ρ ∈ [0, 1] at `offset`:
    /// `|corr| / sqrt(window_energy × template_energy)`.  Use this for
    /// presence detection — on noise it stays well below typical lock values.
    pub rho: f32,
}

/// Carrier-phase-insensitive matched filter for a known real passband template.
pub struct IqMatchedFilter {
    template: Vec<f32>,
    template_q: Vec<f32>,
    t_energy: f32,
}

impl IqMatchedFilter {
    /// Build the filter; precomputes the Hilbert quadrature companion.
    pub fn new(template: Vec<f32>) -> Self {
        let template_q = quadrature(&template);
        let t_energy = template.iter().map(|&x| x * x).sum();
        Self {
            template,
            template_q,
            t_energy,
        }
    }

    /// Template length in samples.
    pub fn len(&self) -> usize {
        self.template.len()
    }

    /// Returns `true` if the template is empty.
    pub fn is_empty(&self) -> bool {
        self.template.is_empty()
    }

    /// Correlation magnitude² and window energy at one offset.
    fn score_at(&self, samples: &[f32], offset: usize) -> (f32, f32) {
        let win = &samples[offset..offset + self.template.len()];
        let mut dot_i = 0.0f32;
        let mut dot_q = 0.0f32;
        let mut energy = 0.0f32;
        for (m, &s) in win.iter().enumerate() {
            dot_i += s * self.template[m];
            dot_q += s * self.template_q[m];
            energy += s * s;
        }
        (dot_i * dot_i + dot_q * dot_q, energy)
    }

    /// Search offsets `0..=bound` (clamped to the available samples) for the
    /// maximum unnormalised correlation magnitude.
    ///
    /// The argmax uses the *unnormalised* score: like the original `Σ a·b` it
    /// favours high-correlation *and* high-energy alignment, so a deep-fade
    /// low-energy window cannot win.  The returned [`IqSearchResult::rho`] is
    /// the normalised magnitude at the winning offset, suitable for a
    /// detection threshold.  Returns `None` when the slice is shorter than the
    /// template.
    pub fn search(&self, samples: &[f32], bound: usize) -> Option<IqSearchResult> {
        if samples.len() < self.template.len() || self.template.is_empty() {
            return None;
        }
        let max_offset = (samples.len() - self.template.len()).min(bound);

        let mut best_offset = 0usize;
        let mut best_score = f32::NEG_INFINITY;
        for offset in 0..=max_offset {
            let (score, _) = self.score_at(samples, offset);
            if score > best_score {
                best_score = score;
                best_offset = offset;
            }
        }

        let (score, energy) = self.score_at(samples, best_offset);
        let denom = (energy * self.t_energy).sqrt() + 1e-12;
        Some(IqSearchResult {
            offset: best_offset,
            score,
            rho: score.sqrt() / denom,
        })
    }

    /// Normalised correlation ρ for every offset in `lo..=hi` (clamped).
    ///
    /// Used by multipath-aware acquisition (e.g. OFDM leading-path selection)
    /// that needs the full correlation profile rather than just the argmax.
    pub fn rho_profile(&self, samples: &[f32], lo: usize, hi: usize) -> Vec<f32> {
        if samples.len() < self.template.len() {
            return vec![];
        }
        let max_offset = samples.len() - self.template.len();
        let hi = hi.min(max_offset);
        if lo > hi {
            return vec![];
        }
        (lo..=hi)
            .map(|d| {
                let (score, energy) = self.score_at(samples, d);
                score.sqrt() / ((energy * self.t_energy).sqrt() + 1e-12)
            })
            .collect()
    }
}

/// Squared magnitude of the complex preamble correlation `|Σ r·conj(e)|²`.
///
/// The carrier-phase-invariant symbol-domain timing metric: at the correct
/// timing the magnitude is maximal for any carrier phase, where the bare real
/// part collapses at 90°/270°.  `received` and `expected` are (I, Q) pairs;
/// correlation runs over `min(len)` symbols.
pub fn preamble_corr_sq(received: &[(f32, f32)], expected: &[(f32, f32)]) -> f32 {
    let (re_sum, im_sum) = received
        .iter()
        .zip(expected.iter())
        .fold((0.0f32, 0.0f32), |(re, im), (&(ri, rq), &(ei, eq))| {
            (re + ri * ei + rq * eq, im + rq * ei - ri * eq)
        });
    re_sum * re_sum + im_sum * im_sum
}

/// M-th power carrier frequency offset estimator for M-PSK symbol streams.
///
/// Raising each symbol to the M-th power removes M-ary PSK modulation, leaving
/// a phasor rotating at `M·2π·Δf/baud` per symbol; the mean phase of
/// consecutive products gives `Δf`.  **Range: ±baud/(2·M).**
///
/// Only valid for (near-)constant-modulus constellations; QAM data symbols
/// add heavy self-noise — use a data-aided preamble estimator instead.
pub fn estimate_cfo_mth_power(i_syms: &[f32], q_syms: &[f32], baud_rate: f32, m: u32) -> f32 {
    if i_syms.len() < 2 || m == 0 {
        return 0.0;
    }

    let mut re_m = Vec::with_capacity(i_syms.len());
    let mut im_m = Vec::with_capacity(i_syms.len());
    for (&i, &q) in i_syms.iter().zip(q_syms.iter()) {
        let mut re = i;
        let mut im = q;
        for _ in 1..m {
            let next_re = re * i - im * q;
            let next_im = re * q + im * i;
            re = next_re;
            im = next_im;
        }
        re_m.push(re);
        im_m.push(im);
    }

    let mut re_sum = 0.0f32;
    let mut im_sum = 0.0f32;
    for k in 1..re_m.len() {
        re_sum += re_m[k] * re_m[k - 1] + im_m[k] * im_m[k - 1];
        im_sum += im_m[k] * re_m[k - 1] - re_m[k] * im_m[k - 1];
    }

    im_sum.atan2(re_sum) * baud_rate / (2.0 * std::f32::consts::PI * m as f32)
}

/// Data-aided carrier frequency offset estimator against a known preamble.
///
/// Removes the known preamble modulation by `y[k] = z[k]·conj(p[k])`, then
/// estimates the per-symbol rotation from consecutive products.
/// **Range: ±baud/2** — much wider than blind M-th-power estimation, with no
/// constellation-dependent self-noise.  Returns `None` for < 2 usable symbols.
pub fn estimate_cfo_data_aided(
    i_syms: &[f32],
    q_syms: &[f32],
    preamble: &[(f32, f32)],
    baud_rate: f32,
) -> Option<f32> {
    let n = i_syms.len().min(q_syms.len()).min(preamble.len());
    if n < 2 {
        return None;
    }

    let mut y_re = Vec::with_capacity(n);
    let mut y_im = Vec::with_capacity(n);
    for k in 0..n {
        let (pr, pi) = preamble[k];
        y_re.push(i_syms[k] * pr + q_syms[k] * pi);
        y_im.push(q_syms[k] * pr - i_syms[k] * pi);
    }

    let mut re_sum = 0.0f32;
    let mut im_sum = 0.0f32;
    for k in 1..n {
        re_sum += y_re[k] * y_re[k - 1] + y_im[k] * y_im[k - 1];
        im_sum += y_im[k] * y_re[k - 1] - y_re[k] * y_im[k - 1];
    }

    Some(im_sum.atan2(re_sum) * baud_rate / (2.0 * std::f32::consts::PI))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn chirp_template(len: usize) -> Vec<f32> {
        (0..len)
            .map(|i| {
                let t = i as f32 / len as f32;
                (2.0 * PI * (5.0 + 20.0 * t) * t * 8.0).sin()
            })
            .collect()
    }

    #[test]
    fn quadrature_shifts_sine_to_negative_cosine_phase() {
        let n = 256;
        let x: Vec<f32> = (0..n)
            .map(|i| (2.0 * PI * 8.0 * i as f32 / n as f32).sin())
            .collect();
        let q = quadrature(&x);
        // Hilbert of sin is -cos.
        for (i, &v) in q.iter().enumerate().skip(16).take(n - 32) {
            let expect = -(2.0 * PI * 8.0 * i as f32 / n as f32).cos();
            assert!((v - expect).abs() < 0.05, "idx {i}: {v} vs {expect}");
        }
    }

    #[test]
    fn matched_filter_finds_offset_for_any_carrier_phase() {
        let template = chirp_template(512);
        let filt = IqMatchedFilter::new(template.clone());
        let template_q = quadrature(&template);
        let true_offset = 137usize;

        for phase_deg in [0.0f32, 45.0, 90.0, 135.0, 180.0, 270.0] {
            let (s, c) = (phase_deg.to_radians()).sin_cos();
            // Rotate the analytic signal: cosφ·x + sinφ·x_q ≈ phase-shifted x.
            let rotated: Vec<f32> = template
                .iter()
                .zip(template_q.iter())
                .map(|(&i, &q)| c * i + s * q)
                .collect();
            let mut samples = vec![0.0f32; true_offset];
            samples.extend_from_slice(&rotated);
            samples.extend(vec![0.0f32; 256]);

            let r = filt.search(&samples, 8192).expect("search");
            assert_eq!(
                r.offset, true_offset,
                "phase {phase_deg}°: offset {} ≠ {true_offset}",
                r.offset
            );
            assert!(r.rho > 0.9, "phase {phase_deg}°: rho {} too low", r.rho);
        }
    }

    #[test]
    fn matched_filter_rho_low_on_noise() {
        let template = chirp_template(512);
        let filt = IqMatchedFilter::new(template);
        // Deterministic pseudo-noise.
        let mut state = 0x12345678u32;
        let noise: Vec<f32> = (0..4096)
            .map(|_| {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                (state >> 16) as f32 / 32768.0 - 1.0
            })
            .collect();
        let r = filt.search(&noise, 8192).expect("search");
        assert!(r.rho < 0.5, "noise rho {} should be well below lock", r.rho);
    }

    #[test]
    fn preamble_corr_sq_invariant_to_carrier_phase() {
        let expected: Vec<(f32, f32)> = (0..16)
            .map(|k| {
                let a = k as f32 * 2.4;
                (a.cos(), a.sin())
            })
            .collect();
        let base = preamble_corr_sq(&expected, &expected);
        for phase in [0.5f32, PI / 2.0, PI, 4.0] {
            let (s, c) = phase.sin_cos();
            let rotated: Vec<(f32, f32)> = expected
                .iter()
                .map(|&(i, q)| (i * c - q * s, i * s + q * c))
                .collect();
            let m = preamble_corr_sq(&rotated, &expected);
            assert!(
                (m - base).abs() / base < 1e-4,
                "phase {phase}: {m} vs {base}"
            );
        }
    }

    #[test]
    fn mth_power_estimates_qpsk_cfo() {
        let baud = 250.0f32;
        let cfo = 5.0f32;
        let n = 200;
        // Random-ish QPSK data with a CFO rotation.
        let mut i_syms = Vec::with_capacity(n);
        let mut q_syms = Vec::with_capacity(n);
        for k in 0..n {
            let data_phase = PI / 4.0 + (k % 4) as f32 * PI / 2.0;
            let total = data_phase + 2.0 * PI * cfo * k as f32 / baud;
            i_syms.push(total.cos());
            q_syms.push(total.sin());
        }
        let est = estimate_cfo_mth_power(&i_syms, &q_syms, baud, 4);
        assert!((est - cfo).abs() < 0.5, "estimated {est}, expected {cfo}");
    }

    #[test]
    fn data_aided_estimates_cfo_beyond_mth_power_range() {
        let baud = 500.0f32;
        let cfo = 100.0f32; // beyond ±baud/8 = 62.5 Hz 4th-power range
        let n = 16;
        let preamble: Vec<(f32, f32)> = (0..n)
            .map(|k| {
                let a = (k as f32 * 1.9).sin() * PI;
                (a.cos(), a.sin())
            })
            .collect();
        let mut i_syms = Vec::with_capacity(n);
        let mut q_syms = Vec::with_capacity(n);
        for (k, &(pi_, pq)) in preamble.iter().enumerate() {
            let rot = 2.0 * PI * cfo * k as f32 / baud;
            let (s, c) = rot.sin_cos();
            i_syms.push(pi_ * c - pq * s);
            q_syms.push(pi_ * s + pq * c);
        }
        let est = estimate_cfo_data_aided(&i_syms, &q_syms, &preamble, baud).expect("estimate");
        assert!((est - cfo).abs() < 2.0, "estimated {est}, expected {cfo}");
    }
}
