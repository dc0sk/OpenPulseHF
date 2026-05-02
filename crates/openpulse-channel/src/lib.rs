//! Channel simulation models for OpenPulseHF.
//!
//! Provides the [`ChannelModel`] trait and configuration types for all channel
//! simulation backends used by the benchmark harness and the testbench GUI.
//! Concrete model implementations live in child modules added in Phase 1.4.

use thiserror::Error;

// ── Error type ────────────────────────────────────────────────────────────────

/// Errors produced by channel model construction and operation.
#[derive(Debug, Error)]
pub enum ChannelError {
    #[error("invalid channel parameter: {0}")]
    InvalidParameter(String),
    #[error("channel configuration error: {0}")]
    Config(String),
}

// ── Core trait ────────────────────────────────────────────────────────────────

/// A stateful channel simulation model.
///
/// Implementors apply signal distortion and/or additive noise to a block of
/// `f32` audio samples at the configured sample rate (8000 Hz).  Both methods
/// take `&mut self` because most models carry RNG or filter state.
pub trait ChannelModel: Send {
    /// Apply the full channel (signal distortion + additive noise) to a block.
    fn apply(&mut self, input: &[f32]) -> Vec<f32>;

    /// Generate the additive noise component alone, without an input signal.
    ///
    /// Used to populate the standalone Noise visualisation tap.  Multiplicative
    /// models (QSB, Watterson) return `vec![0.0; length]` here — fading is not
    /// independent additive noise.
    fn generate_noise(&mut self, length: usize) -> Vec<f32>;
}

// ── Config enums and stubs ────────────────────────────────────────────────────

/// AWGN channel: Gaussian noise at a fixed SNR.
#[derive(Debug, Clone, PartialEq)]
pub struct AwgnConfig {
    /// Signal-to-noise ratio in dB.
    pub snr_db: f32,
    /// RNG seed. `None` draws from thread entropy.
    pub seed: Option<u64>,
}

/// Gilbert-Elliott two-state Markov burst-error channel.
#[derive(Debug, Clone, PartialEq)]
pub struct GilbertElliottConfig {
    /// Transition probability from Good → Bad state.
    pub p_gb: f32,
    /// Transition probability from Bad → Good state.
    pub p_bg: f32,
    /// Bit-error rate in the Bad (burst) state.
    pub ber_bad: f32,
    /// Bit-error rate in the Good (gap) state.
    pub ber_good: f32,
    /// RNG seed. `None` draws from thread entropy.
    pub seed: Option<u64>,
}

/// Watterson two-ray ITU-R F.1487 ionospheric channel.
#[derive(Debug, Clone, PartialEq)]
pub struct WattersonConfig {
    /// Doppler spread in Hz (controls fading rate).
    pub doppler_hz: f32,
    /// Multipath delay spread in seconds.
    pub delay_spread_s: f32,
    /// Audio sample rate — must match the modem pipeline (8000 Hz).
    pub sample_rate: u32,
    /// RNG seed. `None` draws from thread entropy.
    pub seed: Option<u64>,
}

/// QRN (atmospheric noise) — Middleton Class-A impulsive noise model.
#[derive(Debug, Clone, PartialEq)]
pub struct QrnConfig {
    /// Impulsive index A (ratio of mean impulse rate to mean noise bandwidth).
    pub impulsive_index: f32,
    /// Mean power ratio of impulsive to Gaussian component.
    pub power_ratio: f32,
    /// RNG seed. `None` draws from thread entropy.
    pub seed: Option<u64>,
}

/// QRM (man-made interference) — phase-coherent discrete tones.
#[derive(Debug, Clone, PartialEq)]
pub struct QrmConfig {
    /// Centre frequencies of interfering carriers in Hz.
    pub frequencies_hz: Vec<f32>,
    /// Amplitude of each carrier (linear, relative to signal peak).
    pub amplitude: f32,
    /// Audio sample rate — must match the modem pipeline (8000 Hz).
    pub sample_rate: u32,
}

/// QSB (fading) — multiplicative sinusoidal amplitude envelope.
#[derive(Debug, Clone, PartialEq)]
pub struct QsbConfig {
    /// Fading rate in Hz (cycles per second of the envelope sinusoid).
    pub rate_hz: f32,
    /// Minimum envelope amplitude (0.0 = complete fade, 1.0 = no fade).
    pub depth: f32,
    /// Audio sample rate — must match the modem pipeline (8000 Hz).
    pub sample_rate: u32,
}

/// Chirp (linear frequency sweep) interference.
#[derive(Debug, Clone, PartialEq)]
pub struct ChirpConfig {
    /// Start frequency of the sweep in Hz.
    pub start_hz: f32,
    /// End frequency of the sweep in Hz.
    pub end_hz: f32,
    /// Duration of one sweep cycle in seconds.
    pub sweep_s: f32,
    /// Amplitude relative to signal peak.
    pub amplitude: f32,
    /// Audio sample rate — must match the modem pipeline (8000 Hz).
    pub sample_rate: u32,
}

/// Composite channel: series combination of zero or more models.
///
/// Each model in `stages` is applied in order: the output of stage N is the
/// input of stage N+1.
#[derive(Debug, Clone, PartialEq)]
pub struct CompositeConfig {
    pub stages: Vec<ChannelModelConfig>,
}

/// Union of all supported channel model configurations.
#[derive(Debug, Clone, PartialEq)]
pub enum ChannelModelConfig {
    Awgn(AwgnConfig),
    GilbertElliott(GilbertElliottConfig),
    Watterson(WattersonConfig),
    Qrn(QrnConfig),
    Qrm(QrmConfig),
    Qsb(QsbConfig),
    Chirp(ChirpConfig),
    Composite(CompositeConfig),
}

// ── Factory stub ──────────────────────────────────────────────────────────────

/// Construct a boxed [`ChannelModel`] from a configuration.
///
/// The `seed` parameter overrides any seed embedded in the config; pass `None`
/// to use per-config seeds (or thread entropy where no seed is set).
/// Concrete model implementations are added in Phase 1.4.
pub fn build_channel(
    _config: &ChannelModelConfig,
    _seed: Option<u64>,
) -> Result<Box<dyn ChannelModel>, ChannelError> {
    Err(ChannelError::Config(
        "channel models not yet implemented (Phase 1.4)".into(),
    ))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_channel_stub_returns_error() {
        let cfg = ChannelModelConfig::Awgn(AwgnConfig {
            snr_db: 10.0,
            seed: Some(42),
        });
        assert!(build_channel(&cfg, None).is_err());
    }

    #[test]
    fn channel_error_display() {
        let e = ChannelError::InvalidParameter("snr_db must be finite".into());
        assert!(e.to_string().contains("snr_db must be finite"));
    }
}
