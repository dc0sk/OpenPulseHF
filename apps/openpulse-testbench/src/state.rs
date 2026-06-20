use std::collections::VecDeque;
use std::sync::{Arc, RwLock};

use openpulse_channel::dsp::{PowerSpectrum, WaterfallBuffer, FFT_SIZE, FREQ_BINS, WATERFALL_ROWS};
use openpulse_core::compression::CompressionAlgorithm;
use openpulse_core::fec::FecMode;

#[cfg(feature = "cpal")]
use openpulse_audio::CpalBackend;
#[cfg(feature = "cpal")]
use openpulse_core::audio::AudioBackend as _;

/// Enumerate capture-capable device names for the live-audio device selector.
#[cfg(feature = "cpal")]
fn enumerate_input_devices() -> Vec<String> {
    CpalBackend::new()
        .list_devices()
        .map(|devs| {
            devs.into_iter()
                .filter(|d| d.is_input)
                .map(|d| d.name)
                .collect()
        })
        .unwrap_or_default()
}

/// Enumerate playback-capable device names for the hardware-loop TX device selector.
#[cfg(feature = "cpal")]
fn enumerate_output_devices() -> Vec<String> {
    CpalBackend::new()
        .list_devices()
        .map(|devs| {
            devs.into_iter()
                .filter(|d| d.is_output)
                .map(|d| d.name)
                .collect()
        })
        .unwrap_or_default()
}

// ── Audio source selector ─────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum AudioSource {
    /// Direct-plugin synthetic channel (modulate → channel model → demodulate).
    Synthetic,
    /// The testmatrix virtual loop: two real `ModemEngine`s routed through a
    /// channel model via `ChannelSimHarness`. No audio hardware required.
    VirtualLoop,
    /// Runs a full mode × channel × FEC matrix through the virtual loop, advancing
    /// case by case so the whole testmatrix can be watched. No audio hardware required.
    TestMatrix,
    /// Live capture from a single soundcard input.
    #[cfg(feature = "cpal")]
    LiveCapture,
    /// Dual-card hardware loop: modulate out one card, capture from another.
    #[cfg(feature = "cpal")]
    HardwareLoop,
}

impl AudioSource {
    pub fn label(&self) -> &'static str {
        match self {
            AudioSource::Synthetic => "Synthetic",
            AudioSource::VirtualLoop => "Virtual loop",
            AudioSource::TestMatrix => "Test matrix",
            #[cfg(feature = "cpal")]
            AudioSource::LiveCapture => "Live Audio",
            #[cfg(feature = "cpal")]
            AudioSource::HardwareLoop => "Hardware loop",
        }
    }
}

// ── Noise model selector ───────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum NoiseModel {
    Awgn,
    GilbertElliott,
    Watterson,
    Qrn,
    Qrm,
    Qsb,
    Chirp,
}

impl NoiseModel {
    pub fn label(&self) -> &'static str {
        match self {
            NoiseModel::Awgn => "AWGN",
            NoiseModel::GilbertElliott => "Gilbert-Elliott",
            NoiseModel::Watterson => "Watterson",
            NoiseModel::Qrn => "QRN",
            NoiseModel::Qrm => "QRM",
            NoiseModel::Qsb => "QSB",
            NoiseModel::Chirp => "Chirp",
        }
    }

    pub fn all() -> &'static [NoiseModel] {
        use NoiseModel::*;
        &[Awgn, GilbertElliott, Watterson, Qrn, Qrm, Qsb, Chirp]
    }
}

// ── Application configuration ─────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct AppConfig {
    pub mode: String,
    pub noise_model: NoiseModel,
    pub snr_db: f32,
    pub fec_mode: FecMode,
    pub compression: CompressionAlgorithm,
    pub payload_size: usize,
    pub seed_str: String,
    pub min_db: f32,
    pub max_db: f32,
    #[cfg_attr(not(feature = "cpal"), allow(dead_code))]
    pub audio_source: AudioSource,
    /// Capture device for live audio / hardware loop; `None` = system default. cpal-only.
    #[cfg_attr(not(feature = "cpal"), allow(dead_code))]
    pub input_device: Option<String>,
    /// Playback device for the hardware loop TX side; `None` = system default. cpal-only.
    #[cfg_attr(not(feature = "cpal"), allow(dead_code))]
    pub output_device: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            mode: "BPSK250".into(),
            noise_model: NoiseModel::Awgn,
            snr_db: 15.0,
            fec_mode: FecMode::None,
            compression: CompressionAlgorithm::None,
            payload_size: 32,
            seed_str: "42".into(),
            min_db: -100.0,
            max_db: 0.0,
            audio_source: AudioSource::Synthetic,
            input_device: None,
            output_device: None,
        }
    }
}

/// Whether the FEC picker is locked (forced to `None`) for this mode on a given path.
///
/// `engine_path` is `true` for the virtual loop / test matrix (real `ModemEngine`s, which
/// frame the payload before modulation) and `false` for the direct-plugin synthetic / live /
/// hardware paths. FSK4-ACK never carries FEC; OFDM / SC-FDMA carry it only on the engine path
/// — on the direct-plugin path their demodulators emit padded byte counts that are not
/// multiples of 255, which the RS block decoder requires.
pub fn fec_locked(mode: &str, engine_path: bool) -> bool {
    if mode == "FSK4-ACK" {
        return true;
    }
    if mode.starts_with("OFDM") || mode.starts_with("SCFDMA") {
        return !engine_path;
    }
    false
}

pub const ALL_MODES: &[&str] = &[
    // BPSK — HF narrow-band
    "BPSK31",
    "BPSK63",
    "BPSK100",
    "BPSK250",
    // QPSK — HF narrow-band
    "QPSK125",
    "QPSK250",
    "QPSK500",
    "QPSK1000",
    "QPSK1000-HF",
    // 8PSK — HF narrow-band
    "8PSK500",
    "8PSK1000",
    "8PSK1000-HF",
    // 64QAM — HF / full SSB passband (6 bits/sym, up to ~7200 bps eff.)
    "64QAM500",
    "64QAM1000",
    "64QAM2000-RRC",
    // UHF/VHF narrowband — 2000 baud, 8 kHz audio (12.5 kHz channel)
    "QPSK2000",
    "QPSK2000-RRC",
    "8PSK2000",
    "8PSK2000-RRC",
    // RRC variants — HF modes with Root Raised Cosine pulse shaping
    "BPSK250-RRC",
    "QPSK500-RRC",
    "QPSK1000-RRC",
    "8PSK500-RRC",
    "8PSK1000-RRC",
    // ACK channel
    "FSK4-ACK",
    // Multi-carrier
    "OFDM16",
    "OFDM52",
    "SCFDMA16",
    "SCFDMA52",
    // Note: QPSK9600 / 8PSK9600 require 48 kHz sample rate and are not available
    //       in the testbench (which uses 8 kHz).
];

// ── Statistics ────────────────────────────────────────────────────────────────

pub struct TestStats {
    pub runs: u64,
    pub ok: u64,
    pub fail: u64,
    pub total_bits: u64,
    pub error_bits: u64,
    /// Bit errors FEC corrected in the most recent transmission.
    pub last_fec_corrected_bits: u64,
    /// Total channel bit errors entering FEC in the most recent transmission.
    pub last_fec_channel_error_bits: u64,
    /// compressed_bytes / original_bytes for the last run (1.0 = no compression / no saving).
    pub last_compress_ratio: f64,
    /// Sliding window for effective-bitrate calculation: (timestamp, payload_bits_delivered).
    pub rate_window: VecDeque<(std::time::Instant, u64)>,
    pub event_log: VecDeque<String>,
    /// Rolling SNR history: (timestamp, snr_db).  Capacity ~1800 entries (180 s at 10 Hz).
    pub snr_history: VecDeque<(std::time::Instant, f32)>,
    /// Most recent SNR estimate (dB); `None` before the first frame.
    pub current_snr_db: Option<f32>,
    /// Current test-matrix case description (TestMatrix source only); `None` otherwise.
    pub matrix_current: Option<String>,
    /// Mode actually running in the signal thread; drives the bitrate readout so it is
    /// correct even when it differs from the (frozen) UI selection, e.g. in TestMatrix.
    pub active_mode: Option<String>,
    /// FEC actually running in the signal thread (pairs with `active_mode`).
    pub active_fec: FecMode,
}

impl TestStats {
    pub fn new() -> Self {
        Self {
            runs: 0,
            ok: 0,
            fail: 0,
            total_bits: 0,
            error_bits: 0,
            last_fec_corrected_bits: 0,
            last_fec_channel_error_bits: 0,
            last_compress_ratio: 1.0,
            rate_window: VecDeque::new(),
            event_log: VecDeque::new(),
            snr_history: VecDeque::new(),
            current_snr_db: None,
            matrix_current: None,
            active_mode: None,
            active_fec: FecMode::None,
        }
    }

    pub fn ber(&self) -> f32 {
        if self.total_bits == 0 {
            0.0
        } else {
            self.error_bits as f32 / self.total_bits as f32
        }
    }

    /// Fraction of channel bit errors FEC corrected in the last transmission, or `None`
    /// if the last transmission had no channel errors entering the FEC decoder.
    pub fn fec_correction_rate(&self) -> Option<f32> {
        if self.last_fec_channel_error_bits == 0 {
            None
        } else {
            Some(self.last_fec_corrected_bits as f32 / self.last_fec_channel_error_bits as f32)
        }
    }

    pub fn push_event(&mut self, msg: String) {
        if self.event_log.len() >= 200 {
            self.event_log.pop_front();
        }
        self.event_log.push_back(msg);
    }
}

// ── Per-tap data ──────────────────────────────────────────────────────────────

pub struct TapData {
    pub waterfall: WaterfallBuffer,
    pub latest_spectrum: Vec<f32>,
    pub generation: u64,
    /// IQ symbol samples at the decision point; used by the scatter plot (tap[3] only).
    /// Capacity 2000 pairs ≈ 10 s of symbols at 250 baud.
    pub iq_symbols: VecDeque<(f32, f32)>,
}

impl TapData {
    pub fn new() -> Self {
        Self {
            waterfall: WaterfallBuffer::new(WATERFALL_ROWS),
            latest_spectrum: vec![-120.0_f32; FREQ_BINS],
            generation: 0,
            iq_symbols: VecDeque::new(),
        }
    }

    /// Push `samples` through a spectrum analyser and into the waterfall.
    ///
    /// Long sample blocks (BPSK31) are segmented; each `FFT_SIZE` chunk adds
    /// one waterfall row, but only the first chunk updates `latest_spectrum`.
    pub fn push_samples(
        &mut self,
        ps: &mut PowerSpectrum,
        samples: &[f32],
        min_db: f32,
        max_db: f32,
    ) {
        let mut first = true;
        let mut offset = 0;
        while offset < samples.len() {
            let end = (offset + FFT_SIZE).min(samples.len());
            let spectrum = ps.compute(&samples[offset..end]);
            if first {
                self.latest_spectrum = spectrum.clone();
                first = false;
            }
            self.waterfall.push(&spectrum, min_db, max_db);
            offset += FFT_SIZE;
        }
        self.generation += 1;
    }
}

pub type Tap = Arc<RwLock<TapData>>;

// ── Application state ─────────────────────────────────────────────────────────

pub struct AppState {
    pub config: AppConfig,
    pub stats: Arc<RwLock<TestStats>>,
    /// Taps: [0] TX, [1] Noise, [2] Mixed, [3] RX.
    pub taps: [Tap; 4],
    pub stop_tx: Option<crossbeam_channel::Sender<()>>,
    pub running: bool,
    /// Shared config Arc written by the UI each frame; read by the signal thread.
    pub shared_config: Option<Arc<RwLock<AppConfig>>>,
    /// Cached capture-device names for the live-audio selector (cpal-only).
    #[cfg(feature = "cpal")]
    pub input_devices: Vec<String>,
    /// Cached playback-device names for the hardware-loop TX selector (cpal-only).
    #[cfg(feature = "cpal")]
    pub output_devices: Vec<String>,
}

impl AppState {
    pub fn new() -> Self {
        let make_tap = || Arc::new(RwLock::new(TapData::new()));
        Self {
            config: AppConfig::default(),
            stats: Arc::new(RwLock::new(TestStats::new())),
            taps: [make_tap(), make_tap(), make_tap(), make_tap()],
            stop_tx: None,
            running: false,
            shared_config: None,
            #[cfg(feature = "cpal")]
            input_devices: enumerate_input_devices(),
            #[cfg(feature = "cpal")]
            output_devices: enumerate_output_devices(),
        }
    }

    /// Reset all taps and statistics.
    pub fn reset(&mut self) {
        for tap in &self.taps {
            *tap.write().unwrap() = TapData::new();
        }
        *self.stats.write().unwrap() = TestStats::new();
    }

    /// Re-scan the available capture devices (cpal-only).
    #[cfg(feature = "cpal")]
    pub fn refresh_input_devices(&mut self) {
        self.input_devices = enumerate_input_devices();
    }

    /// Re-scan the available playback devices (cpal-only).
    #[cfg(feature = "cpal")]
    pub fn refresh_output_devices(&mut self) {
        self.output_devices = enumerate_output_devices();
    }
}
