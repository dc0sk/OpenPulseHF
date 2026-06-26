//! Two-station bidirectional ARQ link simulator.
//!
//! Models a realistic half-duplex HF exchange between two stations through independent
//! forward (A→B) and reverse (B→A) channel realizations:
//!
//! - Station A transmits data frames at the current speed level (a [`SessionProfile`] ladder).
//! - Station B decodes, estimates the per-frame SNR, and returns a real FSK4 ACK frame
//!   (`AckOk` / `AckUp` / `AckDown` / `Nack`) through the reverse channel.
//! - Station A steps the speed level up/down its [`SessionProfile`] ladder from the ACKs
//!   (mirroring the `RateAdapter` AckUp/AckDown/NACK-threshold policy, bounded to the
//!   profile's defined levels), and retransmits on NACK (or a lost ACK) up to a retry limit.
//!
//! The simulator accounts for forward air time, ACK air time, turnaround, and
//! retransmissions, yielding the **effective two-way transfer rate** — the goodput a
//! station actually achieves under the simulated conditions, not the raw modem rate.

#[cfg(feature = "serve")]
pub mod serve;

use std::collections::{HashMap, VecDeque};

use openpulse_channel::{
    build_channel, AwgnConfig, ChannelModelConfig, GilbertElliottConfig, QsbConfig, WattersonConfig,
};
use openpulse_core::ack::{AckFrame, AckType};
use openpulse_core::compression::{compress, decompress, CompressionAlgorithm};
use openpulse_core::fec::FecMode;
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::channel_sim::ChannelSimHarness;
use openpulse_modem::ModemEngine;

const SAMPLE_RATE: f64 = 8000.0;
const ACK_MODE: &str = "FSK4-ACK";
const SESSION_ID: &str = "LINKSIM0";
/// Max bytes per modem frame. The frame length field is 8-bit (≤255 payload); 200 keeps the
/// framed bytes within one RS(255,223) block. Larger link payloads are sent as a burst of
/// these chunks (a simple block-ACK ARQ).
const FRAME_CHUNK: usize = 200;
/// Window (frames) over which the rolling frame-success rate is averaged for the rate model.
const RATE_WINDOW: usize = 24;

/// A channel condition for one direction of the link.
#[derive(Debug, Clone)]
pub enum ChannelSpec {
    /// Distortion-free (high-SNR reference).
    Clean,
    /// Additive white Gaussian noise at the given SNR (dB).
    Awgn(f32),
    /// Watterson Good-F1 fading at the given SNR (dB).
    WattersonGoodF1(f32),
    /// Watterson Moderate-F1 fading at the given SNR (dB).
    WattersonModerateF1(f32),
    /// Watterson Poor-F1 fading at the given SNR (dB).
    WattersonPoorF1(f32),
    /// Gilbert-Elliott burst-error channel (moderate) at the given good-state SNR (dB).
    GilbertElliott(f32),
    /// Slow QSB amplitude fading on an AWGN floor at the given SNR (dB).
    Qsb(f32),
    /// Frequency-flat Rayleigh fading (1 Hz Doppler, carrier-phase realistic, no multipath)
    /// at the given SNR (dB).
    FlatFading(f32),
}

impl ChannelSpec {
    fn to_config(&self, seed: u64) -> ChannelModelConfig {
        match *self {
            // "Clean" is modelled as very-high-SNR AWGN so all directions share one path type.
            ChannelSpec::Clean => ChannelModelConfig::Awgn(AwgnConfig {
                snr_db: 60.0,
                seed: Some(seed),
            }),
            ChannelSpec::Awgn(snr) => ChannelModelConfig::Awgn(AwgnConfig {
                snr_db: snr,
                seed: Some(seed),
            }),
            ChannelSpec::WattersonGoodF1(snr) => {
                let mut c = WattersonConfig::good_f1(Some(seed));
                c.snr_db = snr;
                ChannelModelConfig::Watterson(c)
            }
            ChannelSpec::WattersonModerateF1(snr) => {
                let mut c = WattersonConfig::moderate_f1(Some(seed));
                c.snr_db = snr;
                ChannelModelConfig::Watterson(c)
            }
            ChannelSpec::WattersonPoorF1(snr) => {
                let mut c = WattersonConfig::poor_f1(Some(seed));
                c.snr_db = snr;
                ChannelModelConfig::Watterson(c)
            }
            ChannelSpec::GilbertElliott(snr) => {
                let mut c = GilbertElliottConfig::moderate(Some(seed));
                c.snr_good_db = snr;
                c.snr_bad_db = snr - 15.0;
                ChannelModelConfig::GilbertElliott(c)
            }
            // QSB is multiplicative slow fading (no additive noise); the SNR label is
            // informational. AWGN / Watterson cover the additive-noise cases.
            ChannelSpec::Qsb(_snr) => ChannelModelConfig::Qsb(QsbConfig {
                fade_rate_hz: 0.2,
                fade_depth: 0.6,
                sample_rate: 8000,
            }),
            ChannelSpec::FlatFading(snr) => ChannelModelConfig::FlatFading(
                openpulse_channel::flat_fading::FlatFadingConfig::moderate(snr, Some(seed)),
            ),
        }
    }

    /// Short human-readable label.
    pub fn label(&self) -> String {
        match self {
            ChannelSpec::Clean => "clean".into(),
            ChannelSpec::Awgn(s) => format!("AWGN {s:.0}dB"),
            ChannelSpec::WattersonGoodF1(s) => format!("Watt-Good-F1 {s:.0}dB"),
            ChannelSpec::WattersonModerateF1(s) => format!("Watt-Mod-F1 {s:.0}dB"),
            ChannelSpec::WattersonPoorF1(s) => format!("Watt-Poor-F1 {s:.0}dB"),
            ChannelSpec::GilbertElliott(s) => format!("G-E {s:.0}dB"),
            ChannelSpec::Qsb(s) => format!("QSB {s:.0}dB"),
            ChannelSpec::FlatFading(s) => format!("FlatFade {s:.0}dB"),
        }
    }
}

/// Parameters for one link run.
#[derive(Debug, Clone)]
pub struct LinkParams {
    /// SessionProfile name driving the adaptive ladder (see `SessionProfile::PROFILE_NAMES`).
    pub profile_name: String,
    /// Forward (A→B) data channel condition.
    pub forward: ChannelSpec,
    /// Reverse (B→A) ACK channel condition.
    pub reverse: ChannelSpec,
    /// User payload bytes per data frame.
    pub payload_bytes_per_frame: usize,
    /// Number of data frames to attempt to deliver.
    pub total_frames: usize,
    /// FEC applied to data frames.
    pub fec: FecMode,
    /// Payload compression applied before FEC (raises the effective rate on compressible data).
    pub compression: CompressionAlgorithm,
    /// Half-duplex turnaround time per direction switch (seconds) — PTT + sync settle.
    pub turnaround_s: f64,
    /// Maximum transmission attempts per frame before giving up.
    pub max_attempts: u32,
    /// RNG seed for reproducible channel realizations.
    pub seed: u64,
    /// CE-SSB TX envelope conditioning (default on, matching the engine). Only acts on the
    /// modes `ModemEngine::cessb_benefits` enables (OFDM QPSK/8PSK); a no-op elsewhere.
    pub cessb_enabled: bool,
}

impl Default for LinkParams {
    fn default() -> Self {
        Self {
            profile_name: "hpx_hf".into(),
            forward: ChannelSpec::Awgn(15.0),
            reverse: ChannelSpec::Awgn(20.0),
            payload_bytes_per_frame: 64,
            total_frames: 40,
            fec: FecMode::Rs,
            compression: CompressionAlgorithm::None,
            turnaround_s: 0.25,
            max_attempts: 6,
            seed: 0xC0FFEE,
            cessb_enabled: true,
        }
    }
}

/// Per-frame outcome record.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FrameRecord {
    pub frame: usize,
    pub level: u8,
    pub mode: String,
    pub attempts: u32,
    pub delivered: bool,
    pub forward_air_s: f64,
    pub ack_air_s: f64,
    pub est_snr_db: f32,
}

/// Aggregate result of a link run.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LinkResult {
    pub profile: String,
    pub forward: String,
    pub reverse: String,
    pub frames_attempted: usize,
    pub frames_delivered: usize,
    pub bytes_delivered: usize,
    /// Total simulated on-air time: forward + ACK + turnaround across all attempts (seconds).
    pub total_air_s: f64,
    /// Effective two-way goodput: delivered payload bits / total on-air time (bps).
    pub effective_bps: f64,
    /// Delivery ratio (frames delivered / attempted).
    pub delivery_ratio: f64,
    /// Mean speed level used across all attempts.
    pub avg_level: f64,
    /// Final speed level at end of run.
    pub final_level: u8,
    pub records: Vec<FrameRecord>,
}

impl LinkResult {
    /// An all-zero result for a profile with no defined levels.
    fn empty(params: &LinkParams) -> Self {
        Self {
            profile: params.profile_name.clone(),
            forward: params.forward.label(),
            reverse: params.reverse.label(),
            frames_attempted: 0,
            frames_delivered: 0,
            bytes_delivered: 0,
            total_air_s: 0.0,
            effective_bps: 0.0,
            delivery_ratio: 0.0,
            avg_level: 0.0,
            final_level: 0,
            records: Vec::new(),
        }
    }
}

/// One process-wide GPU context shared by every engine's GPU-capable plugins (created lazily;
/// `None` when no adapter is available). Only present with the `gpu` feature.
#[cfg(feature = "gpu")]
fn shared_gpu_context() -> Option<&'static std::sync::Arc<openpulse_gpu::GpuContext>> {
    use std::sync::OnceLock;
    static CTX: OnceLock<Option<std::sync::Arc<openpulse_gpu::GpuContext>>> = OnceLock::new();
    CTX.get_or_init(openpulse_gpu::GpuContext::init).as_ref()
}

fn register_all(engine: &mut ModemEngine) {
    use bpsk_plugin::BpskPlugin;
    use fsk4_plugin::Fsk4Plugin;
    use ofdm_plugin::OfdmPlugin;
    use pilot_plugin::PilotPlugin;
    use psk8_plugin::Psk8Plugin;
    use qam64_plugin::Qam64Plugin;
    use qpsk_plugin::QpskPlugin;
    use scfdma_plugin::ScFdmaPlugin;

    // GPU-capable plugins use the shared GpuContext when built `--features gpu` and an adapter
    // is available; otherwise the CPU path. Non-GPU plugins (FSK4/OFDM/pilot) always use `new`.
    #[cfg(feature = "gpu")]
    macro_rules! reg {
        ($P:ident) => {
            match shared_gpu_context() {
                Some(c) => engine.register_plugin(Box::new($P::with_gpu(c.clone()))),
                None => engine.register_plugin(Box::new($P::new())),
            }
        };
    }
    #[cfg(not(feature = "gpu"))]
    macro_rules! reg {
        ($P:ident) => {
            engine.register_plugin(Box::new($P::new()))
        };
    }

    let _ = reg!(BpskPlugin);
    let _ = reg!(QpskPlugin);
    let _ = reg!(Psk8Plugin);
    let _ = reg!(Qam64Plugin);
    let _ = engine.register_plugin(Box::new(Fsk4Plugin::new()));
    let _ = engine.register_plugin(Box::new(OfdmPlugin::new()));
    // SC-FDMA stays on CPU: its small per-frame 256-pt FFTs are ~1.2–1.3× slower on the GPU
    // (dispatch/readback overhead dominates at HF frame sizes).
    let _ = engine.register_plugin(Box::new(ScFdmaPlugin::new()));
    let _ = engine.register_plugin(Box::new(PilotPlugin::new()));
}

/// FSK4-ACK is the only profile-reachable mode that can't carry RS FEC; everything else
/// (incl. OFDM / SC-FDMA / pilot) carries it on the engine path.
fn fec_for(mode: &str, requested: FecMode) -> FecMode {
    if mode == "FSK4-ACK" {
        FecMode::None
    } else {
        requested
    }
}

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

fn make_plugin(mode: &str) -> Box<dyn ModulationPlugin> {
    use bpsk_plugin::BpskPlugin;
    use fsk4_plugin::Fsk4Plugin;
    use ofdm_plugin::OfdmPlugin;
    use pilot_plugin::PilotPlugin;
    use psk8_plugin::Psk8Plugin;
    use qam64_plugin::Qam64Plugin;
    use qpsk_plugin::QpskPlugin;
    use scfdma_plugin::ScFdmaPlugin;
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
        Box::new(QpskPlugin::new())
    }
}

/// A mode's gross (raw) payload bit rate (bps), measured from the modulator by two-point
/// payload differencing (cancels fixed preamble). `None` if the mode can't be modulated.
pub fn mode_gross_bps(mode: &str) -> Option<f64> {
    let plugin = make_plugin(mode);
    let cfg = ModulationConfig {
        mode: mode.into(),
        center_frequency: 1500.0,
        sample_rate: 8000,
        ..ModulationConfig::default()
    };
    let s1 = plugin.modulate(&[0x5A; 128], &cfg).ok()?.len();
    let s2 = plugin.modulate(&[0x5A; 256], &cfg).ok()?.len();
    if s2 <= s1 {
        return None;
    }
    Some(128.0 * 8.0 * SAMPLE_RATE / (s2 - s1) as f64)
}

/// FEC code rate (net/gross) for a mode's payload after FEC overhead.
pub fn fec_code_rate(fec: FecMode) -> f64 {
    match fec {
        FecMode::None | FecMode::ShortRs | FecMode::Ldpc => 1.0,
        FecMode::Rs | FecMode::RsInterleaved => 223.0 / 255.0,
        FecMode::RsStrong => 191.0 / 255.0,
        FecMode::Concatenated | FecMode::SoftConcatenated => 223.0 / 255.0 * 0.5,
        FecMode::LdpcHighRate => 1024.0 / 1152.0,
        FecMode::Turbo => 1.0 / 3.0,
    }
}

/// Estimate the additive-noise SNR (dB) from the clean reference and the realized
/// post-channel signal. Delegates to [`openpulse_channel::estimate_additive_snr_db`], which
/// removes the multiplicative fading gain first — a naive `|tx|²/|tx-rx|²` counts fading as
/// noise and collapses to ~-3 dB on any fading channel, which would pin the rate adapter at
/// the profile floor regardless of the real SNR.
fn estimate_snr_db(tx: &[f32], rx: &[f32]) -> f32 {
    openpulse_channel::estimate_additive_snr_db(tx, rx)
}

/// Station B's ACK decision from the decode result and the estimated per-frame SNR.
fn decide_ack(
    decode_ok: bool,
    snr_db: f32,
    profile: &SessionProfile,
    level: SpeedLevel,
) -> AckType {
    if !decode_ok {
        return AckType::Nack;
    }
    if let Some(floor) = profile.snr_floor_for_level(level) {
        if snr_db < floor {
            return AckType::AckDown;
        }
    }
    if let Some(ceiling) = profile.snr_ceiling_for_level(level) {
        if snr_db >= ceiling {
            return AckType::AckUp;
        }
    }
    AckType::AckOk
}

/// A compressible, frame-distinct payload so the compression modes show a real advantage.
fn make_payload(frame: usize, attempt: u32, size: usize) -> Vec<u8> {
    let header = format!("OpenPulseHF linksim frame {frame} attempt {attempt}; ");
    const PAT: &[u8] = b"the quick brown fox jumps over the lazy dog. ";
    let mut v = Vec::with_capacity(size + PAT.len());
    v.extend_from_slice(header.as_bytes());
    while v.len() < size {
        v.extend_from_slice(PAT);
    }
    v.truncate(size.max(1));
    v
}

/// Compress per the selected algorithm, falling back to no compression if it doesn't shrink.
fn maybe_compress(payload: &[u8], algo: CompressionAlgorithm) -> (Vec<u8>, CompressionAlgorithm) {
    match algo {
        CompressionAlgorithm::None => (payload.to_vec(), CompressionAlgorithm::None),
        a => {
            let c = compress(payload, a);
            if c.len() < payload.len() {
                (c, a)
            } else {
                (payload.to_vec(), CompressionAlgorithm::None)
            }
        }
    }
}

/// Signals and outcome of one simulated frame (the last attempt's waveforms), for live
/// visualization. Sample vectors are at 8 kHz.
#[derive(Debug, Clone)]
pub struct FrameStep {
    pub frame: usize,
    pub level: u8,
    pub mode: String,
    pub attempts: u32,
    pub delivered: bool,
    pub est_snr_db: f32,
    /// ACK type Station B decided to send.
    pub ack_sent: AckType,
    /// ACK type Station A actually decoded (may differ / be Nack if the ACK was lost).
    pub ack_received: AckType,
    /// Station A's transmitted data waveform (pre-channel).
    pub forward_tx: Vec<f32>,
    /// Data waveform as received at Station B (post forward channel).
    pub forward_rx: Vec<f32>,
    /// Station B's transmitted ACK waveform (pre-channel).
    pub ack_tx: Vec<f32>,
    /// ACK waveform as received at Station A (post reverse channel).
    pub ack_rx: Vec<f32>,
    /// Running effective two-way goodput through this frame (bps).
    pub effective_bps_so_far: f64,
    /// Current mode's gross (raw) payload bit rate (bps).
    pub gross_bps: f64,
    /// Net bit rate after FEC overhead (gross × code rate).
    pub net_bps: f64,
    /// Testbench-style effective rate: net × compression advantage × windowed frame-success.
    /// This is the headline figure shared with the GUI display and the panel feed.
    pub effective_bps: f64,
    /// Windowed frame-success rate (delivered / attempted over the recent window).
    pub success_rate: f64,
    /// This frame's on-air time (forward + ACK + turnaround over all its attempts), seconds.
    pub frame_air_s: f64,
    /// User payload bytes attempted this frame (before compression / FEC).
    pub payload_bytes: usize,
    /// Payload bytes delivered by this frame (0 if it failed) — for windowed throughput.
    pub delivered_bytes: usize,
    /// compressed wire bytes / original payload bytes (≤ 1 when compression helped).
    pub compress_ratio: f64,
    /// Wall-clock time spent in the modem decode (engine receive) for this frame, in ms.
    pub decode_ms: f64,
    /// Speed level the link will use for the next frame.
    pub next_level: u8,
}

/// Step-able two-station link simulation. Drive it with [`step`](Self::step); each call
/// runs one frame (all attempts) and returns the last attempt's waveforms plus the outcome.
/// [`set_conditions`](Self::set_conditions) swaps the channels live (for an interactive SNR
/// control). [`run_link`] runs one to completion and returns the aggregate.
pub struct LinkSim {
    params: LinkParams,
    profile: SessionProfile,
    levels: Vec<SpeedLevel>,
    idx: usize,
    consecutive_nack: u32,
    nack_threshold: u32,
    fwd: ChannelSimHarness,
    rev: ChannelSimHarness,
    fwd_ch: Box<dyn openpulse_channel::ChannelModel>,
    rev_ch: Box<dyn openpulse_channel::ChannelModel>,
    fwd_label: String,
    rev_label: String,
    frame: usize,
    total_air_s: f64,
    bytes_delivered: usize,
    frames_delivered: usize,
    level_sum: u64,
    level_count: u64,
    records: Vec<FrameRecord>,
    /// Measured gross bps per mode (filled lazily; modulation measurement is not free).
    rate_cache: HashMap<String, f64>,
    /// Rolling delivered/failed flags for the windowed frame-success rate.
    success_window: VecDeque<bool>,
}

impl LinkSim {
    /// Build a fresh simulation from `params`.
    pub fn new(params: &LinkParams) -> Self {
        let profile =
            SessionProfile::by_name(&params.profile_name).unwrap_or_else(SessionProfile::hpx_hf);

        let mut fwd = ChannelSimHarness::new();
        register_all(&mut fwd.tx_engine);
        register_all(&mut fwd.rx_engine);
        let mut rev = ChannelSimHarness::new();
        register_all(&mut rev.tx_engine);
        register_all(&mut rev.rx_engine);

        // CE-SSB lives on the transmitting engines (applied in `stage_emit_output`).
        fwd.tx_engine.set_cessb_enabled(params.cessb_enabled);
        rev.tx_engine.set_cessb_enabled(params.cessb_enabled);

        let fwd_ch = build_channel(&params.forward.to_config(params.seed), Some(params.seed))
            .expect("forward channel");
        let rev_ch = build_channel(
            &params.reverse.to_config(params.seed ^ 0x5555),
            Some(params.seed ^ 0x5555),
        )
        .expect("reverse channel");

        // Drive the level over the profile's defined ladder (the global RateAdapter clamps to
        // SL1–SL11, which can leave a profile's sub-range; bound to the profile here while
        // mirroring its AckUp / AckDown / NACK-threshold policy).
        let levels = profile.defined_levels();
        let idx = levels
            .iter()
            .position(|&l| l == profile.initial_level)
            .unwrap_or(0);
        let nack_threshold = profile.nack_threshold.max(1) as u32;

        Self {
            fwd_label: params.forward.label(),
            rev_label: params.reverse.label(),
            params: params.clone(),
            profile,
            levels,
            idx,
            consecutive_nack: 0,
            nack_threshold,
            fwd,
            rev,
            fwd_ch,
            rev_ch,
            frame: 0,
            total_air_s: 0.0,
            bytes_delivered: 0,
            frames_delivered: 0,
            level_sum: 0,
            level_count: 0,
            // Cap the preallocation — total_frames may be usize::MAX for a continuous run.
            records: Vec::with_capacity(params.total_frames.min(4096)),
            rate_cache: HashMap::new(),
            success_window: VecDeque::with_capacity(RATE_WINDOW),
        }
    }

    /// Number of frames processed so far.
    pub fn frames_done(&self) -> usize {
        self.frame
    }

    /// Total frames this run will attempt.
    pub fn total_frames(&self) -> usize {
        self.params.total_frames
    }

    /// Speed level the next frame will use.
    pub fn current_level(&self) -> u8 {
        self.levels.get(self.idx).map(|&l| l as u8).unwrap_or(0)
    }

    /// Toggle CE-SSB TX envelope conditioning live (e.g. from a GUI button) without a rebuild.
    pub fn set_cessb(&mut self, enabled: bool) {
        self.params.cessb_enabled = enabled;
        self.fwd.tx_engine.set_cessb_enabled(enabled);
        self.rev.tx_engine.set_cessb_enabled(enabled);
    }

    /// Whether CE-SSB TX conditioning is currently enabled.
    pub fn cessb_enabled(&self) -> bool {
        self.params.cessb_enabled
    }

    /// Swap the forward / reverse channel conditions live (e.g. from an SNR slider).
    pub fn set_conditions(&mut self, forward: ChannelSpec, reverse: ChannelSpec) {
        if let Ok(c) = build_channel(&forward.to_config(self.params.seed), Some(self.params.seed)) {
            self.fwd_ch = c;
            self.fwd_label = forward.label();
            self.params.forward = forward;
        }
        if let Ok(c) = build_channel(
            &reverse.to_config(self.params.seed ^ 0x5555),
            Some(self.params.seed ^ 0x5555),
        ) {
            self.rev_ch = c;
            self.rev_label = reverse.label();
            self.params.reverse = reverse;
        }
    }

    /// Run one frame (all attempts). Returns `None` once `total_frames` is reached.
    pub fn step(&mut self) -> Option<FrameStep> {
        if self.levels.is_empty() || self.frame >= self.params.total_frames {
            return None;
        }
        let frame = self.frame;
        let mut attempts = 0u32;
        let mut delivered = false;
        let mut fwd_air = 0.0;
        let mut ack_air = 0.0;
        let mut last_level = self.levels[self.idx];
        let mut last_snr = 0.0_f32;
        let mut last_mode = String::new();
        let mut last_compress_ratio = 1.0_f64;
        let mut ack_sent = AckType::Nack;
        let mut ack_received = AckType::Nack;
        let mut decode_ns: u128 = 0;
        let (mut forward_tx, mut forward_rx, mut ack_tx, mut ack_rx) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());

        while attempts < self.params.max_attempts {
            attempts += 1;
            let level = self.levels[self.idx];
            last_level = level;
            self.level_sum += level as u64;
            self.level_count += 1;
            let mode = self
                .profile
                .mode_for(level)
                .expect("defined_levels yields mapped modes")
                .to_string();
            last_mode = mode.clone();
            let fec = fec_for(&mode, self.params.fec);
            let payload = make_payload(frame, attempts, self.params.payload_bytes_per_frame);

            // Compress, then send the wire bytes as a burst of ≤FRAME_CHUNK modem frames
            // (the frame length field is 8-bit). The link frame is delivered only if every
            // chunk decodes — a simple block-ACK ARQ.
            let (wire, algo) = maybe_compress(&payload, self.params.compression);
            last_compress_ratio = wire.len() as f64 / payload.len().max(1) as f64;
            let mut received = Vec::with_capacity(wire.len());
            let mut snr_acc = 0.0_f32;
            let mut chunk_count = 0u32;
            let mut burst_ok = true;
            for chunk in wire.chunks(FRAME_CHUNK) {
                if engine_transmit(&mut self.fwd.tx_engine, chunk, &mode, fec).is_err() {
                    burst_ok = false;
                    break;
                }
                let (tx_s, rx_s) = self.fwd.route_tapped(self.fwd_ch.as_mut());
                fwd_air += tx_s.len() as f64 / SAMPLE_RATE;
                snr_acc += estimate_snr_db(&tx_s, &rx_s);
                chunk_count += 1;
                forward_tx = tx_s;
                forward_rx = rx_s;
                let decode_start = std::time::Instant::now();
                let decoded = engine_receive(&mut self.fwd.rx_engine, &mode, fec);
                decode_ns += decode_start.elapsed().as_nanos();
                match decoded {
                    Ok(b) if b == chunk => received.extend_from_slice(&b),
                    _ => {
                        burst_ok = false;
                        break;
                    }
                }
            }
            let snr = if chunk_count > 0 {
                snr_acc / chunk_count as f32
            } else {
                0.0
            };
            last_snr = snr;
            let decode_ok = burst_ok
                && received == wire
                && decompress(&received, algo)
                    .map(|d| d == payload)
                    .unwrap_or(false);

            // B→A ACK (real FSK4 frame through the reverse channel).
            ack_sent = decide_ack(decode_ok, snr, &self.profile, level);
            let ack_bytes = AckFrame::new(ack_sent, SESSION_ID).encode();
            ack_received =
                if engine_transmit(&mut self.rev.tx_engine, &ack_bytes, ACK_MODE, FecMode::None)
                    .is_ok()
                {
                    let (ack_s, ack_o) = self.rev.route_tapped(self.rev_ch.as_mut());
                    ack_air += ack_s.len() as f64 / SAMPLE_RATE;
                    ack_tx = ack_s;
                    ack_rx = ack_o;
                    engine_receive(&mut self.rev.rx_engine, ACK_MODE, FecMode::None)
                        .ok()
                        .filter(|b| b.len() >= 5)
                        .and_then(|b| {
                            let mut arr = [0u8; 5];
                            arr.copy_from_slice(&b[..5]);
                            AckFrame::decode(&arr).ok()
                        })
                        .map(|f| f.ack_type)
                        .unwrap_or(AckType::Nack)
                } else {
                    AckType::Nack
                };

            match ack_received {
                AckType::AckUp => {
                    self.consecutive_nack = 0;
                    if self.idx + 1 < self.levels.len() {
                        self.idx += 1;
                    }
                }
                AckType::AckDown => {
                    self.consecutive_nack = 0;
                    self.idx = self.idx.saturating_sub(1);
                }
                AckType::AckOk => self.consecutive_nack = 0,
                AckType::Nack => {
                    self.consecutive_nack += 1;
                    if self.consecutive_nack >= self.nack_threshold {
                        self.consecutive_nack = 0;
                        self.idx = self.idx.saturating_sub(1);
                    }
                }
                _ => {}
            }

            if decode_ok {
                delivered = true;
                break;
            }
        }

        if delivered {
            self.frames_delivered += 1;
            self.bytes_delivered += self.params.payload_bytes_per_frame;
        }
        let frame_air_s = fwd_air + ack_air + 2.0 * self.params.turnaround_s * attempts as f64;
        self.total_air_s += frame_air_s;

        // Bound memory for continuous (usize::MAX) runs; keeps the most recent records.
        if self.records.len() >= 8192 {
            self.records.remove(0);
        }
        self.records.push(FrameRecord {
            frame,
            level: last_level as u8,
            mode: last_mode.clone(),
            attempts,
            delivered,
            forward_air_s: fwd_air,
            ack_air_s: ack_air,
            est_snr_db: last_snr,
        });
        self.frame += 1;

        let effective_bps_so_far = if self.total_air_s > 0.0 {
            self.bytes_delivered as f64 * 8.0 / self.total_air_s
        } else {
            0.0
        };

        // Testbench-style rate model (shared by the GUI display and the panel feed so they
        // agree): Gross = mode rate, Net = Gross × code rate, Effective = Net × compression
        // advantage × windowed frame-success.
        self.success_window.push_back(delivered);
        while self.success_window.len() > RATE_WINDOW {
            self.success_window.pop_front();
        }
        let success_rate = if self.success_window.is_empty() {
            0.0
        } else {
            self.success_window.iter().filter(|&&d| d).count() as f64
                / self.success_window.len() as f64
        };
        let gross_bps = *self
            .rate_cache
            .entry(last_mode.clone())
            .or_insert_with(|| mode_gross_bps(&last_mode).unwrap_or(0.0));
        let net_bps = gross_bps * fec_code_rate(self.params.fec);
        let compress_adv = 1.0 / last_compress_ratio.max(1e-9);
        let effective_bps = net_bps * compress_adv * success_rate;

        Some(FrameStep {
            frame,
            level: last_level as u8,
            mode: last_mode,
            attempts,
            delivered,
            est_snr_db: last_snr,
            ack_sent,
            ack_received,
            forward_tx,
            forward_rx,
            ack_tx,
            ack_rx,
            effective_bps_so_far,
            gross_bps,
            net_bps,
            effective_bps,
            success_rate,
            frame_air_s,
            payload_bytes: self.params.payload_bytes_per_frame,
            delivered_bytes: if delivered {
                self.params.payload_bytes_per_frame
            } else {
                0
            },
            compress_ratio: last_compress_ratio,
            decode_ms: decode_ns as f64 / 1.0e6,
            next_level: self.current_level(),
        })
    }

    /// Aggregate result for the frames processed so far.
    pub fn result(&self) -> LinkResult {
        if self.levels.is_empty() {
            return LinkResult::empty(&self.params);
        }
        let effective_bps = if self.total_air_s > 0.0 {
            self.bytes_delivered as f64 * 8.0 / self.total_air_s
        } else {
            0.0
        };
        let avg_level = if self.level_count > 0 {
            self.level_sum as f64 / self.level_count as f64
        } else {
            0.0
        };
        LinkResult {
            profile: self.params.profile_name.clone(),
            forward: self.fwd_label.clone(),
            reverse: self.rev_label.clone(),
            frames_attempted: self.params.total_frames,
            frames_delivered: self.frames_delivered,
            bytes_delivered: self.bytes_delivered,
            total_air_s: self.total_air_s,
            effective_bps,
            delivery_ratio: if self.params.total_frames > 0 {
                self.frames_delivered as f64 / self.params.total_frames as f64
            } else {
                0.0
            },
            avg_level,
            final_level: self.current_level(),
            records: self.records.clone(),
        }
    }
}

/// Run one two-station link to completion and return the effective-throughput result.
pub fn run_link(params: &LinkParams) -> LinkResult {
    let mut sim = LinkSim::new(params);
    while sim.step().is_some() {}
    sim.result()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_channel_delivers_all_and_climbs() {
        let params = LinkParams {
            profile_name: "hpx500".into(),
            forward: ChannelSpec::Clean,
            reverse: ChannelSpec::Clean,
            payload_bytes_per_frame: 32,
            total_frames: 12,
            fec: FecMode::Rs,
            compression: CompressionAlgorithm::None,
            turnaround_s: 0.2,
            max_attempts: 4,
            seed: 1,
            cessb_enabled: true,
        };
        let r = run_link(&params);
        assert_eq!(
            r.frames_delivered, r.frames_attempted,
            "clean link delivers all"
        );
        assert!(r.effective_bps > 0.0, "effective rate must be positive");
        // On a clean channel the rate adapter should have climbed above the initial level.
        assert!(
            r.final_level as usize >= SpeedLevel::Sl2 as usize,
            "level should not drop below the floor on a clean channel"
        );
    }

    #[test]
    fn cessb_toggle_reflects_in_linksim() {
        // The GUI/CLI CE-SSB toggle drives `LinkParams.cessb_enabled` into the TX engines;
        // `set_cessb` flips it live. Lock that the LinkSim honours and reports the setting.
        let params = LinkParams {
            cessb_enabled: false,
            ..LinkParams::default()
        };
        let mut sim = LinkSim::new(&params);
        assert!(!sim.cessb_enabled(), "param should propagate to the sim");
        sim.set_cessb(true);
        assert!(sim.cessb_enabled(), "live toggle should enable it");
        sim.set_cessb(false);
        assert!(!sim.cessb_enabled(), "live toggle should disable it");
    }

    #[test]
    fn effective_rate_below_gross_due_to_ack_and_turnaround() {
        // Even on a clean channel, ACK + turnaround overhead must make the effective
        // two-way rate strictly less than the forward mode's raw payload rate.
        let params = LinkParams {
            profile_name: "hpx500".into(),
            forward: ChannelSpec::Clean,
            reverse: ChannelSpec::Clean,
            payload_bytes_per_frame: 64,
            total_frames: 8,
            fec: FecMode::None,
            compression: CompressionAlgorithm::None,
            turnaround_s: 0.25,
            max_attempts: 3,
            seed: 7,
            cessb_enabled: true,
        };
        let r = run_link(&params);
        assert!(r.frames_delivered > 0);
        // QPSK500 gross is 1000 bps; with ACK + turnaround the goodput is far lower.
        assert!(
            r.effective_bps < 1000.0,
            "effective {:.0} bps should be below the raw mode rate",
            r.effective_bps
        );
    }

    #[test]
    fn very_low_snr_degrades_delivery() {
        let clean = run_link(&LinkParams {
            profile_name: "hpx500".into(),
            forward: ChannelSpec::Clean,
            reverse: ChannelSpec::Clean,
            total_frames: 16,
            seed: 3,
            ..LinkParams::default()
        });
        let noisy = run_link(&LinkParams {
            profile_name: "hpx500".into(),
            forward: ChannelSpec::Awgn(-5.0),
            reverse: ChannelSpec::Awgn(0.0),
            total_frames: 16,
            seed: 3,
            ..LinkParams::default()
        });
        assert!(
            noisy.effective_bps <= clean.effective_bps,
            "a very noisy link must not outperform a clean one ({:.0} vs {:.0})",
            noisy.effective_bps,
            clean.effective_bps
        );
    }

    #[test]
    fn large_payload_chunks_and_delivers() {
        // > 255 bytes must not stall: it is sent as a burst of chunks and still delivered.
        let r = run_link(&LinkParams {
            profile_name: "hpx_wideband".into(), // starts fast (QPSK500), avoids slow BPSK31
            forward: ChannelSpec::Clean,
            reverse: ChannelSpec::Clean,
            payload_bytes_per_frame: 600,
            total_frames: 4,
            fec: FecMode::Rs,
            seed: 11,
            ..LinkParams::default()
        });
        assert_eq!(r.frames_delivered, 4, "all large-payload frames delivered");
        assert!(r.effective_bps > 0.0);
    }

    #[test]
    fn compression_raises_effective_rate() {
        let base = LinkParams {
            profile_name: "hpx_wideband".into(),
            forward: ChannelSpec::Clean,
            reverse: ChannelSpec::Clean,
            payload_bytes_per_frame: 400,
            total_frames: 4,
            fec: FecMode::Rs,
            seed: 5,
            ..LinkParams::default()
        };
        let plain = run_link(&base);
        let zipped = run_link(&LinkParams {
            compression: CompressionAlgorithm::Lz4,
            ..base.clone()
        });
        // make_payload is highly compressible repeating text, so LZ4 must raise goodput.
        assert!(
            zipped.effective_bps > plain.effective_bps,
            "compressed {:.0} should exceed plain {:.0}",
            zipped.effective_bps,
            plain.effective_bps
        );
    }
}
