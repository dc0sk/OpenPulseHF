//! Costas carrier-recovery PLL for PSK demodulation.
//!
//! The Costas loop is a classic decision-directed carrier-phase recovery
//! algorithm. It operates on the post-matched-filter, post-timing-recovery
//! IQ samples and drives a phase NCO to rotate each symbol back to its
//! nominal constellation phase.
//!
//! Supported discriminants:
//!
//! - **BPSK** (1 bit/symbol): `e = Q × sign(I)`
//! - **QPSK** (2 bits/symbol): `e = I·sign(Q) − Q·sign(I)`
//! - **8PSK** (3 bits/symbol): decision-directed —
//!   phase of `received × conj(decided)` using nearest-constellation-point.

use std::f32::consts::PI;

/// Costas / decision-directed PLL for coherent PSK carrier recovery.
pub struct CarrierPll {
    /// Current carrier phase estimate (radians).
    pub phase: f32,
    /// Accumulated frequency error (radians per sample).
    freq_err: f32,
    /// Proportional (α) and integral (β) loop filter gains.
    alpha: f32,
    beta: f32,
    /// PSK order: 1 (BPSK), 2 (QPSK), 3 (8PSK).
    psk_order: u32,
}

impl CarrierPll {
    /// Create a new PLL.
    ///
    /// `loop_bw` — normalised loop bandwidth (Bn·Ts); 0.01–0.05 is typical.
    ///             Larger values track faster but are noisier.
    /// `psk_order` — 1, 2, or 3 (bits per symbol = modulation order).
    pub fn new(loop_bw: f32, psk_order: u32) -> Self {
        // Second-order loop filter gains derived from loop bandwidth
        // using the approximation from Mengali & D'Andrea:
        //   damp ≈ 1/√2, α = 2·damp·Bn, β = Bn²
        let damp = 1.0 / 2.0_f32.sqrt();
        let alpha = 2.0 * damp * loop_bw;
        let beta = loop_bw * loop_bw;
        Self {
            phase: 0.0,
            freq_err: 0.0,
            alpha,
            beta,
            psk_order,
        }
    }

    /// Update the PLL with a new IQ sample.
    ///
    /// Returns the phase correction in radians that should be applied to the
    /// raw IQ sample to align it with the constellation before slicing.
    ///
    /// The correction is: `i_corr = i·cos(-phase) − q·sin(-phase)`.
    pub fn update(&mut self, i: f32, q: f32) -> f32 {
        // Apply current phase estimate before computing the error
        // so the discriminant sees phase-corrected IQ.
        let (i_c, q_c) = self.correct(i, q);
        let error = self.discriminant(i_c, q_c);
        self.freq_err += self.beta * error;
        self.phase += self.alpha * error + self.freq_err;
        // Wrap phase to [-π, π)
        self.phase = wrap(self.phase);
        -self.phase
    }

    /// Apply the stored phase correction to an IQ sample.
    #[inline]
    pub fn correct(&self, i: f32, q: f32) -> (f32, f32) {
        let corr = -self.phase;
        let (s, c) = corr.sin_cos();
        (i * c - q * s, i * s + q * c)
    }

    /// Reset the PLL state.
    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.freq_err = 0.0;
    }

    /// Phase error discriminant for the current PSK order.
    fn discriminant(&self, i: f32, q: f32) -> f32 {
        match self.psk_order {
            1 => q * i.signum(),                  // BPSK
            2 => q * i.signum() - i * q.signum(), // QPSK Costas (standard form)
            3 => {
                // 8PSK decision-directed: find nearest 8-PSK phase and compute error
                let angle = q.atan2(i);
                let nearest = (angle * 4.0 / PI).round() * PI / 4.0;
                wrap(angle - nearest)
            }
            _ => 0.0,
        }
    }
}

/// Wrap angle to (−π, π].
#[inline]
fn wrap(a: f32) -> f32 {
    let mut x = a;
    while x > PI {
        x -= 2.0 * PI;
    }
    while x <= -PI {
        x += 2.0 * PI;
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bpsk_pll_tracks_constant_phase_offset() {
        let mut pll = CarrierPll::new(0.05, 1);
        // Feed a BPSK +1 symbol with a constant 0.3 rad phase offset.
        let phase_offset = 0.3f32;
        let (i_raw, q_raw) = (phase_offset.cos(), phase_offset.sin());
        for _ in 0..200 {
            let corr = pll.update(i_raw, q_raw);
            let _ = corr;
        }
        // After 200 iterations the PLL phase should converge close to the offset.
        let residual = (pll.phase - phase_offset).abs();
        assert!(
            residual < 0.05,
            "BPSK PLL residual {residual:.4} rad (phase={:.4})",
            pll.phase
        );
    }

    #[test]
    fn qpsk_pll_tracks_constant_phase_offset() {
        let mut pll = CarrierPll::new(0.05, 2);
        // QPSK symbol in first quadrant, rotated by 0.2 rad.
        let base_phase = PI / 4.0; // 45°
        let offset = 0.2f32;
        let (i_raw, q_raw) = ((base_phase + offset).cos(), (base_phase + offset).sin());
        for _ in 0..300 {
            pll.update(i_raw, q_raw);
        }
        let residual = (pll.phase - offset).abs();
        assert!(residual < 0.08, "QPSK PLL residual {residual:.4} rad");
    }

    #[test]
    fn phase_wraps_to_valid_range() {
        assert!((wrap(PI + 0.1) + PI - 0.1).abs() < 1e-5);
        assert!((wrap(-PI - 0.1) - PI + 0.1).abs() < 1e-5);
    }

    #[test]
    fn reset_clears_state() {
        let mut pll = CarrierPll::new(0.05, 2);
        for _ in 0..100 {
            pll.update(1.0, 0.5);
        }
        pll.reset();
        assert_eq!(pll.phase, 0.0);
    }
}
