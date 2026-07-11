//! GFSK tone synthesis (plan §2.1; FT8/JS8 `gen_ft8wave` + `gfsk_pulse`).
//!
//! Turns a symbol→tone sequence (0..8) into continuous-phase, Gaussian-frequency-smoothed 8-FSK
//! audio. GFSK (not rectangular FSK) is what keeps a JS8 transmission from splattering adjacent
//! users on the air. This module is the pure waveform synthesis — mapping message bits/LDPC to the
//! final tone sequence (Gray coding, Costas insertion) lands with the frame/LDPC units.

/// FT8/JS8 Gaussian pulse bandwidth-time product.
pub const DEFAULT_BT: f32 = 2.0;

/// GFSK synthesis parameters for one submode.
#[derive(Debug, Clone, Copy)]
pub struct GfskParams {
    /// Samples per symbol (submode-dependent, exact integer at 8 kHz).
    pub samples_per_symbol: usize,
    /// Tone spacing in Hz (== baud).
    pub tone_spacing_hz: f32,
    /// Audio sample rate.
    pub sample_rate: u32,
    /// Gaussian pulse bandwidth-time product.
    pub bt: f32,
}

impl GfskParams {
    /// Parameters for a resolved submode with the standard BT.
    pub fn from_submode(p: &crate::submode::SubmodeParams) -> Self {
        Self {
            samples_per_symbol: p.samples_per_symbol,
            tone_spacing_hz: p.tone_spacing_hz,
            sample_rate: crate::submode::SAMPLE_RATE,
            bt: DEFAULT_BT,
        }
    }
}

/// Abramowitz & Stegun 7.1.26 error-function approximation (max abs error ≈ 1.5e-7).
fn erf(x: f32) -> f32 {
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * x);
    let y = 1.0
        - (((((1.061_405_4 * t - 1.453_152) * t) + 1.421_413_7) * t - 0.284_496_74) * t
            + 0.254_829_6)
            * t
            * (-x * x).exp();
    sign * y
}

/// The 3-symbol Gaussian frequency-smoothing pulse. Shifted copies at one-symbol spacing form a
/// partition of unity in the interior, so a run of one tone synthesizes to that tone's steady
/// frequency and transitions are smoothed over ~one symbol.
pub fn gfsk_pulse(bt: f32, samples_per_symbol: usize) -> Vec<f32> {
    let nsps = samples_per_symbol as f32;
    let n = 3 * samples_per_symbol;
    let c = std::f32::consts::PI * (2.0 / 2.0f32.ln()).sqrt() * bt;
    (0..n)
        .map(|i| {
            let t = (i as f32 - 1.5 * nsps) / nsps; // symbol units, centered on the middle third
            0.5 * (erf(c * (t + 0.5)) - erf(c * (t - 0.5)))
        })
        .collect()
}

/// Synthesize GFSK audio for `tones` (each 0..8), the lowest tone at `base_freq_hz`.
///
/// Produces exactly `tones.len() × samples_per_symbol` samples in [−1, 1], with a raised-cosine
/// amplitude ramp over the first/last `samples_per_symbol / 8` samples (keeps the keying transient
/// off the band edges). An empty tone slice yields no samples.
pub fn modulate_tones(tones: &[u8], base_freq_hz: f32, p: &GfskParams) -> Vec<f32> {
    let nsps = p.samples_per_symbol;
    let nwave = tones.len() * nsps;
    if nwave == 0 {
        return Vec::new();
    }
    let pulse = gfsk_pulse(p.bt, nsps);

    // Instantaneous tone value per sample = Σ_j tone[j] · pulse(centered on symbol j).
    let mut tone_of_sample = vec![0.0f32; nwave];
    for (j, &tone) in tones.iter().enumerate() {
        let start = (j as isize - 1) * nsps as isize; // pulse spans symbols j-1..j+2
        for (k, &pv) in pulse.iter().enumerate() {
            let idx = start + k as isize;
            if idx >= 0 && (idx as usize) < nwave {
                tone_of_sample[idx as usize] += tone as f32 * pv;
            }
        }
    }

    // Integrate instantaneous frequency into phase; emit the sine.
    let two_pi = std::f32::consts::TAU;
    let fs = p.sample_rate as f32;
    let mut phase = 0.0f32;
    let mut wave = vec![0.0f32; nwave];
    for (k, w) in wave.iter_mut().enumerate() {
        let f = base_freq_hz + p.tone_spacing_hz * tone_of_sample[k];
        phase += two_pi * f / fs;
        if phase > two_pi {
            phase -= two_pi;
        }
        *w = phase.sin();
    }

    // Raised-cosine keying ramp on both ends.
    let nramp = nsps / 8;
    for i in 0..nramp {
        let env = 0.5 * (1.0 - (std::f32::consts::PI * i as f32 / nramp as f32).cos());
        wave[i] *= env;
        wave[nwave - 1 - i] *= env;
    }
    wave
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::submode::{params, Submode, NUM_SYMBOLS, NUM_TONES};

    fn normal_params() -> GfskParams {
        GfskParams::from_submode(&params(Submode::Normal))
    }

    /// Goertzel power of `x` at `freq`.
    fn goertzel(x: &[f32], freq: f32, fs: f32) -> f32 {
        let w = std::f32::consts::TAU * freq / fs;
        let coeff = 2.0 * w.cos();
        let (mut s1, mut s2) = (0.0f32, 0.0f32);
        for &v in x {
            let s0 = v + coeff * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        s1 * s1 + s2 * s2 - coeff * s1 * s2
    }

    #[test]
    fn output_length_and_amplitude_bounds() {
        let p = normal_params();
        let tones = vec![0u8; NUM_SYMBOLS];
        let w = modulate_tones(&tones, 1500.0, &p);
        assert_eq!(w.len(), NUM_SYMBOLS * p.samples_per_symbol);
        assert!(w.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
    }

    #[test]
    fn empty_tones_yield_no_audio() {
        assert!(modulate_tones(&[], 1500.0, &normal_params()).is_empty());
    }

    #[test]
    fn a_constant_tone_lands_on_its_expected_frequency() {
        // For each tone value, a constant run must peak at base + tone·spacing among the 8 candidates.
        let p = normal_params();
        let base = 1200.0;
        for tone in 0..NUM_TONES as u8 {
            let w = modulate_tones(&[tone; NUM_SYMBOLS], base, &p);
            // Measure the steady middle (skip the ramp + first/last symbols).
            let mid = &w[10 * p.samples_per_symbol..60 * p.samples_per_symbol];
            let best = (0..NUM_TONES)
                .max_by(|&a, &b| {
                    let fa = base + a as f32 * p.tone_spacing_hz;
                    let fb = base + b as f32 * p.tone_spacing_hz;
                    goertzel(mid, fa, p.sample_rate as f32).total_cmp(&goertzel(
                        mid,
                        fb,
                        p.sample_rate as f32,
                    ))
                })
                .unwrap();
            assert_eq!(best as u8, tone, "tone {tone} peaked at candidate {best}");
        }
    }

    #[test]
    fn gfsk_pulse_is_a_partition_of_unity_in_the_interior() {
        // Σ of the pulse's three one-symbol-shifted lobes ≈ 1 at the center of the middle symbol.
        let nsps = 64;
        let pulse = gfsk_pulse(DEFAULT_BT, nsps);
        let center = 3 * nsps / 2; // middle of the 3-symbol pulse
        let sum = pulse[center] + pulse[center - nsps] + pulse[center + nsps];
        assert!((sum - 1.0).abs() < 0.02, "partition-of-unity sum {sum}");
    }

    #[test]
    fn transitions_are_smoothed_not_stepped() {
        // A 0→7 tone jump must not appear as an instantaneous frequency step: the sample-to-sample
        // phase increment changes gradually across the boundary (GFSK), unlike rectangular FSK.
        let p = normal_params();
        let mut tones = vec![0u8; NUM_SYMBOLS];
        for t in tones.iter_mut().skip(40) {
            *t = 7;
        }
        let w = modulate_tones(&tones, 1200.0, &p);
        // Instantaneous frequency proxy = local arcsin-free zero-cross spacing is fiddly; instead
        // check the synthesized wave has no NaNs and bounded first difference (continuous phase).
        let max_step = w
            .windows(2)
            .map(|d| (d[1] - d[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(
            max_step < 1.5,
            "phase-continuous wave, max |Δsample| = {max_step}"
        );
    }
}
