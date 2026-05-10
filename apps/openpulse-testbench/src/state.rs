use std::collections::VecDeque;
use std::sync::{Arc, RwLock};

use openpulse_channel::dsp::{PowerSpectrum, WaterfallBuffer, FFT_SIZE, FREQ_BINS, WATERFALL_ROWS};
use openpulse_core::compression::CompressionAlgorithm;
use openpulse_core::fec::{FecMode, BLOCK_DATA_STANDARD, FEC_ECC_LEN, FEC_ECC_LEN_STRONG};

// ── Audio source selector ─────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum AudioSource {
    Synthetic,
    #[cfg(feature = "cpal")]
    LiveCapture,
}

impl AudioSource {
    #[cfg_attr(not(feature = "cpal"), allow(dead_code))]
    pub fn label(&self) -> &'static str {
        match self {
            AudioSource::Synthetic => "Synthetic",
            #[cfg(feature = "cpal")]
            AudioSource::LiveCapture => "Live Audio",
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
        }
    }
}

/// Returns `true` for modes where FEC cannot be applied in the direct-plugin testbench path.
pub fn mode_fec_incompatible(mode: &str) -> bool {
    mode == "FSK4-ACK" || mode.starts_with("OFDM") || mode.starts_with("SCFDMA")
}

/// Maximum payload bytes that can be encoded in one RS block for the given FEC mode.
///
/// The RS encoder prepends a 4-byte (u32 BE) length prefix before splitting into blocks,
/// so the true single-block payload capacity is `block_data_bytes - 4`.
/// Returns `None` when there is no applicable block-size limit (FEC off, ShortRs, Ldpc).
pub fn fec_payload_limit(mode: FecMode) -> Option<usize> {
    // RS block total = BLOCK_DATA_STANDARD + FEC_ECC_LEN = 255 bytes.
    const RS_BLOCK_TOTAL: usize = BLOCK_DATA_STANDARD + FEC_ECC_LEN;
    const RS_PREFIX_LEN: usize = 4; // sizeof(u32) big-endian original-length prefix
    const BLOCK_DATA_STRONG: usize = RS_BLOCK_TOTAL - FEC_ECC_LEN_STRONG; // 191

    match mode {
        FecMode::None => None,
        FecMode::Rs
        | FecMode::RsInterleaved
        | FecMode::Concatenated
        | FecMode::SoftConcatenated => {
            Some(BLOCK_DATA_STANDARD - RS_PREFIX_LEN) // 223 - 4 = 219
        }
        FecMode::RsStrong => Some(BLOCK_DATA_STRONG - RS_PREFIX_LEN), // 191 - 4 = 187
        // ShortRs is fixed-size (5-byte ACK frames only) and manages its own sizing.
        // Ldpc is not yet implemented.
        FecMode::ShortRs | FecMode::Ldpc => None,
    }
}

pub const ALL_MODES: &[&str] = &[
    "BPSK31",
    "BPSK63",
    "BPSK100",
    "BPSK250",
    "QPSK125",
    "QPSK250",
    "QPSK500",
    "QPSK1000",
    "QPSK1000-HF",
    "8PSK500",
    "8PSK1000",
    "8PSK1000-HF",
    "BPSK250-RRC",
    "QPSK500-RRC",
    "QPSK1000-RRC",
    "8PSK500-RRC",
    "8PSK1000-RRC",
    "FSK4-ACK",
    "OFDM16",
    "OFDM52",
    "SCFDMA16",
    "SCFDMA52",
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
        }
    }

    /// Reset all taps and statistics.
    pub fn reset(&mut self) {
        for tap in &self.taps {
            *tap.write().unwrap() = TapData::new();
        }
        *self.stats.write().unwrap() = TestStats::new();
    }
}
