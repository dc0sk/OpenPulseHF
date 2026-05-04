use std::sync::{Arc, RwLock};

use bpsk_plugin::BpskPlugin;
use openpulse_channel::dsp::PowerSpectrum;
use openpulse_channel::{
    build_channel, AwgnConfig, ChannelModelConfig, ChirpConfig, GilbertElliottConfig, QrmConfig,
    QrnConfig, QsbConfig, ToneConfig, WattersonConfig,
};
use openpulse_core::fec::FecCodec;
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use qpsk_plugin::QpskPlugin;

use crate::state::{AppConfig, NoiseModel, Tap, TestStats};

const PAYLOAD: &[u8] = b"OpenPulseHF testbench v0.1 test!";

pub fn spawn_signal_thread(
    config: AppConfig,
    taps: [Tap; 4],
    stats: Arc<RwLock<TestStats>>,
    stop_rx: crossbeam_channel::Receiver<()>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || run(config, taps, stats, stop_rx))
}

fn run(
    config: AppConfig,
    taps: [Tap; 4],
    stats: Arc<RwLock<TestStats>>,
    stop_rx: crossbeam_channel::Receiver<()>,
) {
    let plugin: Box<dyn ModulationPlugin> = if config.mode.starts_with("BPSK") {
        Box::new(BpskPlugin::new())
    } else {
        Box::new(QpskPlugin::new())
    };
    let mod_config = ModulationConfig {
        mode: config.mode.clone(),
        center_frequency: 1500.0,
        sample_rate: 8000,
    };
    let fec = config.fec_enabled.then(FecCodec::new);
    let seed = config.seed_str.parse::<u64>().ok();
    let channel_config = make_channel_config(&config);
    let mut channel = match build_channel(&channel_config, seed) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("failed to build channel model: {e}");
            return;
        }
    };

    let mut ps = [
        PowerSpectrum::new(),
        PowerSpectrum::new(),
        PowerSpectrum::new(),
        PowerSpectrum::new(),
    ];
    let min_db = config.min_db;
    let max_db = config.max_db;

    loop {
        // Pace the loop to ~50 iterations/s; also serves as the stop-check interval.
        if stop_rx
            .recv_timeout(std::time::Duration::from_millis(20))
            .is_ok()
        {
            break;
        }

        let tx_payload = match &fec {
            Some(codec) => codec.encode(PAYLOAD),
            None => PAYLOAD.to_vec(),
        };

        let tx_samples = match plugin.modulate(&tx_payload, &mod_config) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("modulate error: {e}");
                continue;
            }
        };
        push_tap(&taps[0], &mut ps[0], &tx_samples, min_db, max_db);

        // Noise tap: additive component only (zeros for multiplicative models).
        let noise_samples = channel.generate_noise(tx_samples.len());
        push_tap(&taps[1], &mut ps[1], &noise_samples, min_db, max_db);

        // Mixed tap: full channel output (fading + additive noise for all model types).
        let mixed = channel.apply(&tx_samples);
        push_tap(&taps[2], &mut ps[2], &mixed, min_db, max_db);

        let rx_result = plugin.demodulate(&mixed, &mod_config);
        let rx_samples = match &rx_result {
            Ok(decoded) => {
                let base = match &fec {
                    Some(codec) => codec.decode(decoded).unwrap_or_else(|_| decoded.clone()),
                    None => decoded.clone(),
                };
                plugin
                    .modulate(&base, &mod_config)
                    .unwrap_or_else(|_| vec![0.0_f32; tx_samples.len()])
            }
            Err(_) => vec![0.0_f32; tx_samples.len()],
        };
        push_tap(&taps[3], &mut ps[3], &rx_samples, min_db, max_db);

        update_stats(&stats, &rx_result, &fec, PAYLOAD.len());
    }
}

fn push_tap(tap: &Tap, ps: &mut PowerSpectrum, samples: &[f32], min_db: f32, max_db: f32) {
    tap.write()
        .unwrap()
        .push_samples(ps, samples, min_db, max_db);
}

fn update_stats(
    stats: &Arc<RwLock<TestStats>>,
    rx_result: &Result<Vec<u8>, openpulse_core::error::ModemError>,
    fec: &Option<FecCodec>,
    payload_len: usize,
) {
    let n_bits = payload_len * 8;
    let error_bits: u64 = match rx_result {
        Ok(decoded) => {
            let plain = match fec {
                Some(codec) => codec.decode(decoded).unwrap_or_else(|_| decoded.clone()),
                None => decoded.clone(),
            };
            count_bit_errors(PAYLOAD, &plain)
        }
        Err(_) => n_bits as u64,
    };

    let mut s = stats.write().unwrap();
    s.runs += 1;
    s.total_bits += n_bits as u64;
    s.error_bits += error_bits;
    if rx_result.is_ok() && error_bits == 0 {
        s.ok += 1;
    } else {
        s.fail += 1;
        if s.fail <= 10 || s.fail.is_multiple_of(100) {
            let msg = format!("Run {}: fail ({error_bits} bit errors)", s.runs);
            s.push_event(msg);
        }
    }
}

fn count_bit_errors(a: &[u8], b: &[u8]) -> u64 {
    let len = a.len().min(b.len());
    let tail = if a.len() > b.len() {
        (a.len() - b.len()) * 8
    } else {
        (b.len() - a.len()) * 8
    };
    a[..len]
        .iter()
        .zip(b[..len].iter())
        .map(|(x, y)| (x ^ y).count_ones() as u64)
        .sum::<u64>()
        + tail as u64
}

fn make_channel_config(config: &AppConfig) -> ChannelModelConfig {
    let snr = config.snr_db;
    match &config.noise_model {
        NoiseModel::Awgn => ChannelModelConfig::Awgn(AwgnConfig {
            snr_db: snr,
            seed: None,
        }),
        NoiseModel::GilbertElliott => {
            let mut cfg = GilbertElliottConfig::moderate(None);
            // Map SNR slider: keep the Good/Bad gap (15 dB) but honour the slider level.
            cfg.snr_good_db = snr;
            cfg.snr_bad_db = snr - 15.0;
            ChannelModelConfig::GilbertElliott(cfg)
        }
        NoiseModel::Watterson => {
            let mut cfg = WattersonConfig::moderate_f1(None);
            cfg.snr_db = snr;
            ChannelModelConfig::Watterson(cfg)
        }
        NoiseModel::Qrn => ChannelModelConfig::Qrn(QrnConfig {
            gaussian_snr_db: snr,
            impulse_rate_hz: 5.0,
            impulse_amplitude_ratio: 8.0,
            max_spike_duration_samples: 3,
            sample_rate: 8000,
            seed: None,
        }),
        NoiseModel::Qrm => ChannelModelConfig::Qrm(QrmConfig {
            tones: vec![ToneConfig {
                frequency_hz: 1500.0,
                amplitude: 0.5,
            }],
            noise_floor_snr_db: Some(snr),
            sample_rate: 8000,
            seed: None,
        }),
        NoiseModel::Qsb => ChannelModelConfig::Qsb(QsbConfig {
            fade_rate_hz: 0.3,
            fade_depth: 0.5,
            sample_rate: 8000,
        }),
        NoiseModel::Chirp => ChannelModelConfig::Chirp(ChirpConfig {
            f_start_hz: 300.0,
            f_end_hz: 3000.0,
            period_s: 2.0,
            amplitude: 0.5,
            sample_rate: 8000,
        }),
    }
}
