//! Capture-side AGC and fixed capture gain.
//!
//! Two impairments that live in the RECEIVER's capture chain rather than on the radio path, and
//! that between them account for several days of misdiagnosis in this project:
//!
//! * **Fixed capture gain.** The absolute level a receiver hands the modem is set by the rig's USB
//!   audio output and the host mixer, not by the channel. It matters because the engine's
//!   `EnergyGate` compares against *absolute* thresholds: measured on air 2026-07-28, an idle floor
//!   of mean-square 0.0154 sat 4.7x above the gate's maximum threshold, so the gate could never
//!   shut, fired on noise, and settled AFC on it. Every in-process test ran at a normalised level
//!   where that is impossible.
//!
//! * **Capture AGC.** A soundcard/rig AGC moves the gain *during* a frame. It is near-harmless to a
//!   phase-only waveform and destructive to one carrying bits in amplitude — which is exactly why
//!   the failure set once looked like a waveform property and "the amplitude modes fail, the phase
//!   modes pass" read as physics. It was really a live mixer AGC (see the dual-card lesson in
//!   `CLAUDE.md`); eight modes were wrongly classified "analog-path limited" before it was ablated.
//!
//! Both are multiplicative: [`generate_noise`](AgcChannel::generate_noise) returns zeros.

use crate::{ChannelError, ChannelModel};

/// Capture-chain gain configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct AgcConfig {
    /// RMS the AGC drives the signal toward. The loop is disabled when this is `None`, leaving a
    /// pure fixed gain — the level-only impairment.
    pub target_rms: Option<f32>,
    /// Fixed gain applied before the loop. Use alone (with `target_rms: None`) to reproduce a
    /// capture that is simply too hot or too quiet for the energy gate's absolute thresholds.
    pub fixed_gain: f32,
    /// Seconds for the loop to react to a level INCREASE. Short attack = an AGC that clamps down
    /// hard at the start of a burst, which is what mangles an amplitude-modulated frame.
    pub attack_secs: f32,
    /// Seconds for the loop to recover on a level DECREASE.
    pub decay_secs: f32,
    /// Audio sample rate, Hz.
    pub sample_rate: f32,
}

impl AgcConfig {
    /// A pure fixed capture gain, no AGC loop.
    pub fn fixed(gain: f32) -> Self {
        Self {
            target_rms: None,
            fixed_gain: gain,
            attack_secs: 0.01,
            decay_secs: 0.2,
            sample_rate: 8_000.0,
        }
    }

    /// A live capture AGC driving toward `target_rms`.
    pub fn agc(target_rms: f32, attack_secs: f32, decay_secs: f32, sample_rate: f32) -> Self {
        Self {
            target_rms: Some(target_rms),
            fixed_gain: 1.0,
            attack_secs,
            decay_secs,
            sample_rate,
        }
    }
}

/// Applies a fixed capture gain and, optionally, a tracking AGC loop.
pub struct AgcChannel {
    cfg: AgcConfig,
    /// Envelope estimate, carried across blocks so a block-wise application behaves like a stream.
    env: f64,
    gain: f64,
}

impl AgcChannel {
    pub fn new(cfg: AgcConfig) -> Result<Self, ChannelError> {
        if !cfg.fixed_gain.is_finite() || cfg.fixed_gain < 0.0 {
            return Err(ChannelError::InvalidParameter(
                "fixed_gain must be finite and non-negative".into(),
            ));
        }
        if let Some(t) = cfg.target_rms {
            if !(t.is_finite() && t > 0.0) {
                return Err(ChannelError::InvalidParameter(
                    "target_rms must be finite and positive".into(),
                ));
            }
        }
        if !(cfg.sample_rate.is_finite() && cfg.sample_rate > 0.0) {
            return Err(ChannelError::InvalidParameter(
                "sample_rate must be finite and positive".into(),
            ));
        }
        let attack_ok = cfg.attack_secs.is_finite() && cfg.attack_secs > 0.0;
        let decay_ok = cfg.decay_secs.is_finite() && cfg.decay_secs > 0.0;
        if !(attack_ok && decay_ok) {
            return Err(ChannelError::InvalidParameter(
                "attack_secs and decay_secs must be finite and positive".into(),
            ));
        }
        Ok(Self {
            cfg,
            env: 0.0,
            gain: 1.0,
        })
    }

    /// The AGC's current gain — lets a test show the gain actually MOVED during a burst rather
    /// than assuming the loop ran.
    pub fn current_gain(&self) -> f32 {
        self.gain as f32
    }
}

impl ChannelModel for AgcChannel {
    fn apply(&mut self, input: &[f32]) -> Vec<f32> {
        let fixed = self.cfg.fixed_gain as f64;
        let Some(target) = self.cfg.target_rms else {
            return input.iter().map(|&s| (s as f64 * fixed) as f32).collect();
        };
        let target = target as f64;
        let fs = self.cfg.sample_rate as f64;
        let a_att = (-1.0 / (self.cfg.attack_secs as f64 * fs)).exp();
        let a_dec = (-1.0 / (self.cfg.decay_secs as f64 * fs)).exp();

        let mut out = Vec::with_capacity(input.len());
        for &s in input {
            let x = s as f64 * fixed;
            // One-pole envelope follower on |x|, with distinct attack and decay.
            let mag = x.abs();
            let a = if mag > self.env { a_att } else { a_dec };
            self.env = a * self.env + (1.0 - a) * mag;
            // Drive the envelope toward the target. Guard the silent case so idle noise does not
            // produce unbounded gain.
            if self.env > 1e-9 {
                self.gain = (target / self.env).clamp(1e-3, 1e3);
            }
            out.push((x * self.gain) as f32);
        }
        out
    }

    fn generate_noise(&mut self, length: usize) -> Vec<f32> {
        vec![0.0; length]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
    }

    /// A fixed gain must scale the level by exactly that factor — this is the knob that reproduces
    /// a capture too hot for the energy gate's absolute thresholds.
    #[test]
    fn a_fixed_gain_scales_the_level_exactly() {
        let x: Vec<f32> = (0..4096)
            .map(|i| (std::f32::consts::TAU * 1500.0 * i as f32 / 8000.0).sin() * 0.1)
            .collect();
        for g in [0.25f32, 4.0] {
            let mut ch = AgcChannel::new(AgcConfig::fixed(g)).unwrap();
            let y = ch.apply(&x);
            let ratio = rms(&y) / rms(&x);
            assert!(
                (ratio - g).abs() < 1e-3,
                "fixed gain {g} produced a level ratio of {ratio}"
            );
        }
    }

    /// THE self-validation: an AGC must actually MOVE its gain when the input level jumps. A loop
    /// that silently sat at unity would make every "AGC breaks amplitude modes" test vacuous.
    #[test]
    fn the_agc_gain_moves_when_the_level_jumps() {
        let fs = 8000.0;
        // Quiet for 0.25 s, then 10x louder — a burst arriving after idle, as on a real receiver.
        let mut x: Vec<f32> = (0..2000)
            .map(|i| (std::f32::consts::TAU * 1000.0 * i as f32 / fs).sin() * 0.02)
            .collect();
        x.extend((0..8000).map(|i| (std::f32::consts::TAU * 1000.0 * i as f32 / fs).sin() * 0.2));

        let mut ch = AgcChannel::new(AgcConfig::agc(0.1, 0.02, 0.3, fs)).unwrap();
        let y = ch.apply(&x);

        let quiet_gain_region = rms(&y[500..1800]) / rms(&x[500..1800]);
        let loud_gain_region = rms(&y[6000..9500]) / rms(&x[6000..9500]);
        assert!(
            loud_gain_region < quiet_gain_region * 0.6,
            "the AGC did not pull its gain down on a 10x level jump \
             (quiet region gain {quiet_gain_region:.2}, loud region gain {loud_gain_region:.2})"
        );
    }

    /// The consequence that matters: an AGC flattens amplitude structure. A signal whose envelope
    /// carries information comes out with that envelope compressed — which is precisely why the
    /// amplitude-bearing modes failed on a rig with a live capture AGC while phase-only modes did
    /// not.
    #[test]
    fn the_agc_compresses_amplitude_structure() {
        let fs = 8000.0;
        // Two-level amplitude signal: the envelope IS the information.
        let x: Vec<f32> = (0..16000)
            .map(|i| {
                let amp = if (i / 2000) % 2 == 0 { 0.05 } else { 0.4 };
                (std::f32::consts::TAU * 1000.0 * i as f32 / fs).sin() * amp
            })
            .collect();
        let mut ch = AgcChannel::new(AgcConfig::agc(0.1, 0.01, 0.05, fs)).unwrap();
        let y = ch.apply(&x);

        let ratio_in = rms(&x[3000..3900]) / rms(&x[1000..1900]);
        let ratio_out = rms(&y[3000..3900]) / rms(&y[1000..1900]);
        assert!(
            ratio_out < ratio_in * 0.75,
            "the AGC left the amplitude ratio essentially intact (in {ratio_in:.2}, \
             out {ratio_out:.2}) — it is not compressing, so it cannot reproduce the failure mode"
        );
    }

    #[test]
    fn generate_noise_is_silent() {
        let mut ch = AgcChannel::new(AgcConfig::fixed(2.0)).unwrap();
        assert!(ch.generate_noise(32).iter().all(|&s| s == 0.0));
    }

    #[test]
    fn invalid_parameters_are_rejected() {
        assert!(AgcChannel::new(AgcConfig::fixed(f32::NAN)).is_err());
        assert!(AgcChannel::new(AgcConfig::agc(0.0, 0.01, 0.1, 8000.0)).is_err());
        assert!(AgcChannel::new(AgcConfig::agc(0.1, 0.0, 0.1, 8000.0)).is_err());
    }
}
