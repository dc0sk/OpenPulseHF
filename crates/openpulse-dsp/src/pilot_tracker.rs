//! Pilot-aided carrier tracking — a type-2 PLL driven by known in-band pilots.
//!
//! Inserting known pilot symbols at a fixed cadence lets the receiver track
//! residual carrier phase and frequency from the pilots alone. Because the
//! reference is *known* (not a slicer decision), the loop is immune to the
//! decision errors and ±90°/±45° cycle slips that defeat a decision-directed
//! Costas loop on dense constellations (8PSK / 16QAM / 32APSK / 64QAM) at low
//! SNR — the failure mode the existing single-Costas modes hit. The NCO advances
//! every symbol and is corrected at each pilot; a pilot-referenced amplitude
//! estimate (an AGC reference) is tracked alongside.
//!
//! This is the qo100-modem `pilot_pll` pattern, the basis of the planned
//! pilot-framed waveform. Symbols are complex `(re, im)` pairs, matching the rest
//! of `openpulse-dsp`.

use std::f32::consts::PI;

/// Complex symbol as `(real, imag)`.
type C = (f32, f32);

/// Pilot-aided carrier (phase + frequency) tracker with amplitude estimate.
pub struct PilotTracker {
    /// NCO phase estimate (rad).
    phase: f32,
    /// NCO frequency estimate (rad/symbol).
    freq: f32,
    /// Proportional loop gain.
    alpha: f32,
    /// Integral loop gain.
    beta: f32,
    /// Pilot-referenced amplitude estimate (EMA).
    amplitude: f32,
    /// Amplitude EMA rate.
    amp_alpha: f32,
}

impl PilotTracker {
    /// Create a tracker.
    ///
    /// `loop_bw` — normalised loop bandwidth applied at each pilot update
    /// (0.01–0.1 typical; larger acquires faster but is noisier). Gains use the
    /// same critically-damped 2nd-order form as [`crate::pll::CarrierPll`].
    pub fn new(loop_bw: f32) -> Self {
        let damp = 0.707f32;
        Self {
            phase: 0.0,
            freq: 0.0,
            alpha: 2.0 * damp * loop_bw,
            beta: loop_bw * loop_bw,
            amplitude: 1.0,
            amp_alpha: 0.1,
        }
    }

    /// Seed the NCO frequency (rad/symbol) from a coarse CFO estimate so the loop
    /// only has to track the residual.
    pub fn seed_frequency(&mut self, freq_rad_per_sym: f32) {
        self.freq = freq_rad_per_sym;
    }

    /// Tracked frequency (rad/symbol).
    pub fn frequency(&self) -> f32 {
        self.freq
    }

    /// Tracked phase (rad).
    pub fn phase(&self) -> f32 {
        self.phase
    }

    /// Pilot-referenced amplitude estimate (an AGC reference).
    pub fn amplitude(&self) -> f32 {
        self.amplitude
    }

    /// De-rotate one symbol by the current NCO estimate, updating the loop when a
    /// known `pilot` is supplied. Returns the corrected symbol.
    pub fn process(&mut self, x: C, pilot: Option<C>) -> C {
        let (cos_p, sin_p) = (self.phase.cos(), self.phase.sin());
        // y = x · e^{−j·phase}
        let y = (x.0 * cos_p + x.1 * sin_p, -x.0 * sin_p + x.1 * cos_p);

        if let Some(p) = pilot {
            // Wipe the known pilot: e = y · conj(p); its angle is the residual
            // carrier phase error (true − estimated).
            let e = (y.0 * p.0 + y.1 * p.1, y.1 * p.0 - y.0 * p.1);
            let err = e.1.atan2(e.0);
            self.phase += self.alpha * err;
            self.freq += self.beta * err;

            let pmag = (p.0 * p.0 + p.1 * p.1).sqrt().max(1e-9);
            let ymag = (y.0 * y.0 + y.1 * y.1).sqrt();
            self.amplitude += self.amp_alpha * (ymag / pmag - self.amplitude);
        }

        // NCO advance, wrapped to [−π, π].
        self.phase += self.freq;
        if self.phase > PI {
            self.phase -= 2.0 * PI;
        } else if self.phase < -PI {
            self.phase += 2.0 * PI;
        }
        y
    }

    /// Correct a full symbol frame whose pilots sit at every `spacing`-th symbol
    /// starting at index 0, with `pilots[j]` the known value of the j-th pilot.
    /// Returns the corrected stream (pilots included, in place).
    pub fn correct_frame(&mut self, symbols: &[C], pilots: &[C], spacing: usize) -> Vec<C> {
        let mut out = Vec::with_capacity(symbols.len());
        let mut pj = 0usize;
        for (k, &x) in symbols.iter().enumerate() {
            let pilot = if spacing > 0 && k % spacing == 0 && pj < pilots.len() {
                let p = pilots[pj];
                pj += 1;
                Some(p)
            } else {
                None
            };
            out.push(self.process(x, pilot));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rot(x: C, ang: f32) -> C {
        let (c, s) = (ang.cos(), ang.sin());
        (x.0 * c - x.1 * s, x.0 * s + x.1 * c)
    }

    fn phase_err(a: C, b: C) -> f32 {
        // angle(a · conj(b))
        let e = (a.0 * b.0 + a.1 * b.1, a.1 * b.0 - a.0 * b.1);
        e.1.atan2(e.0).abs()
    }

    /// Build a frame of QPSK data with a BPSK pilot every `spacing` symbols.
    fn make_frame(n: usize, spacing: usize) -> (Vec<C>, Vec<C>) {
        let qpsk = [
            (0.707f32, 0.707),
            (-0.707, 0.707),
            (-0.707, -0.707),
            (0.707, -0.707),
        ];
        let pilot = (1.0f32, 0.0);
        let mut syms = Vec::with_capacity(n);
        let mut pilots = Vec::new();
        for k in 0..n {
            if k % spacing == 0 {
                syms.push(pilot);
                pilots.push(pilot);
            } else {
                syms.push(qpsk[(k * 7 + 3) % 4]);
            }
        }
        (syms, pilots)
    }

    #[test]
    fn removes_static_phase_offset() {
        let (clean, pilots) = make_frame(256, 8);
        let phi0 = 1.1f32;
        let rxd: Vec<C> = clean.iter().map(|&x| rot(x, phi0)).collect();

        let mut t = PilotTracker::new(0.1);
        let corrected = t.correct_frame(&rxd, &pilots, 8);

        // Last quarter: corrected symbols should be back near the clean ones.
        let tail = clean.len() * 3 / 4;
        let mean: f32 = corrected[tail..]
            .iter()
            .zip(&clean[tail..])
            .map(|(&c, &z)| phase_err(c, z))
            .sum::<f32>()
            / (clean.len() - tail) as f32;
        assert!(
            mean < 0.1,
            "residual phase error {mean} after static offset"
        );
    }

    #[test]
    fn acquires_and_tracks_frequency_offset() {
        let (clean, pilots) = make_frame(2000, 8);
        let dphi = 0.02f32; // rad/symbol carrier frequency offset
        let phi0 = 0.4f32;
        let rxd: Vec<C> = clean
            .iter()
            .enumerate()
            .map(|(k, &x)| rot(x, phi0 + dphi * k as f32))
            .collect();

        let mut t = PilotTracker::new(0.1);
        let corrected = t.correct_frame(&rxd, &pilots, 8);

        assert!(
            (t.frequency() - dphi).abs() < 0.003,
            "tracked freq {} should reach {dphi}",
            t.frequency()
        );

        // After acquisition the corrected data should track the clean symbols.
        let tail = clean.len() * 3 / 4;
        let mean: f32 = corrected[tail..]
            .iter()
            .zip(&clean[tail..])
            .map(|(&c, &z)| phase_err(c, z))
            .sum::<f32>()
            / (clean.len() - tail) as f32;
        assert!(
            mean < 0.12,
            "residual phase error {mean} after CFO tracking"
        );
    }

    #[test]
    fn seeding_frequency_speeds_acquisition() {
        let (clean, pilots) = make_frame(400, 8);
        let dphi = 0.03f32;
        let rxd: Vec<C> = clean
            .iter()
            .enumerate()
            .map(|(k, &x)| rot(x, dphi * k as f32))
            .collect();

        let mut t = PilotTracker::new(0.05);
        t.seed_frequency(dphi); // perfect coarse estimate
        let corrected = t.correct_frame(&rxd, &pilots, 8);

        // With the frequency seeded, even early symbols are well corrected.
        let mean: f32 = corrected[50..]
            .iter()
            .zip(&clean[50..])
            .map(|(&c, &z)| phase_err(c, z))
            .sum::<f32>()
            / (clean.len() - 50) as f32;
        assert!(mean < 0.05, "seeded residual phase error {mean}");
    }

    #[test]
    fn tracks_pilot_amplitude() {
        let (clean, pilots) = make_frame(256, 8);
        let scale = 2.5f32;
        let rxd: Vec<C> = clean.iter().map(|&x| (x.0 * scale, x.1 * scale)).collect();

        let mut t = PilotTracker::new(0.1);
        t.correct_frame(&rxd, &pilots, 8);
        assert!(
            (t.amplitude() - scale).abs() < 0.1 * scale,
            "amplitude estimate {} should track {scale}",
            t.amplitude()
        );
    }
}
