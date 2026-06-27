use std::sync::{Arc, RwLock};

use bpsk_plugin::BpskPlugin;
use fsk4_plugin::Fsk4Plugin;
use ofdm_plugin::OfdmPlugin;
use openpulse_channel::dsp::PowerSpectrum;
use openpulse_channel::{
    build_channel, AwgnConfig, ChannelModelConfig, ChirpConfig, GilbertElliottConfig, QrmConfig,
    QrnConfig, QsbConfig, ToneConfig, WattersonConfig,
};
use openpulse_core::compression::{compress, decompress, CompressionAlgorithm};
use openpulse_core::fec::{FecCodec, FecMode};
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use openpulse_core::profile::SessionProfile;
use openpulse_core::soft_viterbi::SoftViterbiCodec;
use openpulse_modem::channel_sim::ChannelSimHarness;
use openpulse_modem::ModemEngine;
use pilot_plugin::PilotPlugin;
use psk8_plugin::Psk8Plugin;
use qam64_plugin::Qam64Plugin;
use qpsk_plugin::QpskPlugin;
use scfdma_plugin::ScFdmaPlugin;

use crate::state::{fec_locked, AppConfig, AudioSource, NoiseModel, Tap, TestStats};

/// Base pattern repeated to fill the configured payload size.
const PATTERN: &[u8] = b"OpenPulseHF testbench v0.1 test!";

/// FEC dispatch for the direct-plugin testbench path.
enum TestbenchFec {
    None,
    Rs(FecCodec),
    RsStrong(FecCodec),
    /// K=7 soft-decision Conv inner + RS(255,223) outer.
    SoftConcatenated(FecCodec),
}

impl TestbenchFec {
    fn from_mode(mode: FecMode) -> Self {
        match mode {
            FecMode::None => Self::None,
            FecMode::Rs | FecMode::RsInterleaved => Self::Rs(FecCodec::new()),
            FecMode::RsStrong => Self::RsStrong(FecCodec::strong()),
            FecMode::SoftConcatenated => Self::SoftConcatenated(FecCodec::new()),
            _ => Self::None,
        }
    }

    fn encode(&self, data: &[u8]) -> Vec<u8> {
        match self {
            Self::None => data.to_vec(),
            Self::Rs(c) | Self::RsStrong(c) => c.encode(data),
            Self::SoftConcatenated(c) => {
                let rs_encoded = c.encode(data);
                SoftViterbiCodec.encode(&rs_encoded)
            }
        }
    }

    /// RS-layer encoded bytes only (no Viterbi for SoftConcatenated).
    /// Used as the reference for FEC channel-error counting.
    fn inner_encode(&self, data: &[u8]) -> Vec<u8> {
        match self {
            Self::None => data.to_vec(),
            Self::Rs(c) | Self::RsStrong(c) | Self::SoftConcatenated(c) => c.encode(data),
        }
    }

    /// Returns `(pre_rs_bytes, post_rs_bytes)` on success.
    ///
    /// `pre_rs_bytes` are the bytes entering the RS decoder (raw demod for Rs/RsStrong,
    /// Viterbi-decoded for SoftConcatenated).  Compare against `inner_encode()` output
    /// to measure channel bit errors before RS correction.
    fn decode_soft(
        &self,
        plugin: &dyn ModulationPlugin,
        samples: &[f32],
        mod_config: &ModulationConfig,
    ) -> Option<(Vec<u8>, Vec<u8>)> {
        match self {
            Self::None => {
                let b = plugin.demodulate(samples, mod_config).ok()?;
                Some((b.clone(), b))
            }
            Self::Rs(c) | Self::RsStrong(c) => {
                let raw = plugin.demodulate(samples, mod_config).ok()?;
                let decoded = c.decode(&raw).ok()?;
                Some((raw, decoded))
            }
            Self::SoftConcatenated(c) => {
                let llrs = plugin.demodulate_soft(samples, mod_config).ok()?;
                let sv_decoded = SoftViterbiCodec.decode_soft(&llrs).ok()?;
                let rs_decoded = c.decode(&sv_decoded).ok()?;
                Some((sv_decoded, rs_decoded))
            }
        }
    }

    fn is_active(&self) -> bool {
        !matches!(self, Self::None)
    }
}

fn mode_symbol_rate(mode: &str) -> f32 {
    match mode {
        "BPSK31" => 31.25,
        "BPSK63" => 62.5,
        "BPSK100" => 100.0,
        "BPSK250" | "BPSK250-RRC" => 250.0,
        "QPSK125" => 125.0,
        "QPSK250" => 250.0,
        "QPSK500" | "QPSK500-RRC" => 500.0,
        "QPSK1000" | "QPSK1000-HF" | "QPSK1000-RRC" => 1000.0,
        "8PSK500" | "8PSK500-RRC" => 500.0,
        "8PSK1000" | "8PSK1000-HF" | "8PSK1000-RRC" => 1000.0,
        "64QAM500" => 500.0,
        "64QAM1000" => 1000.0,
        "64QAM2000-RRC" => 2000.0,
        "QPSK2000" | "QPSK2000-RRC" | "8PSK2000" | "8PSK2000-RRC" => 2000.0,
        "FSK4-ACK" => 100.0,
        // OFDM/SC-FDMA symbol rate = fs / (FFT_SIZE + CP) = 8000 / 288 ≈ 27.78 baud
        "OFDM16" | "OFDM52" | "SCFDMA16" | "SCFDMA52" => 8000.0 / 288.0,
        _ => 250.0,
    }
}

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
    let source = config.read().unwrap().audio_source.clone();
    match source {
        AudioSource::VirtualLoop => {
            std::thread::spawn(move || run_virtual(config, taps, stats, stop_rx))
        }
        AudioSource::TestMatrix => {
            std::thread::spawn(move || run_testmatrix(config, taps, stats, stop_rx))
        }
        AudioSource::AdaptiveLadder => {
            std::thread::spawn(move || run_adaptive_ladder(config, taps, stats, stop_rx))
        }
        #[cfg(feature = "cpal")]
        AudioSource::LiveCapture => {
            std::thread::spawn(move || run_live(config, taps, stats, stop_rx))
        }
        #[cfg(feature = "cpal")]
        AudioSource::HardwareLoop => {
            std::thread::spawn(move || run_hardware(config, taps, stats, stop_rx))
        }
        AudioSource::Synthetic => std::thread::spawn(move || run(config, taps, stats, stop_rx)),
    }
}

fn make_plugin(mode: &str) -> Box<dyn ModulationPlugin> {
    if mode.starts_with("BPSK") {
        Box::new(BpskPlugin::new())
    } else if mode.starts_with("64QAM") {
        Box::new(Qam64Plugin::new())
    } else if mode.starts_with("8PSK") {
        Box::new(Psk8Plugin::new())
    } else if mode == "FSK4-ACK" {
        Box::new(Fsk4Plugin::new())
    } else if mode.starts_with("OFDM") {
        Box::new(OfdmPlugin::new())
    } else if mode.starts_with("SCFDMA") {
        Box::new(ScFdmaPlugin::new())
    } else if mode.starts_with("PILOT") {
        Box::new(PilotPlugin::new())
    } else {
        // QPSK* and any other unrecognised mode
        Box::new(QpskPlugin::new())
    }
}

/// Measure a mode's steady-state payload bit rate (bps) from the modulator.
///
/// Two-point differencing of the sample count for two payload sizes cancels the fixed
/// preamble, leaving the true per-payload-byte rate (which for pilot / OFDM / SC-FDMA
/// modes correctly includes their periodic pilot/CP overhead). Returns `None` if the
/// mode cannot be modulated.
pub fn measure_mode_rate(mode: &str) -> Option<f64> {
    let plugin = make_plugin(mode);
    let cfg = ModulationConfig {
        mode: mode.into(),
        center_frequency: 1500.0,
        sample_rate: 8000,
        ..ModulationConfig::default()
    };
    let (n1, n2) = (128usize, 256usize);
    let p1: Vec<u8> = (0..n1).map(|i| i as u8).collect();
    let p2: Vec<u8> = (0..n2).map(|i| i as u8).collect();
    let s1 = plugin.modulate(&p1, &cfg).ok()?.len();
    let s2 = plugin.modulate(&p2, &cfg).ok()?.len();
    if s2 <= s1 {
        return None;
    }
    Some((n2 - n1) as f64 * 8.0 * 8000.0 / (s2 - s1) as f64)
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
    let mut fec = TestbenchFec::from_mode(current.fec_mode);

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
            if new_cfg.fec_mode != current.fec_mode {
                fec = TestbenchFec::from_mode(new_cfg.fec_mode);
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

        // Compress payload before FEC, honouring the user's algorithm selection.
        // Fall back to no compression if the chosen algorithm expands the data.
        let (compressed, actual_algo) = match current.compression {
            CompressionAlgorithm::None => (payload.clone(), CompressionAlgorithm::None),
            algo => {
                let c = compress(&payload, algo);
                if c.len() < payload.len() {
                    (c, algo)
                } else {
                    (payload.clone(), CompressionAlgorithm::None)
                }
            }
        };
        let compress_ratio = compressed.len() as f64 / payload.len() as f64;
        let tx_inner = fec.inner_encode(&compressed);
        let tx_payload = fec.encode(&compressed);

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

        // Decode using the soft path (uses LLRs for SoftConcatenated, hard-decision otherwise).
        let decoded = fec.decode_soft(plugin.as_ref(), &mixed, &mod_config);
        let (rx_result, pre_rs_bytes) = match decoded {
            Some((pre_rs, post_rs)) => {
                let result = decompress(&post_rs, actual_algo).map_err(|_| {
                    openpulse_core::error::ModemError::Demodulation("decompress failed".into())
                });
                (result, Some(pre_rs))
            }
            None => (
                Err(openpulse_core::error::ModemError::Demodulation(
                    "decode failed".into(),
                )),
                None,
            ),
        };

        // Show a clean re-modulated signal only when the decoded payload is bit-perfect.
        let rx_samples = match &rx_result {
            Ok(plain) if *plain == payload => plugin
                .modulate(&tx_payload, &mod_config)
                .unwrap_or_else(|_| vec![0.0_f32; tx_samples.len()]),
            _ => vec![0.0_f32; tx_samples.len()],
        };
        push_tap(&taps[3], &mut ps[3], &rx_samples, min_db, max_db);

        // IQ scatter: extract symbols from the mixed (post-channel) signal.
        let baud = mode_symbol_rate(current.mode.as_str());
        push_iq_symbols(&taps[3], &mixed, 1500.0, 8000.0, baud);

        let snr_db = estimate_snr_db(&tx_samples, &noise_samples);
        update_stats(
            &stats,
            &rx_result,
            fec.is_active(),
            &tx_inner,
            pre_rs_bytes.as_deref(),
            compress_ratio,
            &payload,
            snr_db,
        );
    }
}

/// Register the modulation plugins the testbench ships with on a modem engine.
fn register_engine_plugins(engine: &mut ModemEngine) {
    let _ = engine.register_plugin(Box::new(BpskPlugin::new()));
    let _ = engine.register_plugin(Box::new(QpskPlugin::new()));
    let _ = engine.register_plugin(Box::new(Psk8Plugin::new()));
    let _ = engine.register_plugin(Box::new(Qam64Plugin::new()));
    let _ = engine.register_plugin(Box::new(Fsk4Plugin::new()));
    let _ = engine.register_plugin(Box::new(OfdmPlugin::new()));
    let _ = engine.register_plugin(Box::new(ScFdmaPlugin::new()));
    let _ = engine.register_plugin(Box::new(PilotPlugin::new()));
}

/// Transmit through the engine, dispatching on the FEC mode the testbench exposes.
fn engine_transmit(
    engine: &mut ModemEngine,
    data: &[u8],
    mode: &str,
    fec: FecMode,
) -> Result<(), openpulse_core::error::ModemError> {
    match fec {
        FecMode::Rs | FecMode::RsInterleaved => engine.transmit_with_fec(data, mode, None),
        FecMode::RsStrong => engine.transmit_with_strong_fec(data, mode, None),
        FecMode::SoftConcatenated => engine.transmit_with_soft_viterbi_fec(data, mode, None),
        _ => engine.transmit(data, mode, None),
    }
    .map(|_| ())
}

/// Receive through the engine, dispatching on the FEC mode the testbench exposes.
fn engine_receive(
    engine: &mut ModemEngine,
    mode: &str,
    fec: FecMode,
) -> Result<Vec<u8>, openpulse_core::error::ModemError> {
    match fec {
        FecMode::Rs | FecMode::RsInterleaved => engine.receive_with_fec(mode, None),
        FecMode::RsStrong => engine.receive_with_strong_fec(mode, None),
        FecMode::SoftConcatenated => engine.receive_with_soft_viterbi_fec(mode, None),
        _ => engine.receive(mode, None),
    }
}

/// The testmatrix virtual loop: two real `ModemEngine`s routed through a channel
/// model via `ChannelSimHarness`. Visualizes the true TX, post-channel, and decoded
/// signals — the same path the `openpulse-testmatrix` raw-modem runner exercises.
fn run_virtual(
    config: Arc<RwLock<AppConfig>>,
    taps: [Tap; 4],
    stats: Arc<RwLock<TestStats>>,
    stop_rx: crossbeam_channel::Receiver<()>,
) {
    let mut current = config.read().unwrap().clone();

    let mut harness = ChannelSimHarness::new();
    register_engine_plugins(&mut harness.tx_engine);
    register_engine_plugins(&mut harness.rx_engine);

    let seed = current.seed_str.parse::<u64>().ok();
    let mut channel = match build_channel(&make_channel_config(&current), seed) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("virtual loop: failed to build channel model: {e}");
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

        let new_cfg = config.read().unwrap().clone();
        if new_cfg != current {
            if new_cfg.noise_model != current.noise_model
                || new_cfg.snr_db != current.snr_db
                || new_cfg.seed_str != current.seed_str
            {
                let seed = new_cfg.seed_str.parse::<u64>().ok();
                match build_channel(&make_channel_config(&new_cfg), seed) {
                    Ok(c) => channel = c,
                    Err(e) => {
                        tracing::error!("virtual loop: failed to rebuild channel model: {e}");
                        break;
                    }
                }
            }
            current = new_cfg;
        }

        run_count += 1;
        let mode = current.mode.clone();
        // Engine path: only FSK4-ACK can't carry FEC; OFDM / SC-FDMA can (the engine frames it).
        let fec_mode = if fec_locked(&mode, true) {
            FecMode::None
        } else {
            current.fec_mode
        };
        virtual_frame(
            &mut harness,
            channel.as_mut(),
            &mode,
            fec_mode,
            current.compression,
            current.payload_size,
            run_count,
            &taps,
            &mut ps,
            &stats,
            current.min_db,
            current.max_db,
        );
    }
}

/// Run a single virtual-loop frame: transmit → route through `channel` → receive,
/// updating all four taps and the statistics. Returns whether the payload decoded.
#[allow(clippy::too_many_arguments)]
fn virtual_frame(
    harness: &mut ChannelSimHarness,
    channel: &mut dyn openpulse_channel::ChannelModel,
    mode: &str,
    fec_mode: FecMode,
    compression: CompressionAlgorithm,
    payload_size: usize,
    run_count: u64,
    taps: &[Tap; 4],
    ps: &mut [PowerSpectrum; 4],
    stats: &Arc<RwLock<TestStats>>,
    min_db: f32,
    max_db: f32,
) -> bool {
    let payload = make_payload(payload_size, run_count);

    let (wire_payload, actual_algo) = match compression {
        CompressionAlgorithm::None => (payload.clone(), CompressionAlgorithm::None),
        algo => {
            let c = compress(&payload, algo);
            if c.len() < payload.len() {
                (c, algo)
            } else {
                (payload.clone(), CompressionAlgorithm::None)
            }
        }
    };
    let compress_ratio = wire_payload.len() as f64 / payload.len() as f64;

    if let Err(e) = engine_transmit(&mut harness.tx_engine, &wire_payload, mode, fec_mode) {
        tracing::warn!("virtual frame: transmit error: {e}");
        stats.write().unwrap().push_event(format!("TX error: {e}"));
        return false;
    }

    let (tx_samples, channel_output) = harness.route_tapped(channel);

    // Realized impairment: post-channel minus pre-channel (additive component).
    let n = tx_samples.len().min(channel_output.len());
    let noise: Vec<f32> = (0..n).map(|i| channel_output[i] - tx_samples[i]).collect();

    push_tap(&taps[0], &mut ps[0], &tx_samples, min_db, max_db);
    push_tap(&taps[1], &mut ps[1], &noise, min_db, max_db);
    push_tap(&taps[2], &mut ps[2], &channel_output, min_db, max_db);

    let rx_result = match engine_receive(&mut harness.rx_engine, mode, fec_mode) {
        Ok(raw) => decompress(&raw, actual_algo).map_err(|_| {
            openpulse_core::error::ModemError::Demodulation("decompress failed".into())
        }),
        Err(e) => Err(e),
    };

    // tap[3]: clean pre-channel reference when the link delivered the payload, else silence.
    let decode_ok = matches!(&rx_result, Ok(p) if *p == payload);
    let rx_samples = if decode_ok {
        tx_samples.clone()
    } else {
        vec![0.0_f32; tx_samples.len()]
    };
    push_tap(&taps[3], &mut ps[3], &rx_samples, min_db, max_db);
    push_iq_symbols(
        &taps[3],
        &channel_output,
        1500.0,
        8000.0,
        mode_symbol_rate(mode),
    );

    let snr_db = estimate_snr_db(&tx_samples, &noise);
    update_stats(
        stats,
        &rx_result,
        false,
        &[],
        None,
        compress_ratio,
        &payload,
        snr_db,
    );
    {
        // Record the actually-running mode/FEC so the bitrate readout is correct even
        // when it differs from the (frozen) UI selection, e.g. during a matrix sweep.
        let mut s = stats.write().unwrap();
        s.active_mode = Some(mode.to_string());
        s.active_fec = fec_mode;
    }
    decode_ok
}

/// One test-matrix case: a mode × channel × FEC combination.
struct MatrixCase {
    mode: &'static str,
    channel_label: &'static str,
    channel: ChannelModelConfig,
    fec: FecMode,
}

/// Build a representative mode × channel × FEC matrix to sweep in TestMatrix mode.
fn build_matrix_cases() -> Vec<MatrixCase> {
    // Modes spanning the constellation orders the testbench can exercise.
    const MODES: &[&str] = &[
        "BPSK250",
        "QPSK500",
        "8PSK1000",
        "64QAM1000",
        "OFDM52",
        "SCFDMA52",
    ];
    // Channels: clean (high-SNR AWGN), two AWGN SNRs, and a Watterson F1 fade.
    let channels: [(&str, ChannelModelConfig); 4] = [
        (
            "clean",
            ChannelModelConfig::Awgn(AwgnConfig {
                snr_db: 50.0,
                seed: None,
            }),
        ),
        (
            "AWGN 20 dB",
            ChannelModelConfig::Awgn(AwgnConfig {
                snr_db: 20.0,
                seed: None,
            }),
        ),
        (
            "AWGN 10 dB",
            ChannelModelConfig::Awgn(AwgnConfig {
                snr_db: 10.0,
                seed: None,
            }),
        ),
        ("Watterson F1 20 dB", {
            let mut cfg = WattersonConfig::moderate_f1(None);
            cfg.snr_db = 20.0;
            ChannelModelConfig::Watterson(cfg)
        }),
    ];

    let mut cases = Vec::new();
    for &mode in MODES {
        // Engine path frames the payload, so OFDM / SC-FDMA carry RS; only FSK4-ACK can't.
        let fecs: &[FecMode] = if fec_locked(mode, true) {
            &[FecMode::None]
        } else {
            &[FecMode::None, FecMode::Rs]
        };
        for (label, channel) in &channels {
            for &fec in fecs {
                cases.push(MatrixCase {
                    mode,
                    channel_label: label,
                    channel: channel.clone(),
                    fec,
                });
            }
        }
    }
    cases
}

/// Sweep the full mode × channel × FEC matrix through the virtual loop, advancing
/// case by case and visualizing each in the 4-tap view.
fn run_testmatrix(
    config: Arc<RwLock<AppConfig>>,
    taps: [Tap; 4],
    stats: Arc<RwLock<TestStats>>,
    stop_rx: crossbeam_channel::Receiver<()>,
) {
    const FRAMES_PER_CASE: u64 = 4;

    let cases = build_matrix_cases();
    let total = cases.len();

    let mut harness = ChannelSimHarness::new();
    register_engine_plugins(&mut harness.tx_engine);
    register_engine_plugins(&mut harness.rx_engine);

    let mut ps = [
        PowerSpectrum::new(),
        PowerSpectrum::new(),
        PowerSpectrum::new(),
        PowerSpectrum::new(),
    ];
    let mut run_count: u64 = 0;
    let payload_size = config.read().unwrap().payload_size;

    'sweep: loop {
        for (idx, case) in cases.iter().enumerate() {
            if stop_rx.try_recv().is_ok() {
                break 'sweep;
            }

            let (min_db, max_db) = {
                let c = config.read().unwrap();
                (c.min_db, c.max_db)
            };

            let mut channel = match build_channel(&case.channel, None) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("test matrix: channel build failed for {}: {e}", case.mode);
                    continue;
                }
            };

            let fec_label = match case.fec {
                FecMode::Rs => "RS",
                _ => "no FEC",
            };
            stats.write().unwrap().matrix_current = Some(format!(
                "[{}/{}] {} | {} | {}",
                idx + 1,
                total,
                case.mode,
                case.channel_label,
                fec_label,
            ));

            let mut case_ok = 0u64;
            for _ in 0..FRAMES_PER_CASE {
                if stop_rx.try_recv().is_ok() {
                    break 'sweep;
                }
                run_count += 1;
                if virtual_frame(
                    &mut harness,
                    channel.as_mut(),
                    case.mode,
                    case.fec,
                    CompressionAlgorithm::None,
                    payload_size,
                    run_count,
                    &taps,
                    &mut ps,
                    &stats,
                    min_db,
                    max_db,
                ) {
                    case_ok += 1;
                }
                // Brief dwell so each case is watchable rather than flashing past.
                if stop_rx
                    .recv_timeout(std::time::Duration::from_millis(60))
                    .is_ok()
                {
                    break 'sweep;
                }
            }

            stats.write().unwrap().push_event(format!(
                "[{}/{}] {} | {} | {}: {}/{} ok",
                idx + 1,
                total,
                case.mode,
                case.channel_label,
                fec_label,
                case_ok,
                FRAMES_PER_CASE,
            ));
        }
    }

    stats.write().unwrap().matrix_current = None;
}

/// Run a SessionProfile's adaptive ladder through the virtual loop, stepping the speed
/// level up/down against the live SNR so the rate-adaptation ladder can be demonstrated.
fn run_adaptive_ladder(
    config: Arc<RwLock<AppConfig>>,
    taps: [Tap; 4],
    stats: Arc<RwLock<TestStats>>,
    stop_rx: crossbeam_channel::Receiver<()>,
) {
    let mut current = config.read().unwrap().clone();
    let mut profile_name = current.profile.clone();
    let mut profile = SessionProfile::by_name(&profile_name).unwrap_or_else(SessionProfile::hpx500);
    let mut levels = profile.defined_levels();
    if levels.is_empty() {
        stats
            .write()
            .unwrap()
            .push_event(format!("profile '{profile_name}' has no levels"));
        return;
    }

    let mut harness = ChannelSimHarness::new();
    register_engine_plugins(&mut harness.tx_engine);
    register_engine_plugins(&mut harness.rx_engine);

    let seed = current.seed_str.parse::<u64>().ok();
    let mut channel = match build_channel(&make_channel_config(&current), seed) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("adaptive ladder: failed to build channel: {e}");
            return;
        }
    };

    let mut ps = [
        PowerSpectrum::new(),
        PowerSpectrum::new(),
        PowerSpectrum::new(),
        PowerSpectrum::new(),
    ];
    let mut idx = 0usize; // start at the most robust level and climb
    let mut run_count: u64 = 0;

    loop {
        if stop_rx
            .recv_timeout(std::time::Duration::from_millis(120))
            .is_ok()
        {
            break;
        }

        let new_cfg = config.read().unwrap().clone();
        if new_cfg.profile != profile_name {
            profile_name = new_cfg.profile.clone();
            profile = SessionProfile::by_name(&profile_name).unwrap_or_else(SessionProfile::hpx500);
            levels = profile.defined_levels();
            idx = 0;
            if levels.is_empty() {
                stats
                    .write()
                    .unwrap()
                    .push_event(format!("profile '{profile_name}' has no levels"));
                break;
            }
        }
        if new_cfg.noise_model != current.noise_model
            || new_cfg.snr_db != current.snr_db
            || new_cfg.seed_str != current.seed_str
        {
            let seed = new_cfg.seed_str.parse::<u64>().ok();
            if let Ok(c) = build_channel(&make_channel_config(&new_cfg), seed) {
                channel = c;
            }
        }
        current = new_cfg;

        let level = levels[idx];
        let mode = profile.mode_for(level).unwrap_or("BPSK250").to_string();
        let fec = if fec_locked(&mode, true) {
            FecMode::None
        } else {
            FecMode::Rs
        };

        // Publish the current case before transmitting (the frame can take a while for
        // slow modes), so the status reflects what is on air right now.
        let snr = current.snr_db;
        let floor = profile.snr_floor_for_level(level);
        let ceiling = profile.snr_ceiling_for_level(level);
        let floor_s = floor
            .map(|f| format!("{f:.0}"))
            .unwrap_or_else(|| "—".into());
        let ceil_s = ceiling
            .map(|c| format!("{c:.0}"))
            .unwrap_or_else(|| "—".into());
        stats.write().unwrap().matrix_current = Some(format!(
            "{profile_name} | SL{} {} | SNR {snr:.0} dB (floor {floor_s} / ceil {ceil_s}) | {}/{} levels",
            level as usize,
            mode,
            idx + 1,
            levels.len(),
        ));

        run_count += 1;
        let ok = virtual_frame(
            &mut harness,
            channel.as_mut(),
            &mode,
            fec,
            CompressionAlgorithm::None,
            current.payload_size,
            run_count,
            &taps,
            &mut ps,
            &stats,
            current.min_db,
            current.max_db,
        );

        // SNR-driven step with hysteresis from the profile's own floor/ceiling thresholds;
        // also step down immediately on a decode failure.
        let step_down = !ok || floor.is_some_and(|f| snr < f);
        let step_up = ceiling.is_some_and(|c| snr >= c);
        if step_down && idx > 0 {
            idx -= 1;
        } else if step_up && idx + 1 < levels.len() {
            idx += 1;
        }
    }

    stats.write().unwrap().matrix_current = None;
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

    let mut current = config.read().unwrap().clone();
    let backend = CpalBackend::new();
    let hw_cfg = HwConfig::default(); // 8000 Hz mono — radio audio interfaces support this
    let mut input = match backend.open_input(current.input_device.as_deref(), &hw_cfg) {
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

    let mut plugin = make_plugin(&current.mode);
    let mut mod_config = make_mod_config(&current);
    let mut fec = TestbenchFec::from_mode(current.fec_mode);

    let mut ps = [
        PowerSpectrum::new(),
        PowerSpectrum::new(),
        PowerSpectrum::new(),
        PowerSpectrum::new(),
    ];

    // Pre-modulate a TX reference frame shown in tap[0].
    let mut tx_samples = build_tx_ref(plugin.as_ref(), &mod_config, &fec);

    loop {
        if stop_rx.try_recv().is_ok() {
            break;
        }

        // Apply any mode/fec changes from the UI.
        let new_cfg = config.read().unwrap().clone();
        if new_cfg.mode != current.mode || new_cfg.fec_mode != current.fec_mode {
            plugin = make_plugin(&new_cfg.mode);
            mod_config = make_mod_config(&new_cfg);
            fec = TestbenchFec::from_mode(new_cfg.fec_mode);
            tx_samples = build_tx_ref(plugin.as_ref(), &mod_config, &fec);
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
        let decoded_opt = fec.decode_soft(plugin.as_ref(), &captured, &mod_config);
        let rx_result: Result<Vec<u8>, openpulse_core::error::ModemError> = match decoded_opt {
            Some((_, post_rs)) => Ok(post_rs),
            None => Err(openpulse_core::error::ModemError::Demodulation(
                "decode failed".into(),
            )),
        };
        let rx_samples = match &rx_result {
            Ok(b) => plugin
                .modulate(b, &mod_config)
                .unwrap_or_else(|_| vec![0.0_f32; captured.len()]),
            Err(_) => vec![0.0_f32; captured.len()],
        };
        push_tap(&taps[3], &mut ps[3], &rx_samples, min_db, max_db);

        // IQ scatter from the captured audio so the constellation works in live mode.
        push_iq_symbols(
            &taps[3],
            &captured,
            1500.0,
            8000.0,
            mode_symbol_rate(&current.mode),
        );

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

/// Dual-card hardware loop: modulate a frame out one soundcard (TX) and capture
/// from another (RX), visualizing both ends of the real analog loopback.
#[cfg(feature = "cpal")]
fn run_hardware(
    config: Arc<RwLock<AppConfig>>,
    taps: [Tap; 4],
    stats: Arc<RwLock<TestStats>>,
    stop_rx: crossbeam_channel::Receiver<()>,
) {
    use openpulse_audio::CpalBackend;
    use openpulse_core::audio::{AudioBackend as _, AudioConfig as HwConfig};

    let mut current = config.read().unwrap().clone();
    let backend = CpalBackend::new();
    let hw_cfg = HwConfig::default(); // 8 kHz mono

    let mut output = match backend.open_output(current.output_device.as_deref(), &hw_cfg) {
        Ok(s) => s,
        Err(e) => {
            let msg = format!("hardware loop: TX output open failed: {e}");
            tracing::error!("{msg}");
            stats.write().unwrap().push_event(msg);
            return;
        }
    };
    let mut input = match backend.open_input(current.input_device.as_deref(), &hw_cfg) {
        Ok(s) => s,
        Err(e) => {
            let msg = format!("hardware loop: RX input open failed: {e}");
            tracing::error!("{msg}");
            stats.write().unwrap().push_event(msg);
            output.close();
            return;
        }
    };

    let mut plugin = make_plugin(&current.mode);
    let mut mod_config = make_mod_config(&current);
    let mut fec = TestbenchFec::from_mode(current.fec_mode);
    let mut tx_samples = build_tx_ref(plugin.as_ref(), &mod_config, &fec);

    let mut ps = [
        PowerSpectrum::new(),
        PowerSpectrum::new(),
        PowerSpectrum::new(),
        PowerSpectrum::new(),
    ];

    loop {
        if stop_rx.try_recv().is_ok() {
            break;
        }

        let new_cfg = config.read().unwrap().clone();
        if new_cfg.mode != current.mode || new_cfg.fec_mode != current.fec_mode {
            plugin = make_plugin(&new_cfg.mode);
            mod_config = make_mod_config(&new_cfg);
            fec = TestbenchFec::from_mode(new_cfg.fec_mode);
            tx_samples = build_tx_ref(plugin.as_ref(), &mod_config, &fec);
        }
        current = new_cfg;

        let min_db = current.min_db;
        let max_db = current.max_db;

        // Discard any stale captured audio so this iteration's capture lines up with
        // the frame we are about to transmit.
        let _ = input.read();

        // Transmit one frame out card A; flush() blocks until it has fully played,
        // which also paces the loop and prevents the output buffer from growing.
        if let Err(e) = output.write(&tx_samples) {
            tracing::warn!("hardware loop: output write error: {e}");
        }
        let _ = output.flush();

        push_tap(&taps[0], &mut ps[0], &tx_samples, min_db, max_db);

        // Read what card B captured during playback.
        let captured = match input.read() {
            Ok(s) if !s.is_empty() => s,
            Ok(_) => continue,
            Err(e) => {
                tracing::warn!("hardware loop: capture error: {e}");
                std::thread::sleep(std::time::Duration::from_millis(20));
                continue;
            }
        };
        push_tap(&taps[2], &mut ps[2], &captured, min_db, max_db);

        // tap[3]: demodulate the captured audio.
        let decoded_opt = fec.decode_soft(plugin.as_ref(), &captured, &mod_config);
        let rx_result: Result<Vec<u8>, openpulse_core::error::ModemError> = match decoded_opt {
            Some((_, post_rs)) => Ok(post_rs),
            None => Err(openpulse_core::error::ModemError::Demodulation(
                "decode failed".into(),
            )),
        };
        let rx_samples = match &rx_result {
            Ok(b) => plugin
                .modulate(b, &mod_config)
                .unwrap_or_else(|_| vec![0.0_f32; captured.len()]),
            Err(_) => vec![0.0_f32; captured.len()],
        };
        push_tap(&taps[3], &mut ps[3], &rx_samples, min_db, max_db);
        push_iq_symbols(
            &taps[3],
            &captured,
            1500.0,
            8000.0,
            mode_symbol_rate(&current.mode),
        );

        // Count decode success/fail only — no BER against PAYLOAD in the hardware loop.
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
    output.close();
}

#[cfg(feature = "cpal")]
fn build_tx_ref(
    plugin: &dyn ModulationPlugin,
    mod_config: &ModulationConfig,
    fec: &TestbenchFec,
) -> Vec<f32> {
    let tx_payload = fec.encode(PATTERN);
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

/// Push IQ scatter symbols for the RX tap (tap[3]).
///
/// Extracts one IQ pair per symbol period by coherent integration at `center_hz`.
fn push_iq_symbols(tap: &Tap, samples: &[f32], center_hz: f32, sample_rate: f32, baud: f32) {
    if samples.is_empty() || baud <= 0.0 {
        return;
    }
    const MAX_IQ: usize = 2000;
    let sps = (sample_rate / baud).round() as usize;
    if sps < 2 {
        return;
    }
    let scale = 2.0 / sps as f32;
    let mut symbols: Vec<(f32, f32)> = Vec::new();
    let mut offset = 0usize;
    while offset + sps <= samples.len() {
        let (mut i_sum, mut q_sum) = (0.0f32, 0.0f32);
        for k in 0..sps {
            let t = (offset + k) as f32 / sample_rate;
            let phi = std::f32::consts::TAU * center_hz * t;
            i_sum += samples[offset + k] * phi.cos();
            q_sum += -samples[offset + k] * phi.sin();
        }
        symbols.push((i_sum * scale, q_sum * scale));
        offset += sps;
    }
    let mut t = tap.write().unwrap();
    for sym in symbols {
        if t.iq_symbols.len() >= MAX_IQ {
            t.iq_symbols.pop_front();
        }
        t.iq_symbols.push_back(sym);
    }
}

/// Estimate instantaneous SNR from signal and noise RMS (returns dB).
fn estimate_snr_db(signal: &[f32], noise: &[f32]) -> f32 {
    if signal.is_empty() {
        return 0.0;
    }
    let sig_power: f32 = signal.iter().map(|x| x * x).sum::<f32>() / signal.len() as f32;
    let noise_len = noise.len().max(1);
    let noise_power: f32 = noise.iter().map(|x| x * x).sum::<f32>() / noise_len as f32;
    if noise_power < 1e-12 {
        return 40.0; // clean channel
    }
    10.0 * (sig_power / noise_power).log10()
}

#[allow(clippy::too_many_arguments)]
fn update_stats(
    stats: &Arc<RwLock<TestStats>>,
    rx_result: &Result<Vec<u8>, openpulse_core::error::ModemError>,
    fec_is_active: bool,
    tx_inner: &[u8],
    pre_rs_bytes: Option<&[u8]>,
    compress_ratio: f64,
    payload: &[u8],
    snr_db: f32,
) {
    const WINDOW_SECS: f64 = 1.5;
    let n_bits = payload.len() * 8;
    let error_bits: u64 = match rx_result {
        Ok(plain) => count_bit_errors(payload, plain),
        Err(_) => n_bits as u64,
    };

    // FEC correction accounting: compare RS-layer bytes entering/leaving the RS decoder.
    // tx_inner = RS-encoded reference; pre_rs_bytes = received bytes before RS decode.
    // Channel error count is reported whenever pre_rs_bytes is available, independent of
    // whether decompression succeeded, so the ECC readout is not lost on failed frames.
    let (fec_channel_errors, fec_corrected) = if fec_is_active {
        let channel_errors = pre_rs_bytes
            .map(|recv| count_bit_errors(tx_inner, recv))
            .unwrap_or(0);
        let corrected = match (pre_rs_bytes, rx_result) {
            (Some(_), Ok(plain)) => {
                let post_fec = count_bit_errors(payload, plain);
                channel_errors.saturating_sub(post_fec)
            }
            _ => 0,
        };
        (channel_errors, corrected)
    } else {
        (0, 0)
    };

    let success = rx_result.is_ok() && error_bits == 0;

    let mut s = stats.write().unwrap();
    s.runs += 1;
    s.total_bits += n_bits as u64;
    s.error_bits += error_bits;
    s.last_fec_channel_error_bits = fec_channel_errors;
    s.last_fec_corrected_bits = fec_corrected;
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

    // SNR history: cap at 1800 entries (180 s at ~10 Hz).
    const SNR_CAP: usize = 1800;
    let now = std::time::Instant::now();
    if s.snr_history.len() >= SNR_CAP {
        s.snr_history.pop_front();
    }
    s.snr_history.push_back((now, snr_db));
    s.current_snr_db = Some(snr_db);
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
        NoiseModel::FlatFading => ChannelModelConfig::FlatFading(
            openpulse_channel::flat_fading::FlatFadingConfig::moderate(snr, None),
        ),
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

    fn bpsk250_config() -> ModulationConfig {
        ModulationConfig {
            mode: "BPSK250".into(),
            center_frequency: 1500.0,
            sample_rate: 8000,
            ..ModulationConfig::default()
        }
    }

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

    #[test]
    fn run_virtual_produces_tap_updates() {
        let cfg = AppConfig {
            audio_source: crate::state::AudioSource::VirtualLoop,
            mode: "BPSK250".into(),
            ..Default::default()
        };
        let config = Arc::new(RwLock::new(cfg));
        let make_tap = || Arc::new(RwLock::new(TapData::new()));
        let taps: [Tap; 4] = [make_tap(), make_tap(), make_tap(), make_tap()];
        let stats = Arc::new(RwLock::new(TestStats::new()));
        let (stop_tx, stop_rx) = crossbeam_channel::bounded(1);

        let taps_clone = taps.clone();
        let stats_clone = Arc::clone(&stats);
        let config_clone = Arc::clone(&config);
        let handle = std::thread::spawn(move || {
            run_virtual(config_clone, taps_clone, stats_clone, stop_rx);
        });

        std::thread::sleep(std::time::Duration::from_millis(250));
        let _ = stop_tx.send(());
        handle.join().expect("virtual-loop thread should not panic");

        for (i, tap) in taps.iter().enumerate() {
            let gen = tap.read().unwrap().generation;
            assert!(gen > 0, "virtual-loop tap[{i}] generation should be > 0");
        }
        let s = stats.read().unwrap();
        assert!(s.runs > 0, "virtual loop should record runs");
        assert!(
            s.ok > 0,
            "clean BPSK250 virtual loop should decode at least one frame"
        );
    }

    #[test]
    fn all_modes_have_measured_rates() {
        // Every advertised mode must modulate and yield a positive steady-state rate —
        // this also proves make_plugin routes every mode (including pilot) to a plugin.
        for &mode in crate::state::ALL_MODES {
            let r = measure_mode_rate(mode);
            assert!(
                r.is_some_and(|v| v > 0.0),
                "{mode}: no positive measured rate ({r:?})"
            );
        }
    }

    #[test]
    fn run_adaptive_ladder_produces_updates_and_status() {
        // hpx_wideband starts at QPSK500 (fast frames) so the test doesn't wait on a
        // huge BPSK31+RS buffer; hpx500 works in the app but is slow for a unit test.
        let mut cfg = AppConfig {
            audio_source: crate::state::AudioSource::AdaptiveLadder,
            profile: "hpx_wideband".into(),
            noise_model: NoiseModel::Awgn,
            ..Default::default()
        };
        cfg.snr_db = 30.0; // high SNR → the ladder should climb
        let config = Arc::new(RwLock::new(cfg));
        let make_tap = || Arc::new(RwLock::new(TapData::new()));
        let taps: [Tap; 4] = [make_tap(), make_tap(), make_tap(), make_tap()];
        let stats = Arc::new(RwLock::new(TestStats::new()));
        let (stop_tx, stop_rx) = crossbeam_channel::bounded(1);

        let taps_clone = taps.clone();
        let stats_clone = Arc::clone(&stats);
        let config_clone = Arc::clone(&config);
        let handle = std::thread::spawn(move || {
            run_adaptive_ladder(config_clone, taps_clone, stats_clone, stop_rx);
        });

        std::thread::sleep(std::time::Duration::from_millis(500));
        {
            let s = stats.read().unwrap();
            assert!(s.matrix_current.is_some(), "ladder status should be set");
            assert!(
                s.active_mode.is_some(),
                "ladder should set the running mode"
            );
        }
        let _ = stop_tx.send(());
        handle
            .join()
            .expect("adaptive-ladder thread should not panic");

        for (i, tap) in taps.iter().enumerate() {
            assert!(
                tap.read().unwrap().generation > 0,
                "ladder tap[{i}] generation should be > 0"
            );
        }
        assert!(stats.read().unwrap().runs > 0, "ladder should record runs");
    }

    #[test]
    fn matrix_cases_cover_modes_and_channels() {
        let cases = build_matrix_cases();
        assert!(!cases.is_empty(), "matrix should have cases");
        // On the engine path every swept mode carries both None and Rs, incl. OFDM/SC-FDMA.
        assert!(cases
            .iter()
            .any(|c| c.mode == "OFDM52" && c.fec == FecMode::None));
        assert!(cases
            .iter()
            .any(|c| c.mode == "OFDM52" && c.fec == FecMode::Rs));
        assert!(cases
            .iter()
            .any(|c| c.mode == "BPSK250" && c.fec == FecMode::Rs));
    }

    #[test]
    fn run_testmatrix_produces_tap_updates_and_status() {
        let cfg = AppConfig {
            audio_source: crate::state::AudioSource::TestMatrix,
            ..Default::default()
        };
        let config = Arc::new(RwLock::new(cfg));
        let make_tap = || Arc::new(RwLock::new(TapData::new()));
        let taps: [Tap; 4] = [make_tap(), make_tap(), make_tap(), make_tap()];
        let stats = Arc::new(RwLock::new(TestStats::new()));
        let (stop_tx, stop_rx) = crossbeam_channel::bounded(1);

        let taps_clone = taps.clone();
        let stats_clone = Arc::clone(&stats);
        let config_clone = Arc::clone(&config);
        let handle = std::thread::spawn(move || {
            run_testmatrix(config_clone, taps_clone, stats_clone, stop_rx);
        });

        std::thread::sleep(std::time::Duration::from_millis(400));
        {
            // While running, the current-case label should be populated.
            let s = stats.read().unwrap();
            assert!(
                s.matrix_current.is_some(),
                "matrix status should be set while running"
            );
        }
        let _ = stop_tx.send(());
        handle.join().expect("test-matrix thread should not panic");

        for (i, tap) in taps.iter().enumerate() {
            let gen = tap.read().unwrap().generation;
            assert!(gen > 0, "matrix tap[{i}] generation should be > 0");
        }
        assert!(stats.read().unwrap().runs > 0, "matrix should record runs");
    }

    #[test]
    fn testbench_fec_rs_strong_round_trip() {
        let data = b"hello world - RsStrong encode/decode test payload";
        let fec = TestbenchFec::from_mode(FecMode::RsStrong);
        let encoded = fec.encode(data);
        let inner = fec.inner_encode(data);
        // For RsStrong, inner_encode == encode (no extra layer).
        assert_eq!(encoded, inner);

        let plugin = BpskPlugin::new();
        let mod_cfg = bpsk250_config();
        let samples = plugin.modulate(&encoded, &mod_cfg).unwrap();
        let (pre_rs, post_rs) = fec.decode_soft(&plugin, &samples, &mod_cfg).unwrap();

        assert_eq!(
            &post_rs, data,
            "RsStrong: decoded payload must match original"
        );
        assert_eq!(
            pre_rs, encoded,
            "RsStrong: pre_rs must match RS-encoded bytes in clean channel"
        );
    }

    #[test]
    fn testbench_fec_soft_concatenated_round_trip() {
        let data = b"SoftConcatenated FEC encode/decode round-trip test";
        let fec = TestbenchFec::from_mode(FecMode::SoftConcatenated);
        let inner = fec.inner_encode(data); // RS-encoded reference (before Viterbi)
        let encoded = fec.encode(data); // Viterbi(RS(data)) — what gets modulated
        assert_ne!(
            encoded, inner,
            "Viterbi-encoded output must differ from RS-only output"
        );

        let plugin = BpskPlugin::new();
        let mod_cfg = bpsk250_config();
        let samples = plugin.modulate(&encoded, &mod_cfg).unwrap();
        let (pre_rs, post_rs) = fec.decode_soft(&plugin, &samples, &mod_cfg).unwrap();

        assert_eq!(
            &post_rs, data,
            "SoftConcatenated: decoded payload must match original"
        );
        assert_eq!(
            pre_rs, inner,
            "SoftConcatenated: pre_rs (Viterbi-decoded) must match RS-encoded reference"
        );
    }

    #[test]
    fn push_iq_symbols_extracts_carrier_tone() {
        // A pure cosine at the 1500 Hz centre coherently integrates to (+1, ~0); one IQ
        // symbol is emitted per symbol period. This is the extraction the live constellation
        // relies on, so lock it down without needing an audio device.
        let tap: Tap = Arc::new(RwLock::new(TapData::new()));
        let sr = 8000.0_f32;
        let baud = 250.0_f32; // 32 samples/symbol
        let n = 32 * 8; // 8 whole symbols
        let samples: Vec<f32> = (0..n)
            .map(|i| (std::f32::consts::TAU * 1500.0 * i as f32 / sr).cos())
            .collect();

        push_iq_symbols(&tap, &samples, 1500.0, sr, baud);

        let t = tap.read().unwrap();
        assert_eq!(t.iq_symbols.len(), 8, "one IQ symbol per symbol period");
        let (i, q) = t.iq_symbols[0];
        assert!((i - 1.0).abs() < 0.1, "cosine → I≈1.0, got {i}");
        assert!(q.abs() < 0.1, "cosine → Q≈0, got {q}");
    }

    #[test]
    fn push_iq_symbols_ignores_subnyquist_baud() {
        // sps < 2 (baud > sample_rate/2) must be rejected, not panic.
        let tap: Tap = Arc::new(RwLock::new(TapData::new()));
        push_iq_symbols(&tap, &[0.1_f32; 64], 1500.0, 8000.0, 6000.0);
        assert!(tap.read().unwrap().iq_symbols.is_empty());
    }
}
