//! Sample-rate-offset (clock-drift) channel.
//!
//! Models the RX sampling clock running at a slightly different rate than the TX
//! clock — the unavoidable condition between two independent soundcards (and two
//! on-air stations). The waveform is identical; only the sampling grid differs.
//!
//! If the RX clock is `ppm` parts-per-million faster than the TX clock, the RX
//! collects `(1 + ppm·1e-6)` times as many samples for the same waveform. We model
//! that by resampling the TX block by `ratio = 1 + ppm·1e-6` with 4-point cubic
//! (Catmull-Rom) interpolation — clean enough at HF audio frequencies that any
//! resulting decode failure is attributable to the timing slip, not interpolation
//! artifacts. A receiver that assumes the nominal rate then sees its symbol clock
//! drift by `ppm` across the frame, exactly as on real hardware.
//!
//! This is a pure multiplicative/timing impairment: [`generate_noise`] returns zeros.

use crate::{ChannelError, ChannelModel};

/// Sample-rate-offset channel configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct SroConfig {
    /// RX-vs-TX clock offset in parts-per-million. Positive = RX clock faster
    /// (more samples per frame); negative = slower. Typical USB soundcard crystal
    /// tolerance is ±20–100 ppm, so the relative offset between two cards can reach
    /// a few hundred ppm.
    pub ppm: f32,
}

impl SroConfig {
    pub fn new(ppm: f32) -> Self {
        Self { ppm }
    }
}

/// Sample-rate-offset channel: resamples each block by `1 + ppm·1e-6`.
pub struct SroChannel {
    ratio: f64,
}

impl SroChannel {
    pub fn new(config: SroConfig) -> Result<Self, ChannelError> {
        if !config.ppm.is_finite() {
            return Err(ChannelError::InvalidParameter("ppm must be finite".into()));
        }
        let ratio = 1.0 + config.ppm as f64 * 1e-6;
        if ratio <= 0.0 {
            return Err(ChannelError::InvalidParameter(
                "ppm offset must leave a positive resample ratio".into(),
            ));
        }
        Ok(Self { ratio })
    }

    /// 4-point Catmull-Rom cubic interpolation of `input` at fractional index `t`.
    fn cubic(input: &[f32], t: f64) -> f32 {
        let n = input.len() as isize;
        let k = t.floor() as isize;
        let frac = (t - k as f64) as f32;
        let at = |i: isize| input[i.clamp(0, n - 1) as usize];
        let p0 = at(k - 1);
        let p1 = at(k);
        let p2 = at(k + 1);
        let p3 = at(k + 2);
        let a0 = -0.5 * p0 + 1.5 * p1 - 1.5 * p2 + 0.5 * p3;
        let a1 = p0 - 2.5 * p1 + 2.0 * p2 - 0.5 * p3;
        let a2 = -0.5 * p0 + 0.5 * p2;
        let a3 = p1;
        ((a0 * frac + a1) * frac + a2) * frac + a3
    }
}

impl ChannelModel for SroChannel {
    fn apply(&mut self, input: &[f32]) -> Vec<f32> {
        if input.is_empty() {
            return Vec::new();
        }
        // Output length scales with the clock ratio: a faster RX clock yields more samples.
        let m = ((input.len() as f64) * self.ratio).round() as usize;
        let mut out = Vec::with_capacity(m);
        for i in 0..m {
            // Output sample i corresponds to input time i / ratio.
            out.push(Self::cubic(input, i as f64 / self.ratio));
        }
        out
    }

    fn generate_noise(&mut self, length: usize) -> Vec<f32> {
        vec![0.0; length]
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    #[test]
    fn rejects_non_finite_ppm() {
        assert!(SroChannel::new(SroConfig::new(f32::NAN)).is_err());
    }

    #[test]
    fn zero_ppm_is_identity_length_and_near_passthrough() {
        let input: Vec<f32> = (0..2000)
            .map(|i| (2.0 * PI * 1200.0 * i as f32 / 8000.0).sin())
            .collect();
        let mut ch = SroChannel::new(SroConfig::new(0.0)).unwrap();
        let out = ch.apply(&input);
        assert_eq!(out.len(), input.len());
        // Cubic interp at integer positions reproduces the samples exactly.
        for (a, b) in input.iter().zip(out.iter()) {
            assert!((a - b).abs() < 1e-5, "{a} vs {b}");
        }
    }

    #[test]
    fn positive_ppm_lengthens_negative_shortens() {
        let input = vec![0.25f32; 100_000];
        let mut up = SroChannel::new(SroConfig::new(100.0)).unwrap();
        let mut dn = SroChannel::new(SroConfig::new(-100.0)).unwrap();
        assert_eq!(up.apply(&input).len(), 100_010); // +100 ppm of 100k = +10
        assert_eq!(dn.apply(&input).len(), 99_990);
    }

    /// A pure tone stays a tone of the same value range; SRO shifts its apparent
    /// frequency by the ppm fraction but does not add noise.
    #[test]
    fn tone_amplitude_preserved() {
        let input: Vec<f32> = (0..8000)
            .map(|i| (2.0 * PI * 1000.0 * i as f32 / 8000.0).sin())
            .collect();
        let mut ch = SroChannel::new(SroConfig::new(50.0)).unwrap();
        let out = ch.apply(&input);
        let peak = out.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        assert!(
            (0.97..=1.03).contains(&peak),
            "peak {peak} should stay ~1.0"
        );
    }
}
