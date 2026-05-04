use std::collections::VecDeque;
use std::sync::{Arc, RwLock};

use openpulse_channel::dsp::{PowerSpectrum, WaterfallBuffer, FFT_SIZE, FREQ_BINS, WATERFALL_ROWS};

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

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub mode: String,
    pub noise_model: NoiseModel,
    pub snr_db: f32,
    pub fec_enabled: bool,
    pub seed_str: String,
    pub min_db: f32,
    pub max_db: f32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            mode: "BPSK250".into(),
            noise_model: NoiseModel::Awgn,
            snr_db: 15.0,
            fec_enabled: false,
            seed_str: "42".into(),
            min_db: -100.0,
            max_db: 0.0,
        }
    }
}

pub const ALL_MODES: &[&str] = &[
    "BPSK31", "BPSK63", "BPSK100", "BPSK250", "QPSK125", "QPSK250", "QPSK500",
];

// ── Statistics ────────────────────────────────────────────────────────────────

pub struct TestStats {
    pub runs: u64,
    pub ok: u64,
    pub fail: u64,
    pub total_bits: u64,
    pub error_bits: u64,
    pub event_log: VecDeque<String>,
}

impl TestStats {
    pub fn new() -> Self {
        Self {
            runs: 0,
            ok: 0,
            fail: 0,
            total_bits: 0,
            error_bits: 0,
            event_log: VecDeque::new(),
        }
    }

    pub fn ber(&self) -> f32 {
        if self.total_bits == 0 {
            0.0
        } else {
            self.error_bits as f32 / self.total_bits as f32
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
}

impl TapData {
    pub fn new() -> Self {
        Self {
            waterfall: WaterfallBuffer::new(WATERFALL_ROWS),
            latest_spectrum: vec![-120.0_f32; FREQ_BINS],
            generation: 0,
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
