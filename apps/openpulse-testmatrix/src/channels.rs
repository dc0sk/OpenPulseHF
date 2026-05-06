use openpulse_channel::{
    AwgnConfig, ChannelModel, ChannelModelConfig, ChirpConfig, GilbertElliottConfig, QrmConfig,
    QrnConfig, QsbConfig, ToneConfig, WattersonConfig,
};

use crate::matrix::{ChannelSpec, Tier};

pub fn channel_suite(tier: Tier) -> Vec<ChannelSpec> {
    let mut channels = vec![
        ChannelSpec::Clean,
        ChannelSpec::Awgn {
            snr_db: 20.0,
            seed: 42,
        },
        ChannelSpec::Awgn {
            snr_db: 10.0,
            seed: 42,
        },
    ];
    if tier == Tier::Full {
        channels.extend([
            ChannelSpec::Awgn {
                snr_db: 5.0,
                seed: 42,
            },
            ChannelSpec::Awgn {
                snr_db: 0.0,
                seed: 42,
            },
            ChannelSpec::WattersonGoodF1,
            ChannelSpec::WattersonGoodF2,
            ChannelSpec::WattersonModerateF1,
            ChannelSpec::WattersonPoorF1,
            ChannelSpec::GilbertElliottLight,
            ChannelSpec::GilbertElliottModerate,
            ChannelSpec::GilbertElliottHeavy,
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
        ChannelSpec::WattersonModerateF1 => {
            let cfg = WattersonConfig::moderate_f1(Some(3));
            openpulse_channel::build_channel(&ChannelModelConfig::Watterson(cfg), None).unwrap()
        }
        ChannelSpec::WattersonPoorF1 => {
            let cfg = WattersonConfig::poor_f1(Some(4));
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
