use std::sync::{Arc, RwLock};

use bpsk_plugin::BpskPlugin;
use fsk4_plugin::Fsk4Plugin;
use openpulse_channel::dsp::PowerSpectrum;
use openpulse_channel::{
    build_channel, AwgnConfig, ChannelModelConfig, ChirpConfig, GilbertElliottConfig, QrmConfig,
    QrnConfig, QsbConfig, ToneConfig, WattersonConfig,
};
use openpulse_core::compression::{compress_if_smaller, decompress, CompressionAlgorithm};
use openpulse_core::fec::FecCodec;
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use psk8_plugin::Psk8Plugin;
use qpsk_plugin::QpskPlugin;

#[cfg(feature = "cpal")]
use crate::state::AudioSource;
use crate::state::{AppConfig, NoiseModel, Tap, TestStats};

/// Base pattern repeated to fill the configured payload size.
const PATTERN: &[u8] = b"OpenPulseHF testbench v0.1 test!";

/// Build a payload of `size` bytes with a repeating 8-byte seed derived from `run` XORed
/// across every byte, so each frame has a distinct bit pattern and the spectrum animates.
fn make_payload(size: usize, run: u64) -> Vec<u8> {
    let seed = run.to_le_bytes();
    PATTERN
        .iter()
        .cloned()
        .cycle()
        .take(size)
        .enumerate()
        .map(|(i, b)| b ^ seed[i % seed.len()])
        .collect()
}

pub fn spawn_signal_thread(
    config: Arc<RwLock<AppConfig>>,
    taps: [Tap; 4],
    stats: Arc<RwLock<TestStats>>,
    stop_rx: crossbeam_channel::Receiver<()>,
) -> std::thread::JoinHandle<()> {
    #[cfg(feature = "cpal")]
    if config.read().unwrap().audio_source == AudioSource::LiveCapture {
        return std::thread::spawn(move || run_live(config, taps, stats, stop_rx));
    }
    std::thread::spawn(move || run(config, taps, stats, stop_rx))
}

fn make_plugin(mode: &str) -> Box<dyn ModulationPlugin> {
    if mode.starts_with("BPSK") {
        Box::new(BpskPlugin::new())
    } else if mode.starts_with("8PSK") {
        Box::new(Psk8Plugin::new())
    } else if mode == "FSK4-ACK" {
        Box::new(Fsk4Plugin::new())
    } else {
        Box::new(QpskPlugin::new())
    }
}

fn make_mod_config(config: &AppConfig) -> ModulationConfig {
    ModulationConfig {
        mode: config.mode.clone(),
        center_frequency: 1500.0,
        sample_rate: 8000,
        ..ModulationConfig::default()
    }
}

fn run(
    config: Arc<RwLock<AppConfig>>,
    taps: [Tap; 4],
    stats: Arc<RwLock<TestStats>>,
    stop_rx: crossbeam_channel::Receiver<()>,
) {
    let mut current = config.read().unwrap().clone();
    let mut plugin = make_plugin(&current.mode);
    let mut mod_config = make_mod_config(&current);
    let mut fec = current.fec_enabled.then(FecCodec::new);

    let seed = current.seed_str.parse::<u64>().ok();
    let channel_config = make_channel_config(&current);
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
    let mut run_count: u64 = 0;

    loop {
        if stop_rx
            .recv_timeout(std::time::Duration::from_millis(20))
            .is_ok()
        {
            break;
        }

        // Apply any config changes from the UI.
        let new_cfg = config.read().unwrap().clone();
        if new_cfg != current {
            if new_cfg.mode != current.mode {
                plugin = make_plugin(&new_cfg.mode);
                mod_config = make_mod_config(&new_cfg);
            }
            if new_cfg.fec_enabled != current.fec_enabled {
                fec = new_cfg.fec_enabled.then(FecCodec::new);
            }
            if new_cfg.noise_model != current.noise_model
                || new_cfg.snr_db != current.snr_db
                || new_cfg.seed_str != current.seed_str
            {
                let seed = new_cfg.seed_str.parse::<u64>().ok();
                let channel_config = make_channel_config(&new_cfg);
                match build_channel(&channel_config, seed) {
                    Ok(c) => channel = c,
                    Err(e) => {
                        tracing::error!("failed to rebuild channel model: {e}");
                        break;
                    }
                }
            }
            current = new_cfg;
        }

        run_count += 1;
        let min_db = current.min_db;
        let max_db = current.max_db;
        let payload = make_payload(current.payload_size, run_count);

        // Compress payload before FEC. compress_if_smaller falls back to None when LZ4
        // would expand the data, so compress_ratio is always ≤ 1.0.
        let (compressed, actual_algo) = match current.compression {
            CompressionAlgorithm::None => (payload.clone(), CompressionAlgorithm::None),
            CompressionAlgorithm::Lz4 => compress_if_smaller(&payload),
        };
        let compress_ratio = compressed.len() as f64 / payload.len() as f64;
        let tx_payload = match &fec {
            Some(codec) => codec.encode(&compressed),
            None => compressed,
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
        // Show a clean re-modulated signal only when the decoded payload is bit-perfect.
        // The BPSK demodulator returns Ok even for noise, so we must compare content:
        // any byte error makes the spectrum look identical to a clean decode (same
        // center + bandwidth), hiding the channel degradation from the user.
        let rx_samples = match &rx_result {
            Ok(decoded) => {
                let base = match &fec {
                    Some(codec) => codec.decode(decoded).unwrap_or_else(|_| decoded.clone()),
                    None => decoded.clone(),
                };
                let plain = decompress(&base, actual_algo).unwrap_or_else(|_| base.clone());
                if plain == payload {
                    plugin
                        .modulate(&tx_payload, &mod_config)
                        .unwrap_or_else(|_| vec![0.0_f32; tx_samples.len()])
                } else {
                    vec![0.0_f32; tx_samples.len()]
                }
            }
            Err(_) => vec![0.0_f32; tx_samples.len()],
        };
        push_tap(&taps[3], &mut ps[3], &rx_samples, min_db, max_db);

        update_stats(
            &stats,
            &rx_result,
            &fec,
            actual_algo,
            compress_ratio,
            &payload,
        );
    }
}

#[cfg(feature = "cpal")]
fn run_live(
    config: Arc<RwLock<AppConfig>>,
    taps: [Tap; 4],
    stats: Arc<RwLock<TestStats>>,
    stop_rx: crossbeam_channel::Receiver<()>,
) {
    use openpulse_audio::CpalBackend;
    use openpulse_core::audio::{AudioBackend as _, AudioConfig as HwConfig};

    let backend = CpalBackend::new();
    let hw_cfg = HwConfig::default(); // 8000 Hz mono — radio audio interfaces support this
    let mut input = match backend.open_input(None, &hw_cfg) {
        Ok(s) => s,
        Err(e) => {
            let msg = format!(
                "audio input failed: {e} — ensure your audio interface supports 8 kHz mono"
            );
            tracing::error!("{msg}");
            stats.write().unwrap().push_event(msg);
            return;
        }
    };

    let mut current = config.read().unwrap().clone();
    let mut plugin = make_plugin(&current.mode);
    let mut mod_config = make_mod_config(&current);
    let mut fec = current.fec_enabled.then(FecCodec::new);

    let mut ps = [
        PowerSpectrum::new(),
        PowerSpectrum::new(),
        PowerSpectrum::new(),
        PowerSpectrum::new(),
    ];

    // Pre-modulate a TX reference frame shown in tap[0].
    let mut tx_samples = build_tx_ref(&plugin, &mod_config, &fec);

    loop {
        if stop_rx.try_recv().is_ok() {
            break;
        }

        // Apply any mode/fec changes from the UI.
        let new_cfg = config.read().unwrap().clone();
        if new_cfg.mode != current.mode || new_cfg.fec_enabled != current.fec_enabled {
            plugin = make_plugin(&new_cfg.mode);
            mod_config = make_mod_config(&new_cfg);
            fec = new_cfg.fec_enabled.then(FecCodec::new);
            tx_samples = build_tx_ref(&plugin, &mod_config, &fec);
        }
        current = new_cfg;

        let min_db = current.min_db;
        let max_db = current.max_db;

        // Tap[2]: raw audio captured from the soundcard.
        let captured = match input.read() {
            Ok(s) if !s.is_empty() => s,
            Ok(_) => continue,
            Err(e) => {
                tracing::warn!("audio capture error: {e}");
                std::thread::sleep(std::time::Duration::from_millis(20));
                continue;
            }
        };

        // Tap[0]: synthesized TX reference (static while no mode change).
        push_tap(&taps[0], &mut ps[0], &tx_samples, min_db, max_db);
        push_tap(&taps[2], &mut ps[2], &captured, min_db, max_db);

        // Tap[3]: demodulate captured audio.
        let rx_result = plugin.demodulate(&captured, &mod_config);
        let rx_samples = match &rx_result {
            Ok(decoded) => {
                let base = match &fec {
                    Some(codec) => codec.decode(decoded).unwrap_or_else(|_| decoded.clone()),
                    None => decoded.clone(),
                };
                plugin
                    .modulate(&base, &mod_config)
                    .unwrap_or_else(|_| vec![0.0_f32; captured.len()])
            }
            Err(_) => vec![0.0_f32; captured.len()],
        };
        push_tap(&taps[3], &mut ps[3], &rx_samples, min_db, max_db);

        // Live mode: count decode success/fail only — no BER against PAYLOAD.
        {
            let mut s = stats.write().unwrap();
            s.runs += 1;
            if rx_result.is_ok() {
                s.ok += 1;
            } else {
                s.fail += 1;
                if s.fail <= 10 || s.fail.is_multiple_of(100) {
                    let msg = format!("Run {}: no signal decoded", s.runs);
                    s.push_event(msg);
                }
            }
        }
    }

    input.close();
}

#[cfg(feature = "cpal")]
fn build_tx_ref(
    plugin: &Box<dyn ModulationPlugin>,
    mod_config: &ModulationConfig,
    fec: &Option<FecCodec>,
) -> Vec<f32> {
    let tx_payload = match fec {
        Some(codec) => codec.encode(PATTERN),
        None => PATTERN.to_vec(),
    };
    plugin
        .modulate(&tx_payload, mod_config)
        .unwrap_or_else(|e| {
            tracing::error!("modulate error for TX ref: {e}");
            Vec::new()
        })
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
    compression: CompressionAlgorithm,
    compress_ratio: f64,
    payload: &[u8],
) {
    const WINDOW_SECS: f64 = 1.5;
    let n_bits = payload.len() * 8;
    let error_bits: u64 = match rx_result {
        Ok(decoded) => {
            let fec_out = match fec {
                Some(codec) => codec.decode(decoded).unwrap_or_else(|_| decoded.clone()),
                None => decoded.clone(),
            };
            let plain = decompress(&fec_out, compression).unwrap_or(fec_out);
            count_bit_errors(payload, &plain)
        }
        Err(_) => n_bits as u64,
    };
    let success = rx_result.is_ok() && error_bits == 0;

    let mut s = stats.write().unwrap();
    s.runs += 1;
    s.total_bits += n_bits as u64;
    s.error_bits += error_bits;
    s.last_compress_ratio = compress_ratio;

    // Sliding window: push this run's delivered bits and evict entries older than WINDOW_SECS.
    let now = std::time::Instant::now();
    let delivered = if success { n_bits as u64 } else { 0 };
    s.rate_window.push_back((now, delivered));
    let cutoff = now - std::time::Duration::from_secs_f64(WINDOW_SECS);
    while s
        .rate_window
        .front()
        .map(|(t, _)| *t < cutoff)
        .unwrap_or(false)
    {
        s.rate_window.pop_front();
    }

    if success {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{AppConfig, TapData, TestStats};

    #[test]
    fn run_loop_produces_tap_updates() {
        let config = Arc::new(RwLock::new(AppConfig::default()));
        let make_tap = || Arc::new(RwLock::new(TapData::new()));
        let taps: [Tap; 4] = [make_tap(), make_tap(), make_tap(), make_tap()];
        let stats = Arc::new(RwLock::new(TestStats::new()));
        let (stop_tx, stop_rx) = crossbeam_channel::bounded(1);

        let taps_clone = taps.clone();
        let stats_clone = Arc::clone(&stats);
        let config_clone = Arc::clone(&config);
        let handle = std::thread::spawn(move || {
            run(config_clone, taps_clone, stats_clone, stop_rx);
        });

        // Let the signal thread do a few iterations
        std::thread::sleep(std::time::Duration::from_millis(150));
        let _ = stop_tx.send(());
        handle.join().expect("signal thread should not panic");

        // Taps should have received data
        for (i, tap) in taps.iter().enumerate() {
            let gen = tap.read().unwrap().generation;
            assert!(gen > 0, "tap[{i}] generation should be > 0, got {gen}");
        }
        assert!(stats.read().unwrap().runs > 0, "runs should be > 0");
    }
}
