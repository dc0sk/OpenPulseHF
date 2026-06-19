use openpulse_channel::{
    AwgnConfig, ChannelModel, ChannelModelConfig, ChirpConfig, GilbertElliottConfig, QrmConfig,
    QrnConfig, QsbConfig, ToneConfig, WattersonConfig,
};

use crate::matrix::{ChannelSpec, Tier};

/// AWGN SNR levels for the systematic sweep (full tier).
pub const SNR_SWEEP_DB: &[f32] = &[0.0, 3.0, 5.0, 8.0, 10.0, 12.0, 15.0, 20.0, 25.0, 30.0];

/// Quick-tier AWGN levels: clean + two reference points.
pub const SNR_QUICK_DB: &[f32] = &[20.0, 10.0];

pub fn channel_suite(tier: Tier) -> Vec<ChannelSpec> {
    let mut channels = vec![ChannelSpec::Clean];

    // AWGN sweep
    let snr_levels = if tier == Tier::Full {
        SNR_SWEEP_DB
    } else {
        SNR_QUICK_DB
    };
    for &snr_db in snr_levels {
        channels.push(ChannelSpec::Awgn { snr_db, seed: 42 });
    }

    if tier == Tier::Full {
        channels.extend([
            ChannelSpec::WattersonGoodF1,
            ChannelSpec::WattersonGoodF2,
            ChannelSpec::WattersonModerateF1,
            ChannelSpec::WattersonPoorF1,
            ChannelSpec::WattersonExtreme,
            ChannelSpec::GilbertElliottLight,
            ChannelSpec::GilbertElliottModerate,
            ChannelSpec::GilbertElliottHeavy,
            ChannelSpec::GilbertElliottSevere,
            ChannelSpec::QrnLight,
            ChannelSpec::QrmTone,
            ChannelSpec::QsbSlow,
            ChannelSpec::ChirpSlow,
        ]);
    }
    channels
}

/// Instantiate a boxed ChannelModel from a ChannelSpec.
pub fn build(spec: &ChannelSpec) -> Box<dyn ChannelModel> {
    match spec {
        ChannelSpec::Clean => Box::new(CleanChannel),
        ChannelSpec::Awgn { snr_db, seed } => {
            let cfg = AwgnConfig::new(*snr_db, Some(*seed));
            openpulse_channel::build_channel(&ChannelModelConfig::Awgn(cfg), None).unwrap()
        }
        ChannelSpec::WattersonGoodF1 => {
            let cfg = WattersonConfig::good_f1(Some(1));
            openpulse_channel::build_channel(&ChannelModelConfig::Watterson(cfg), None).unwrap()
        }
        ChannelSpec::WattersonGoodF2 => {
            let cfg = WattersonConfig::good_f2(Some(2));
            openpulse_channel::build_channel(&ChannelModelConfig::Watterson(cfg), None).unwrap()
        }
        ChannelSpec::WattersonGoodF1Snr { snr_db, seed } => {
            let mut cfg = WattersonConfig::good_f1(Some(*seed));
            cfg.snr_db = *snr_db;
            openpulse_channel::build_channel(&ChannelModelConfig::Watterson(cfg), None).unwrap()
        }
        ChannelSpec::WattersonGoodF2Snr { snr_db, seed } => {
            let mut cfg = WattersonConfig::good_f2(Some(*seed));
            cfg.snr_db = *snr_db;
            openpulse_channel::build_channel(&ChannelModelConfig::Watterson(cfg), None).unwrap()
        }
        ChannelSpec::WattersonModerateF1 => {
            let cfg = WattersonConfig::moderate_f1(Some(3));
            openpulse_channel::build_channel(&ChannelModelConfig::Watterson(cfg), None).unwrap()
        }
        ChannelSpec::WattersonPoorF1 => {
            let cfg = WattersonConfig::poor_f1(Some(4));
            openpulse_channel::build_channel(&ChannelModelConfig::Watterson(cfg), None).unwrap()
        }
        ChannelSpec::WattersonExtreme => {
            let cfg = WattersonConfig::extreme(Some(5));
            openpulse_channel::build_channel(&ChannelModelConfig::Watterson(cfg), None).unwrap()
        }
        ChannelSpec::GilbertElliottLight => {
            let cfg = GilbertElliottConfig::light(Some(10));
            openpulse_channel::build_channel(&ChannelModelConfig::GilbertElliott(cfg), None)
                .unwrap()
        }
        ChannelSpec::GilbertElliottModerate => {
            let cfg = GilbertElliottConfig::moderate(Some(11));
            openpulse_channel::build_channel(&ChannelModelConfig::GilbertElliott(cfg), None)
                .unwrap()
        }
        ChannelSpec::GilbertElliottHeavy => {
            let cfg = GilbertElliottConfig::heavy(Some(12));
            openpulse_channel::build_channel(&ChannelModelConfig::GilbertElliott(cfg), None)
                .unwrap()
        }
        ChannelSpec::GilbertElliottSevere => {
            let cfg = GilbertElliottConfig::severe(Some(13));
            openpulse_channel::build_channel(&ChannelModelConfig::GilbertElliott(cfg), None)
                .unwrap()
        }
        ChannelSpec::QrnLight => {
            let cfg = QrnConfig {
                gaussian_snr_db: 20.0,
                impulse_rate_hz: 10.0,
                impulse_amplitude_ratio: 5.0,
                max_spike_duration_samples: 8,
                sample_rate: 8000,
                seed: Some(20),
            };
            openpulse_channel::build_channel(&ChannelModelConfig::Qrn(cfg), None).unwrap()
        }
        ChannelSpec::QrmTone => {
            let cfg = QrmConfig {
                tones: vec![ToneConfig {
                    frequency_hz: 900.0,
                    amplitude: 0.3,
                }],
                noise_floor_snr_db: None,
                sample_rate: 8000,
                seed: None,
            };
            openpulse_channel::build_channel(&ChannelModelConfig::Qrm(cfg), None).unwrap()
        }
        ChannelSpec::QsbSlow => {
            let cfg = QsbConfig {
                fade_rate_hz: 0.5,
                fade_depth: 0.4,
                sample_rate: 8000,
            };
            openpulse_channel::build_channel(&ChannelModelConfig::Qsb(cfg), None).unwrap()
        }
        ChannelSpec::ChirpSlow => {
            let cfg = ChirpConfig {
                f_start_hz: 400.0,
                f_end_hz: 2400.0,
                period_s: 5.0,
                amplitude: 0.2,
                sample_rate: 8000,
            };
            openpulse_channel::build_channel(&ChannelModelConfig::Chirp(cfg), None).unwrap()
        }
    }
}

/// Passthrough channel (no distortion).
struct CleanChannel;

impl ChannelModel for CleanChannel {
    fn apply(&mut self, input: &[f32]) -> Vec<f32> {
        input.to_vec()
    }
    fn generate_noise(&mut self, length: usize) -> Vec<f32> {
        vec![0.0f32; length]
    }
}
