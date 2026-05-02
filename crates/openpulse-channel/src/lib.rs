//! Channel simulation models for OpenPulseHF.
//!
//! Provides the [`ChannelModel`] trait and configuration types for all channel
//! simulation backends used by the benchmark harness and the testbench GUI.

use thiserror::Error;

pub mod awgn;
pub mod chirp;
pub mod composite;
pub mod dsp;
pub mod gilbert_elliott;
pub mod qrm;
pub mod qrn;
pub mod qsb;
pub mod watterson;

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

// ── Config types ──────────────────────────────────────────────────────────────

/// AWGN channel: Gaussian noise at a fixed SNR.
#[derive(Debug, Clone, PartialEq)]
pub struct AwgnConfig {
    /// Signal-to-noise ratio in dB.
    pub snr_db: f32,
    /// RNG seed. `None` draws from thread entropy.
    pub seed: Option<u64>,
}

impl AwgnConfig {
    pub fn new(snr_db: f32, seed: Option<u64>) -> Self {
        Self { snr_db, seed }
    }
}

/// Gilbert-Elliott two-state Markov burst-error channel.
#[derive(Debug, Clone, PartialEq)]
pub struct GilbertElliottConfig {
    /// Transition probability from Good → Bad state.
    pub p_gb: f32,
    /// Transition probability from Bad → Good state.
    pub p_bg: f32,
    /// SNR in the Good state (dB).
    pub snr_good_db: f32,
    /// SNR in the Bad state (dB).
    pub snr_bad_db: f32,
    /// RNG seed. `None` draws from thread entropy.
    pub seed: Option<u64>,
}

impl GilbertElliottConfig {
    /// Light burst: mean burst = 1/p_bg = 10 symbols.
    pub fn light(seed: Option<u64>) -> Self {
        Self {
            p_gb: 0.02,
            p_bg: 0.1,
            snr_good_db: 20.0,
            snr_bad_db: 3.0,
            seed,
        }
    }
    /// Moderate burst: mean burst = 1/p_bg = 20 symbols.
    pub fn moderate(seed: Option<u64>) -> Self {
        Self {
            p_gb: 0.02,
            p_bg: 0.05,
            snr_good_db: 20.0,
            snr_bad_db: 0.0,
            seed,
        }
    }
    /// Heavy burst: mean burst = 1/p_bg = 50 symbols.
    pub fn heavy(seed: Option<u64>) -> Self {
        Self {
            p_gb: 0.02,
            p_bg: 0.02,
            snr_good_db: 20.0,
            snr_bad_db: -3.0,
            seed,
        }
    }
    /// Severe burst: mean burst = 1/p_bg = 100 symbols.
    pub fn severe(seed: Option<u64>) -> Self {
        Self {
            p_gb: 0.02,
            p_bg: 0.01,
            snr_good_db: 20.0,
            snr_bad_db: -6.0,
            seed,
        }
    }
}

/// Watterson two-ray ITU-R F.1487 ionospheric channel.
#[derive(Debug, Clone, PartialEq)]
pub struct WattersonConfig {
    /// Doppler spread in Hz (controls fading rate).
    pub doppler_spread_hz: f32,
    /// Multipath delay spread in milliseconds.
    pub delay_spread_ms: f32,
    /// Overall SNR in dB.
    pub snr_db: f32,
    /// RNG seed. `None` draws from thread entropy.
    pub seed: Option<u64>,
    /// Audio sample rate — must match the modem pipeline.
    pub sample_rate: u32,
}

impl WattersonConfig {
    /// ITU-R F.1487 Good F1: Doppler 0.1 Hz, delay 0.5 ms.
    pub fn good_f1(seed: Option<u64>) -> Self {
        Self {
            doppler_spread_hz: 0.1,
            delay_spread_ms: 0.5,
            snr_db: 20.0,
            seed,
            sample_rate: 8000,
        }
    }
    /// ITU-R F.1487 Good F2: Doppler 0.5 Hz, delay 1.0 ms.
    pub fn good_f2(seed: Option<u64>) -> Self {
        Self {
            doppler_spread_hz: 0.5,
            delay_spread_ms: 1.0,
            snr_db: 15.0,
            seed,
            sample_rate: 8000,
        }
    }
    /// ITU-R F.1487 Moderate F1: Doppler 1.0 Hz, delay 1.0 ms.
    pub fn moderate_f1(seed: Option<u64>) -> Self {
        Self {
            doppler_spread_hz: 1.0,
            delay_spread_ms: 1.0,
            snr_db: 10.0,
            seed,
            sample_rate: 8000,
        }
    }
    /// ITU-R F.1487 Moderate F2: Doppler 1.0 Hz, delay 2.0 ms.
    pub fn moderate_f2(seed: Option<u64>) -> Self {
        Self {
            doppler_spread_hz: 1.0,
            delay_spread_ms: 2.0,
            snr_db: 10.0,
            seed,
            sample_rate: 8000,
        }
    }
    /// ITU-R F.1487 Poor F1: Doppler 2.0 Hz, delay 2.0 ms.
    pub fn poor_f1(seed: Option<u64>) -> Self {
        Self {
            doppler_spread_hz: 2.0,
            delay_spread_ms: 2.0,
            snr_db: 5.0,
            seed,
            sample_rate: 8000,
        }
    }
    /// ITU-R F.1487 Poor F2: Doppler 2.0 Hz, delay 5.0 ms.
    pub fn poor_f2(seed: Option<u64>) -> Self {
        Self {
            doppler_spread_hz: 2.0,
            delay_spread_ms: 5.0,
            snr_db: 3.0,
            seed,
            sample_rate: 8000,
        }
    }
    /// Extreme: Doppler 10.0 Hz, delay 10.0 ms.
    pub fn extreme(seed: Option<u64>) -> Self {
        Self {
            doppler_spread_hz: 10.0,
            delay_spread_ms: 10.0,
            snr_db: 0.0,
            seed,
            sample_rate: 8000,
        }
    }
}

/// QRN (atmospheric noise) — Middleton Class-A impulsive noise model.
#[derive(Debug, Clone, PartialEq)]
pub struct QrnConfig {
    /// Background Gaussian noise SNR in dB.
    pub gaussian_snr_db: f32,
    /// Mean impulse arrival rate in Hz.
    pub impulse_rate_hz: f32,
    /// Amplitude ratio of impulse spikes to background RMS.
    pub impulse_amplitude_ratio: f32,
    /// Maximum spike duration in samples.
    pub max_spike_duration_samples: u8,
    /// Audio sample rate — must match the modem pipeline (8000 Hz).
    pub sample_rate: u32,
    /// RNG seed. `None` draws from thread entropy.
    pub seed: Option<u64>,
}

/// A single tone for QRM interference.
#[derive(Debug, Clone, PartialEq)]
pub struct ToneConfig {
    /// Centre frequency of the interfering carrier in Hz.
    pub frequency_hz: f32,
    /// Amplitude relative to signal peak (linear).
    pub amplitude: f32,
}

/// QRM (man-made interference) — phase-coherent discrete tones.
#[derive(Debug, Clone, PartialEq)]
pub struct QrmConfig {
    /// Interfering tone list.
    pub tones: Vec<ToneConfig>,
    /// Background noise floor SNR in dB. `None` → no background noise.
    pub noise_floor_snr_db: Option<f32>,
    /// Audio sample rate — must match the modem pipeline (8000 Hz).
    pub sample_rate: u32,
    /// RNG seed for background noise component. `None` draws from thread entropy.
    pub seed: Option<u64>,
}

/// QSB (fading) — multiplicative sinusoidal amplitude envelope.
#[derive(Debug, Clone, PartialEq)]
pub struct QsbConfig {
    /// Fading rate in Hz (cycles per second of the envelope sinusoid).
    pub fade_rate_hz: f32,
    /// Minimum envelope amplitude (0.0 = complete fade, 1.0 = no fade).
    pub fade_depth: f32,
    /// Audio sample rate — must match the modem pipeline (8000 Hz).
    pub sample_rate: u32,
}

/// Chirp (linear frequency sweep) interference.
#[derive(Debug, Clone, PartialEq)]
pub struct ChirpConfig {
    /// Start frequency of the sweep in Hz.
    pub f_start_hz: f32,
    /// End frequency of the sweep in Hz.
    pub f_end_hz: f32,
    /// Duration of one sweep cycle in seconds.
    pub period_s: f32,
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

// ── Factory ───────────────────────────────────────────────────────────────────

/// Construct a boxed [`ChannelModel`] from a configuration.
///
/// The `seed` parameter overrides any seed embedded in the config; pass `None`
/// to use per-config seeds (or thread entropy where no seed is set).
pub fn build_channel(
    config: &ChannelModelConfig,
    seed: Option<u64>,
) -> Result<Box<dyn ChannelModel>, ChannelError> {
    match config {
        ChannelModelConfig::Awgn(cfg) => {
            let mut c = cfg.clone();
            if seed.is_some() {
                c.seed = seed;
            }
            Ok(Box::new(awgn::AwgnChannel::new(c)?))
        }
        ChannelModelConfig::GilbertElliott(cfg) => {
            let mut c = cfg.clone();
            if seed.is_some() {
                c.seed = seed;
            }
            Ok(Box::new(gilbert_elliott::GilbertElliottChannel::new(c)?))
        }
        ChannelModelConfig::Watterson(cfg) => {
            let mut c = cfg.clone();
            if seed.is_some() {
                c.seed = seed;
            }
            Ok(Box::new(watterson::WattersonChannel::new(c)?))
        }
        ChannelModelConfig::Qrn(cfg) => {
            let mut c = cfg.clone();
            if seed.is_some() {
                c.seed = seed;
            }
            Ok(Box::new(qrn::QrnChannel::new(c)?))
        }
        ChannelModelConfig::Qrm(cfg) => {
            let mut c = cfg.clone();
            if seed.is_some() {
                c.seed = seed;
            }
            Ok(Box::new(qrm::QrmChannel::new(c)?))
        }
        ChannelModelConfig::Qsb(cfg) => Ok(Box::new(qsb::QsbChannel::new(cfg.clone())?)),
        ChannelModelConfig::Chirp(cfg) => Ok(Box::new(chirp::ChirpChannel::new(cfg.clone())?)),
        ChannelModelConfig::Composite(cfg) => {
            Ok(Box::new(composite::CompositeChannel::build(cfg, seed)?))
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_channel_awgn_ok() {
        let cfg = ChannelModelConfig::Awgn(AwgnConfig {
            snr_db: 10.0,
            seed: Some(42),
        });
        assert!(build_channel(&cfg, None).is_ok());
    }

    #[test]
    fn channel_error_display() {
        let e = ChannelError::InvalidParameter("snr_db must be finite".into());
        assert!(e.to_string().contains("snr_db must be finite"));
    }
}
