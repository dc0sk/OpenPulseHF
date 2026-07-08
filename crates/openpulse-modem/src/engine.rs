//! The core [`ModemEngine`] struct.

use openpulse_audio::tanh_limit;
use rand::Rng;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tracing::{debug, info};

use openpulse_core::ack::AckFrame;
use openpulse_core::ack::AckType;
use openpulse_core::audio::{AudioBackend, AudioConfig, AudioInputStream};
use openpulse_core::conv::ConvCodec;
use openpulse_core::dcd::DcdState;
use openpulse_core::error::{ModemError, PluginError};
use openpulse_core::fec::{
    apply_window_retransmit, combine_llrs_map, combine_llrs_map_in_ranges,
    encode_window_retransmit, FecCodec, FecMode, Interleaver, ShortFecCodec, SoftCombiner,
    WindowArqFeedback, DEFAULT_INTERLEAVER_DEPTH,
};
use openpulse_core::frame::Frame;
use openpulse_core::hpx::{HpxEvent, HpxSession, HpxState, HpxTransition};
use openpulse_core::ldpc::{IterativeDecoder, LdpcCodec};
use openpulse_core::ota_rate::{OtaRateController, RxOutcome};
use openpulse_core::plugin::{ModulationConfig, PluginRegistry};
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::OtaAggressiveness;
use openpulse_core::rate::RateEvent;
use openpulse_core::rate::SpeedLevel;
use openpulse_core::signed_envelope::SignedEnvelope;
use openpulse_core::soft_viterbi::SoftViterbiCodec;
use openpulse_core::trust::{
    evaluate_handshake, CertificateSource, ConnectionTrustLevel, HandshakeDecision, PolicyProfile,
    PublicKeyTrustLevel, SigningMode,
};
use openpulse_core::turbo::{turbo_decode_soft, turbo_encode, TURBO_MAX_INFO_BYTES};
use openpulse_core::tx_metadata::{TxMetadata, TxSessionLog};
use openpulse_core::wire_query::{callsign_hash, BroadcastFrame, WireEnvelope, WireMsgType};

use crate::event::EngineEvent;
use crate::harq::{HarqDecision, HarqPolicy};
use crate::pipeline::{
    AudioSamples, BackpressurePolicy, DecodedFrame, PipelineMetricsSnapshot, PipelineScheduler,
    PipelineStage, WirePayload,
};
use crate::rate_policy::{RateAdaptationPolicy, RateChangePayload};

#[derive(Debug, Clone)]
pub struct SecureSessionParams {
    pub local_minimum_mode: SigningMode,
    pub peer_supported_modes: Vec<SigningMode>,
    pub key_trust: PublicKeyTrustLevel,
    pub certificate_source: CertificateSource,
    pub psk_validated: bool,
}

/// The modem engine.
///
/// # Example
/// ```no_run
/// use openpulse_modem::ModemEngine;
/// use openpulse_audio::LoopbackBackend;
/// use bpsk_plugin::BpskPlugin;
///
/// let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
/// engine.register_plugin(Box::new(BpskPlugin::new()));
/// engine.transmit(b"Hello", "BPSK100", None).unwrap();
/// let received = engine.receive("BPSK100", None).unwrap();
/// ```
/// Scan/retry policy for [`ModemEngine::receive_with_timeout`], extracted as
/// a pure state machine over (elapsed seconds, buffer length) so the policy is
/// unit-testable without an audio backend.
///
/// Responsibilities: incremental scan-position bookkeeping (never re-try a
/// start position), the post-AFC-settle scan reset back to the first-energy
/// position, and the wall-clock full-buffer retry cadence.
struct ScanPlanner {
    step: usize,
    min_frame_samples: usize,
    last_tried_end: usize,
    first_energy_pos: Option<usize>,
    /// Elapsed-seconds timestamp of the last fired retry.
    last_retry_at_secs: Option<u64>,
}

impl ScanPlanner {
    /// Full-buffer retries start at this elapsed time.  The wall-clock
    /// trigger exists because effective sample rates vary widely between
    /// audio stacks (FT-991A PipeWire: 2 300–7 600 samples/s), making
    /// sample-count thresholds unreliable.
    const RETRY_START_SECS: u64 = 12;
    /// Re-fire cadence: each subsequent retry sees a longer buffer until the
    /// frame fits and the decode succeeds.
    const RETRY_INTERVAL_SECS: u64 = 2;

    fn new(step: usize, min_frame_samples: usize) -> Self {
        Self {
            step,
            min_frame_samples,
            last_tried_end: 0,
            first_energy_pos: None,
            last_retry_at_secs: None,
        }
    }

    /// `true` once AFC settling has located the first signal energy.
    fn is_settled(&self) -> bool {
        self.first_energy_pos.is_some()
    }

    /// The settled first-energy (≈ preamble) position, if settling has occurred.
    fn first_energy_pos(&self) -> Option<usize> {
        self.first_energy_pos
    }

    /// Record a successful AFC settle at `pos` (the refined preamble onset).
    ///
    /// The decode from this position is driven by the dedicated first-energy
    /// re-decode in the receive loop (which re-tries as the buffer grows), so we
    /// do NOT rewind `last_tried_end`: rewinding made the broad scan re-decode a
    /// huge range every time the buffer jumped, stalling the loop.
    fn note_settled(&mut self, pos: usize) {
        self.first_energy_pos = Some(pos);
    }

    /// Untried scan start positions for the current buffer, ending exactly at
    /// the last position that still fits a minimal frame.
    fn scan_positions(&self, buffer_len: usize) -> impl Iterator<Item = usize> + use<> {
        let new_end = buffer_len.saturating_sub(self.min_frame_samples);
        (self.last_tried_end..=new_end).step_by(self.step.max(1))
    }

    /// Mark the current buffer's positions as tried.
    fn commit_scan(&mut self, buffer_len: usize) {
        let new_end = buffer_len.saturating_sub(self.min_frame_samples);
        if new_end > self.last_tried_end {
            self.last_tried_end = new_end;
        }
    }

    /// Whether a full-buffer retry fires now.  Consumes the tick: the next
    /// retry becomes due `RETRY_INTERVAL_SECS` later.
    fn retry_due(&mut self, elapsed_secs: u64, buffer_len: usize) -> bool {
        if buffer_len == 0 || elapsed_secs < Self::RETRY_START_SECS {
            return false;
        }
        let ready = match self.last_retry_at_secs {
            None => true,
            Some(t) => elapsed_secs.saturating_sub(t) >= Self::RETRY_INTERVAL_SECS,
        };
        if ready {
            self.last_retry_at_secs = Some(elapsed_secs);
        }
        ready
    }
}

/// Settled AFC corrections below this magnitude (Hz) are treated as measurement
/// noise and snapped to zero.  A short data-aided/blind estimate on a zero-offset
/// frame lands a few tenths of a Hz off; applying that spurious correction breaks
/// modes that re-fit carrier phase from the (now over-corrected) preamble — 8PSK's
/// `carrier_phase_correct` enters a fragile drift-fit branch at ≥0.5 Hz.  Real HF
/// offsets are tens to hundreds of Hz (the carrier-offset regression uses 15 Hz;
/// the measured inter-rig offset is ~400 Hz), so this never suppresses a real one.
const AFC_SETTLE_DEADBAND_HZ: f32 = 2.0;

/// Result of [`ModemEngine::afc_mini_settle`].
struct AfcSettleOutcome {
    /// Correction after the one-shot wide-scan anchor pass.
    anchor: f32,
    /// Correction after the fine-tracking passes.
    fine: f32,
    /// Absolute change introduced by the final fine pass (convergence check).
    last_delta: f32,
}

/// Adaptive scan energy gate: an absolute floor plus a noise-floor-relative
/// threshold.
///
/// The fixed 1e-4 mean-square gate passes every position when the band noise
/// floor is elevated (on-air QRM ≈ 1.5e-3), firing the expensive AFC
/// mini-settle at each scan step.  The gate keeps a short history of window
/// energies and uses the 25th percentile as the noise-floor estimate (robust
/// to up to 75% signal-bearing windows in the history), gating at 3× that
/// floor.  The threshold is clamped to [1e-4, 3.2e-3] so it can never rise
/// above the weakest decodable loopback signal level, and the adaptive part
/// only engages once enough history exists to be a genuine noise estimate.
struct EnergyGate {
    history: std::collections::VecDeque<f32>,
}

impl EnergyGate {
    /// Legacy absolute floor (DcdState default: 0.01 RMS → 1e-4 mean-square).
    const ABS_THRESHOLD: f32 = 0.0001;
    /// Upper clamp: below loopback signal levels (mean-square ≈ 1e-3 … 5e-3).
    const MAX_THRESHOLD: f32 = 0.0032;
    const HISTORY: usize = 128;
    const MIN_HISTORY: usize = 32;

    fn new() -> Self {
        Self {
            history: std::collections::VecDeque::with_capacity(Self::HISTORY),
        }
    }

    fn threshold(&self) -> f32 {
        if self.history.len() < Self::MIN_HISTORY {
            return Self::ABS_THRESHOLD;
        }
        let mut sorted: Vec<f32> = self.history.iter().copied().collect();
        sorted.sort_by(f32::total_cmp);
        let floor = sorted[sorted.len() / 4];
        (floor * 3.0).clamp(Self::ABS_THRESHOLD, Self::MAX_THRESHOLD)
    }

    /// Record one gate-window energy and return whether it passes the gate.
    fn passes(&mut self, mean_sq: f32) -> bool {
        let thr = self.threshold();
        if self.history.len() == Self::HISTORY {
            self.history.pop_front();
        }
        self.history.push_back(mean_sq);
        mean_sq >= thr
    }
}

/// Refine a coarse first-energy position to the actual signal onset.
///
/// The energy gate's wide window (`acq_samples`, ~32 symbols) trips up to a full
/// window before the true onset — its tail catches the first signal samples — so
/// the coarse position can sit a whole acquisition window ahead of the preamble,
/// far beyond the demodulator's one-symbol timing search.  Scan symbol-length
/// sub-windows across the gate span and return the first whose energy reaches a
/// quarter of the span's peak (where the signal turns on), so the preamble lands
/// within one symbol period of the returned position.
fn refine_onset(buf: &[f32], start: usize, span: usize, step: usize) -> usize {
    let end = (start + span).min(buf.len());
    if step == 0 || end <= start + step {
        return start;
    }
    let energy = |p: usize| -> f32 {
        let e = (p + step).min(buf.len());
        buf[p..e].iter().map(|s| s * s).sum::<f32>() / (e - p) as f32
    };
    let positions: Vec<usize> = (start..end).step_by(step).collect();
    let peak = positions.iter().map(|&p| energy(p)).fold(0.0f32, f32::max);
    if peak <= 0.0 {
        return start;
    }
    positions
        .into_iter()
        .find(|&p| energy(p) >= peak * 0.25)
        .unwrap_or(start)
}

/// Internal return of [`ModemEngine::ota_decode_and_ack`]: the decoded
/// payload+mode (if any), the ACK frame to send, and the last decode error.
type OtaDecodeOutcome = (Option<(Vec<u8>, String)>, AckFrame, Option<ModemError>);

/// Outcome of one daemon-facing OTA receive poll ([`ModemEngine::poll_ota_rx`]):
/// the decode result plus the ACK frame the caller must transmit back.
#[derive(Debug, Clone)]
pub struct OtaRxResult {
    /// Decoded payload, or `None` when every candidate failed (the ACK is a Nack).
    pub payload: Option<Vec<u8>>,
    /// ACK frame to transmit back to the sender (key PTT around the transmit).
    pub ack: AckFrame,
    /// Mode string a candidate decoded at, for event reporting.
    pub mode: Option<String>,
}

pub struct ModemEngine {
    audio: Box<dyn AudioBackend>,
    plugins: PluginRegistry,
    sequence: u16,
    hpx: HpxSession,
    scheduler: PipelineScheduler,
    trust_policy_profile: PolicyProfile,
    active_handshake: Option<HandshakeDecision>,
    /// Residual carrier frequency error measured at the corrected reference from
    /// the most recent demodulation call.  This is the error *after* applying
    /// `afc_correction_hz`; the total offset from the nominal centre frequency is
    /// approximately `afc_correction_hz + last_afc_offset_hz`.
    last_afc_offset_hz: Option<f32>,
    /// Accumulated AFC carrier correction applied to demodulation (Hz).
    afc_correction_hz: f32,
    /// Whether the AFC tracking loop is active (default: true).
    afc_enabled: bool,
    /// Fraction of the estimated offset applied to the correction each frame.
    afc_step: f32,
    /// Audio centre frequency used for modulation and demodulation (Hz).
    center_frequency: f32,
    rate_policy: RateAdaptationPolicy,
    /// Receiver-led OTA rate controller (per-direction lockstep); `None` until
    /// [`start_ota_session`](ModemEngine::start_ota_session) is called.
    ota: Option<OtaRateController>,
    /// Externally-supplied RX SNR estimate (dB) for OTA adaptive decisions.
    /// When `None`, the weak LLR-magnitude proxy is used. A real estimator
    /// (or a channel-sim harness) should feed this for meaningful stepping.
    rx_snr_estimate: Option<f32>,
    dcd: DcdState,
    csma_enabled: bool,
    csma_persistence: f32,
    event_tx: broadcast::Sender<EngineEvent>,
    /// Sender-local sequence counter for broadcast frames.
    broadcast_seq: u16,
    /// Callsign used in broadcast frame headers (set via `set_callsign`).
    callsign: String,
    /// TX attenuation in dB applied to output samples (0.0 = no attenuation).
    tx_attenuation_db: f32,
    /// Soft TX limiter threshold (0.0 = disabled). See `tanh_limit`.
    tx_limiter_threshold: f32,
    /// Maximum TX power in watts for regulatory compliance (0.0 = no limit).
    max_power_watts: f32,
    /// Transmission metadata log for regulatory compliance (station_id, timestamps).
    tx_session_log: TxSessionLog,
    /// Default audio device name used when a per-call `device` is `None`.
    /// Lets a daemon pin its engine to a specific capture/playback device (e.g. an
    /// `snd-aloop` PCM) without threading the name through every transmit/receive.
    default_device: Option<String>,
    /// Most recent audio window the engine captured (RX) or emitted (TX), bounded
    /// to [`SPECTRUM_TAP_MAX`] samples. A spectrum/waterfall consumer (e.g. the
    /// daemon's control-port broadcast) reads this so the FFT is of real audio, not
    /// silence. Empty until the first transmit/receive.
    last_audio: Vec<f32>,
    /// [`capture_burst`](ModemEngine::capture_burst) accumulator: samples gathered
    /// across receive ticks while a carrier is present, flushed for decode when it
    /// drops. Lets a tick-based daemon assemble a full frame from a streaming
    /// (cpal) backend instead of decoding one partial tick window.
    rx_burst: Vec<f32>,
    /// Whether [`capture_burst`](ModemEngine::capture_burst) is mid-burst (carrier
    /// was present on a prior tick and not yet flushed).
    rx_capturing: bool,
    /// Master enable for CE-SSB TX envelope conditioning. Default on; it only acts
    /// on modes that benefit (multicarrier — see [`cessb_benefits`]), so it is a
    /// no-op for single-carrier modes regardless.
    cessb_enabled: bool,
    /// Receiver-side automatic notch on captured audio (default off). Removes out-of-band CW
    /// interference (QRM) before demod; its protected band tracks the active mode's occupied
    /// bandwidth so the signal is never notched. See `docs/dev/notch-equalizer-experiment.md`.
    notch_enabled: bool,
    /// The notch bank, used only while `notch_enabled`.
    notch_bank: openpulse_dsp::notch::NotchBank,
    /// Protected-band full bandwidth (Hz) used when the active mode can't report its occupied
    /// bandwidth (e.g. multicarrier modes, or a mode-agnostic capture); ± half this around the
    /// carrier is never notched.
    notch_fallback_bw_hz: f32,
    /// Confirmed in-band interferers (Hz) from the notch persistence tracker — a notch can't
    /// remove these, so they are QSY (move-frequency) candidates. Empty unless persistence is on.
    notch_in_band_interferers: Vec<f32>,
    /// Active mode for the receiver front end, set by the capture entry points and read at the
    /// `PipelineStage::InputCapture` notch seam (where the mode isn't otherwise in scope).
    rx_mode: Option<String>,
    /// Count of capture blocks the notch processed — a tripwire: an enabled notch that never runs
    /// on a given path (e.g. a new capture path that skips the InputCapture seam) leaves this at 0.
    notch_blocks_processed: u64,
    /// Receiver-side streaming AGC on captured audio (default off). Normalises the level so the
    /// PSK/QAM ladder sees a consistent amplitude despite QSB fading and inter-station spread.
    /// Active-span gated: the gain only adapts on carrier-present blocks (RMS ≥ DCD threshold) and
    /// is frozen through silence, so a long leading gap can't ramp it to its clamp before the burst.
    agc_enabled: bool,
    /// The AGC loop, used only while `agc_enabled`.
    agc: openpulse_dsp::agc::Agc,
    /// Count of capture blocks the AGC processed — same tripwire role as `notch_blocks_processed`.
    agc_blocks_processed: u64,
    /// Count of capture blocks the DC block processed — tripwire for the always-on DC removal
    /// (REQ-PHY-02); stays 0 if a capture path ever skips the InputCapture seam.
    dc_blocks_processed: u64,
    /// Monotonic count of frames emitted at the single TX seam (`stage_emit_output`) — every
    /// transmit path (data, FEC, ACK, retransmit, QSY, ID) increments it once. A pollable
    /// TX-activity signal for the daemon's periodic station-ID timer (REQ-REG-10).
    frames_transmitted: u64,
}

/// CE-SSB TX conditioning clip level as a multiple of the RMS envelope. 2.0×
/// recovered ~2.7 dB average power on OFDM at zero BER cost in the channel-sim
/// measurement (`tests/cessb_power_evm.rs`).
const CESSB_CLIP_RATIO: f32 = 2.0;
/// Peak-stretcher look-ahead window (samples) for CE-SSB TX conditioning.
const CESSB_LOOKAHEAD: usize = 16;

/// Safety cap on the [`ModemEngine::capture_burst`] accumulator (~30 s at 8 kHz):
/// if a carrier never "drops" (e.g. DCD threshold below the noise floor) the burst
/// is force-flushed rather than growing without bound.
const BURST_MAX_SAMPLES: usize = 240_000;

/// Cap on the [`ModemEngine::last_audio`] window — a few FFT frames is plenty for a
/// representative spectrum row and bounds the per-call clone.
const SPECTRUM_TAP_MAX: usize = 16384;

impl ModemEngine {
    /// Create a new engine backed by the given audio backend.
    pub fn new(audio: Box<dyn AudioBackend>) -> Self {
        let (event_tx, _) = broadcast::channel(64);
        Self {
            audio,
            plugins: PluginRegistry::new(),
            sequence: 0,
            hpx: HpxSession::new(),
            scheduler: PipelineScheduler::new(8, BackpressurePolicy::Block),
            trust_policy_profile: PolicyProfile::Balanced,
            active_handshake: None,
            last_afc_offset_hz: None,
            afc_correction_hz: 0.0,
            afc_enabled: true,
            afc_step: 0.1,
            center_frequency: 1500.0,
            rate_policy: RateAdaptationPolicy::new(),
            ota: None,
            rx_snr_estimate: None,
            dcd: DcdState::new(0.01, 800), // 100 ms hold at 8 kHz
            csma_enabled: false,
            csma_persistence: 0.3,
            event_tx,
            broadcast_seq: 0,
            callsign: String::new(),
            tx_attenuation_db: 0.0,
            tx_limiter_threshold: 0.0,
            max_power_watts: 0.0, // 0.0 means no limit
            tx_session_log: TxSessionLog::new("UNKNOWN"),
            default_device: None,
            last_audio: Vec::new(),
            rx_burst: Vec::new(),
            rx_capturing: false,
            cessb_enabled: true,
            notch_enabled: false,
            notch_bank: openpulse_dsp::notch::NotchBank::new(
                openpulse_dsp::notch::NotchParams::default(),
            ),
            notch_fallback_bw_hz: 2000.0,
            notch_in_band_interferers: Vec::new(),
            rx_mode: None,
            notch_blocks_processed: 0,
            agc_enabled: false,
            // target RMS 0.3 (headroom below ±1.0), slow loop (α=0.02), ±40 dB clamp.
            agc: openpulse_dsp::agc::Agc::new(0.3, 0.02, 40.0),
            agc_blocks_processed: 0,
            dc_blocks_processed: 0,
            frames_transmitted: 0,
        }
    }

    /// Enable the receiver-side automatic notch (removes out-of-band CW interference before
    /// demod). Off by default. The protected band tracks the active mode so the signal is never
    /// notched; an in-band interferer still can't be removed (that is a QSY case).
    pub fn enable_notch(&mut self) {
        self.notch_enabled = true;
    }

    /// Disable the receiver-side automatic notch.
    pub fn disable_notch(&mut self) {
        self.notch_enabled = false;
    }

    /// Whether the receiver-side automatic notch is enabled.
    pub fn is_notch_enabled(&self) -> bool {
        self.notch_enabled
    }

    /// Number of capture blocks the notch has processed. A tripwire for the "feature wired at a
    /// seam the runtime path skips" class of gap: if the notch is enabled but this stays 0 while
    /// the daemon runs, the receive path isn't reaching the front-end seam.
    pub fn notch_blocks_processed(&self) -> u64 {
        self.notch_blocks_processed
    }

    /// Monotonic count of frames emitted at the TX seam. The daemon polls the delta to detect
    /// transmit activity for the periodic station-ID timer (REQ-REG-10) without threading a
    /// `note_tx()` call through every transmit call site.
    pub fn frames_transmitted(&self) -> u64 {
        self.frames_transmitted
    }

    /// Emit a Morse CW station identification (keyed sine) for `text` through the single TX seam —
    /// used to honour the ARDOP `CWID` option alongside the digital ID. Counts as a transmitted
    /// frame (`frames_transmitted`). No-op (returns `Ok`) when `text` has no renderable characters.
    pub fn emit_cw_id(&mut self, text: &str, device: Option<&str>) -> Result<(), ModemError> {
        let fs = AudioConfig::default().sample_rate;
        let samples = openpulse_core::cw_id::CwId::default().samples(text, fs);
        if samples.is_empty() {
            return Ok(());
        }
        let routed = self.route_audio_stage(PipelineStage::OutputEmit, AudioSamples { samples })?;
        self.stage_emit_output(device, "CW", &routed)
    }

    /// Configure the notch bank: max simultaneous notches, sharpness `q` (BW ≈ f0/q), and the
    /// protected-band fallback bandwidth (Hz) used when the active mode can't report its own.
    pub fn configure_notch(&mut self, max_notches: usize, q: f32, fallback_bw_hz: f32) {
        use openpulse_dsp::notch::{NotchBank, NotchParams};
        self.notch_bank = NotchBank::new(NotchParams {
            max_notches,
            q,
            ..NotchParams::default()
        });
        self.notch_fallback_bw_hz = fallback_bw_hz;
    }

    /// Enable notch persistence/silence tracking: a tone must appear in this many signal-absent
    /// blocks before it counts as a confirmed external interferer. 0 disables it (default). This
    /// lets the notch null externally-confirmed tones robustly, and surfaces in-band ones via
    /// [`in_band_interferers`](Self::in_band_interferers) for QSY.
    pub fn set_notch_persistence(&mut self, min_silence_hits: u32) {
        self.notch_bank.set_persistence(min_silence_hits);
    }

    /// Confirmed in-band interferers (Hz): a notch can't remove these without harming the signal,
    /// so they are QSY (move-frequency) candidates. Empty unless notch persistence is enabled.
    pub fn in_band_interferers(&self) -> &[f32] {
        &self.notch_in_band_interferers
    }

    /// Forget the confirmed in-band interferers and the notch persistence state — e.g. after a
    /// QSY to a new frequency, where the old interferers no longer apply.
    pub fn clear_in_band_interferers(&mut self) {
        self.notch_in_band_interferers.clear();
        self.notch_bank.clear_persistence();
    }

    /// Centre frequencies (Hz) of the notches placed on the most recent captured block.
    pub fn notch_active_freqs(&self) -> Vec<f32> {
        self.notch_bank.active_freqs()
    }

    /// Apply the receiver notch to a captured block: protect the active mode's occupied band
    /// (so the signal is never notched), then null out-of-band CW interferers. When persistence
    /// is on, feed the block to the silence tracker and surface any confirmed in-band interferer
    /// (a QSY case the notch can't fix).
    fn apply_rx_notch(&mut self, mode: Option<&str>, samples: Vec<f32>) -> Vec<f32> {
        let center = self.center_frequency + self.afc_correction_hz;
        let bw = mode
            .and_then(|m| self.plugins.get(m).and_then(|p| p.occupied_bandwidth_hz(m)))
            .unwrap_or(self.notch_fallback_bw_hz);
        let half = bw / 2.0;
        self.notch_bank
            .set_protect_band((center - half).max(0.0), center + half);

        // Persistence: the bank classifies the block (our wideband signal fills the protected
        // band; a lone CW tone does not), so it can tell an external interferer from our own lines.
        self.notch_bank.observe(&samples);
        let in_band = self.notch_bank.in_band_interferers();
        if in_band != self.notch_in_band_interferers {
            if !in_band.is_empty() {
                tracing::warn!(freqs_hz = ?in_band, "in-band interference confirmed; a notch cannot remove it — QSY recommended");
            }
            self.notch_in_band_interferers = in_band;
        }
        self.notch_bank.process_block(&samples)
    }

    /// Enable the receiver-side streaming AGC (level normalisation before demod). Off by default.
    pub fn enable_agc(&mut self) {
        self.agc_enabled = true;
    }

    /// Disable the receiver-side streaming AGC.
    pub fn disable_agc(&mut self) {
        self.agc_enabled = false;
        self.agc.reset();
    }

    /// Whether the receiver-side streaming AGC is enabled.
    pub fn is_agc_enabled(&self) -> bool {
        self.agc_enabled
    }

    /// Configure the AGC loop: target output RMS, adaptation rate `bandwidth` (α in (0,1]), and the
    /// symmetric gain clamp in dB. Resets the loop. See [`openpulse_dsp::agc::Agc::new`].
    pub fn configure_agc(&mut self, target_rms: f32, bandwidth: f32, max_gain_db: f32) {
        self.agc = openpulse_dsp::agc::Agc::new(target_rms, bandwidth, max_gain_db);
    }

    /// Number of capture blocks the AGC has processed — a tripwire for the "feature wired at a seam
    /// the runtime path skips" class of gap (see [`Self::notch_blocks_processed`]).
    pub fn agc_blocks_processed(&self) -> u64 {
        self.agc_blocks_processed
    }

    /// Number of capture blocks the DC block (REQ-PHY-02) has processed — a tripwire that the
    /// always-on DC removal runs on every receive path (it lives at the single InputCapture seam).
    pub fn dc_blocks_processed(&self) -> u64 {
        self.dc_blocks_processed
    }

    /// Current AGC gain in dB (0 dB = unity). A readout of the active-span loop state.
    pub fn agc_gain_db(&self) -> f32 {
        self.agc.gain_db()
    }

    /// Apply the streaming AGC to one capture block, active-span gated: the gain only adapts on
    /// carrier-present blocks (RMS ≥ DCD squelch) and is frozen through silence, so a long leading
    /// gap can't ramp the gain to its clamp before the burst arrives.
    fn apply_rx_agc(&mut self, mut samples: Vec<f32>) -> Vec<f32> {
        let n = samples.len();
        let rms = if n == 0 {
            0.0
        } else {
            (samples.iter().map(|s| s * s).sum::<f32>() / n as f32).sqrt()
        };
        if rms >= self.dcd.threshold() {
            self.agc.unlock();
        } else {
            self.agc.lock();
        }
        self.agc.process(&mut samples);
        samples
    }

    /// Enable/disable CE-SSB TX envelope conditioning (master switch). It still
    /// only acts on modes that benefit ([`cessb_benefits`](Self::cessb_benefits)).
    pub fn set_cessb_enabled(&mut self, enabled: bool) {
        self.cessb_enabled = enabled;
    }

    /// Whether CE-SSB TX conditioning is enabled (master switch).
    pub fn cessb_enabled(&self) -> bool {
        self.cessb_enabled
    }

    /// Whether `mode` benefits from CE-SSB conditioning — **only the QPSK-subcarrier
    /// OFDM waveforms** (`OFDM16`, `OFDM52`). Every denser constellation and every
    /// single-carrier waveform is excluded, each decided by end-to-end decode through
    /// the real engine+channel path, not synthetic raw-BER:
    ///
    /// 1. **SC-FDMA is excluded entirely.** Despite its multicarrier subcarrier
    ///    structure it is a *single-carrier* FDM waveform, low-PAPR by construction,
    ///    so CE-SSB recovers only ~⅓ of OFDM's average-power gain (2.6 vs 8.5 dB at
    ///    the 2.0×rms operating point on the 64QAM rung) while its EVM alone injects
    ///    ~0.5 % raw BER — collapsing decode of SCFDMA52-{32,64}QAM (5/30 vs 30/30
    ///    through AWGN 35 dB).
    /// 2. **OFDM ≥16QAM is excluded.** Their decision regions are too tight for the
    ///    CE-SSB EVM. 32/64QAM collapse outright (OFDM52-32QAM 0/20, -64QAM 3/20 vs
    ///    20/20 off, soft FEC through AWGN); 16QAM is marginal — it survives easy
    ///    AWGN but breaks on the realistic HF fading path (OFDM52-16QAM soft-FEC
    ///    Watterson Good-F1: 0/16 on vs 16/16 off; uncoded AWGN 20 dB: 0/20 vs
    ///    20/20). The earlier `cessb_benefits_hold_on_ofdm_hom` claim measured raw
    ///    BER at a fixed operating point and missed the acquisition/decode failure
    ///    on the real path (DSP playbook: validate FEC-protected modes WITH FEC,
    ///    and against the fading channel — dense constellations are the canaries).
    ///
    /// The exclusions all trace to one principle: **CE-SSB trades in-band EVM for
    /// average-power gain, and that trade only wins where the envelope is high-PAPR
    /// *and* the decision margins are loose.** QPSK-subcarrier OFDM sums ~52 carriers
    /// into a near-Gaussian envelope that rarely nulls hard, so envelope limiting costs
    /// almost no EVM; higher-order (8PSK/QAM/APSK) subcarriers and single-carrier QAM
    /// transit near the constellation origin, where the envelope passes through zero and
    /// the instantaneous phase goes discontinuous (the "equal-amplitude" singularity —
    /// *Dave's Hacks*, Feb 2025, catalogued in `docs/dev/research/references.md`).
    /// Limiting that envelope injects EVM their tighter slicers can't absorb, so CE-SSB
    /// is gated OFF for them — 8PSK included (a marginal-SNR sweep goes 12/12 → 0/12
    /// with CE-SSB on, and decodes only once gated off).
    /// Measured in `openpulse-linksim/tests/cessb_ab.rs` and `tests/cessb_power_evm.rs`.
    pub fn cessb_benefits(mode: &str) -> bool {
        let m = mode.to_ascii_uppercase();
        if !m.starts_with("OFDM") {
            return false;
        }
        // Only the QPSK-subcarrier OFDM modes (OFDM16, OFDM52) tolerate the clip. Every
        // higher-order constellation is gated off: the in-band clipping distortion exceeds the
        // tighter decision margins, costing several dB — peak-fair `cessb_power_evm` shows
        // OFDM52-8PSK going BER 0.0000→0.0026, and a marginal-SNR sweep has it fail entirely with
        // CE-SSB on (12/12 → 0/12 at 12–16 dB) but decode once gated off.
        !(m.contains("8PSK")
            || m.contains("16QAM")
            || m.contains("32QAM")
            || m.contains("64QAM")
            || m.contains("32APSK"))
    }

    /// Apply CE-SSB envelope conditioning to a real passband TX block and rescale
    /// to the original peak, so the freed headroom becomes average power at the same
    /// PEP. Returns the input unchanged if the envelope is degenerate.
    fn cessb_condition_tx(&self, samples: &[f32]) -> Vec<f32> {
        let fs = AudioConfig::default().sample_rate as f32;
        let (i, q) = openpulse_core::iq::hilbert_iq(samples, self.center_frequency, fs);
        let env = openpulse_dsp::cessb::envelope(&i, &q);
        let rms_env = (env.iter().map(|e| e * e).sum::<f32>() / env.len().max(1) as f32).sqrt();
        if rms_env <= f32::MIN_POSITIVE {
            return samples.to_vec();
        }
        let level = CESSB_CLIP_RATIO * rms_env;
        let gain = openpulse_dsp::cessb::peak_stretch_gain(&env, level, CESSB_LOOKAHEAD);
        let mut out = openpulse_dsp::cessb::apply_gain(samples, &gain);
        // Restore the original peak: the average-power gain is realised by scaling
        // the now-lower-PAPR signal back up to the same peak (PEP).
        let p0 = samples.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
        let p1 = out.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
        if p0 > 0.0 && p1 > f32::MIN_POSITIVE {
            let scale = p0 / p1;
            for x in &mut out {
                *x *= scale;
            }
        }
        out
    }

    /// Most recent audio window the engine captured (RX) or emitted (TX).
    ///
    /// Bounded to the last [`SPECTRUM_TAP_MAX`] samples; empty until the first
    /// transmit/receive. Intended for a spectrum/waterfall tap so the FFT sees real
    /// audio rather than silence.
    pub fn last_audio(&self) -> &[f32] {
        &self.last_audio
    }

    /// Record the most recent audio window for the spectrum tap (keeps the tail).
    fn record_audio(&mut self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        let start = samples.len().saturating_sub(SPECTRUM_TAP_MAX);
        self.last_audio.clear();
        self.last_audio.extend_from_slice(&samples[start..]);
    }

    /// Pin all audio I/O to `device` (by backend device name) when a per-call
    /// `device` argument is `None`. Pass `None` to clear (use the backend default).
    ///
    /// Used by the daemon to bind one engine to a specific full-duplex device — the
    /// real-audio twin-station rig points station A at one `snd-aloop` PCM and
    /// station B at the crossed PCM so the kernel routes A↔B.
    pub fn set_default_device(&mut self, device: Option<String>) {
        self.default_device = device;
    }

    /// Subscribe to the real-time engine event stream.
    ///
    /// Returns a [`broadcast::Receiver`] that receives every [`EngineEvent`]
    /// emitted after this call.  If a receiver falls behind, `try_recv()` returns
    /// `TryRecvError::Lagged(n)` indicating the number of dropped events; callers
    /// must handle this variant explicitly.
    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.event_tx.subscribe()
    }

    /// Returns the active trust policy profile used as session default.
    pub fn trust_policy_profile(&self) -> PolicyProfile {
        self.trust_policy_profile
    }

    /// Sets the active trust policy profile used as session default.
    pub fn set_trust_policy_profile(&mut self, profile: PolicyProfile) {
        self.trust_policy_profile = profile;
    }

    /// Returns the residual carrier frequency error measured at the corrected
    /// reference from the most recent demodulation call, in Hz.
    ///
    /// This is the error *after* `afc_correction_hz` has been applied.  The
    /// total offset from the nominal centre frequency is approximately
    /// `afc_correction_hz() + last_afc_offset_hz()`.  Returns `None` until the
    /// first receive or if the active plugin does not support AFC.
    pub fn last_afc_offset_hz(&self) -> Option<f32> {
        self.last_afc_offset_hz
    }

    /// Returns the accumulated AFC carrier correction applied to demodulation (Hz).
    pub fn afc_correction_hz(&self) -> f32 {
        self.afc_correction_hz
    }

    /// Sets the audio centre frequency used for modulation and demodulation.
    pub fn set_center_frequency(&mut self, hz: f32) {
        self.center_frequency = hz;
    }

    /// Returns the audio centre frequency.
    pub fn center_frequency(&self) -> f32 {
        self.center_frequency
    }

    /// Enable the AFC tracking loop (default: enabled).
    pub fn enable_afc(&mut self) {
        self.afc_enabled = true;
    }

    /// Disable the AFC tracking loop.
    pub fn disable_afc(&mut self) {
        self.afc_enabled = false;
    }

    /// Reset the accumulated AFC correction and offset estimate to zero.
    pub fn reset_afc(&mut self) {
        self.afc_correction_hz = 0.0;
        self.last_afc_offset_hz = None;
    }

    /// Enable 0.3-persistence CSMA channel access control.
    ///
    /// When enabled, [`transmit`](Self::transmit) checks the DCD state before
    /// emitting audio.  If the channel is busy, or if the random p-persistence
    /// draw fails (70% of the time on a clear channel), it returns
    /// [`ModemError::ChannelBusy`] and the caller should back off and retry.
    pub fn enable_csma(&mut self) {
        self.csma_enabled = true;
    }

    /// Disable CSMA channel access control.
    pub fn disable_csma(&mut self) {
        self.csma_enabled = false;
    }

    /// Returns `true` if the DCD detector currently sees a busy channel.
    pub fn is_channel_busy(&self) -> bool {
        self.dcd.is_busy()
    }

    /// Returns the most recent DCD RMS energy estimate.
    pub fn dcd_energy(&self) -> f32 {
        self.dcd.energy()
    }

    /// Check CSMA policy and return `ChannelBusy` if the channel is occupied
    /// or the p-persistence draw fails.  Called before encoding to avoid
    /// burning sequence numbers on a deferred transmission.
    fn csma_check(&self) -> Result<(), ModemError> {
        if !self.csma_enabled {
            return Ok(());
        }
        if self.dcd.is_busy() {
            return Err(ModemError::ChannelBusy);
        }
        // 0.3-persistence: transmit only with 30% probability on a clear channel
        let p: f32 = rand::thread_rng().gen();
        if p >= self.csma_persistence {
            return Err(ModemError::ChannelBusy);
        }
        Ok(())
    }

    /// Begin an adaptive-rate session using the given profile.
    ///
    /// Initialises a bidirectional rate adapter
    /// ([`openpulse_core::rate::BiDirRateAdapter`]) at `profile.initial_level`
    /// and stores the profile so that
    /// [`current_adaptive_mode`](Self::current_adaptive_mode)
    /// can resolve the current mode string on each transmit/receive cycle.
    pub fn start_adaptive_session(&mut self, profile: SessionProfile) {
        self.rate_policy.start_session(profile);
    }

    /// Cap the adaptive ladder at `max` (host/bandwidth limit, e.g. ARDOP `ARQBW`); `None` clears
    /// the cap. The active session is clamped immediately and future AckUp steps respect it.
    pub fn set_arq_max_tx_level(&mut self, max: Option<openpulse_core::rate::SpeedLevel>) {
        self.rate_policy.set_max_tx_level(max);
    }

    /// The active adaptive profile's defined `(level, mode)` pairs (ascending), for mapping a
    /// bandwidth cap in Hz to a max speed level. Empty when no adaptive session is active.
    pub fn adaptive_profile_modes(&self) -> Vec<(openpulse_core::rate::SpeedLevel, &'static str)> {
        self.rate_policy.defined_modes()
    }

    /// A2 (backlog-aware gating): minimum queued TX bytes required before an
    /// AckUp upgrade is acted on. `0` (default) disables the gate. Prevents
    /// spending upgrade airtime when only a frame or two remain queued.
    pub fn set_min_backlog_for_upgrade(&mut self, bytes: usize) {
        self.rate_policy.set_min_backlog_for_upgrade(bytes);
    }

    /// A2: update the current queued TX backlog (bytes) used by the gate.
    pub fn set_tx_backlog(&mut self, bytes: usize) {
        self.rate_policy.set_tx_backlog(bytes);
    }

    /// A3 (anti-oscillation): suppress this many upgrade attempts after a
    /// downgrade. `0` (default) disables the hold.
    pub fn set_upgrade_hold_frames(&mut self, frames: u32) {
        self.rate_policy.set_upgrade_hold_frames(frames);
    }

    /// Apply an [`OtaAggressiveness`] preset: sets the A2 backlog gate and the A3
    /// re-upgrade hold together so an operator picks one behaviour instead of
    /// tuning both knobs.
    pub fn set_ota_aggressiveness(&mut self, preset: OtaAggressiveness) {
        let (min_backlog, hold) = preset.knobs();
        self.rate_policy.set_min_backlog_for_upgrade(min_backlog);
        self.rate_policy.set_upgrade_hold_frames(hold);
    }

    /// Apply a received ACK type to the TX-direction rate adapter.
    ///
    /// Returns [`RateEvent::Maintained`] when no adaptive session is active.
    pub fn apply_ack(&mut self, ack: AckType) -> RateEvent {
        let (event, payload) = self.rate_policy.apply_ack(ack);
        if let Some(p) = payload {
            self.emit_rate_change(p);
        }
        event
    }

    /// Apply a received ACK frame, updating both TX and RX directions.
    ///
    /// When the frame carries a `reverse_ack`, the RX-direction adapter is also
    /// updated and a second `RateChange` event is emitted.
    pub fn apply_ack_frame(&mut self, frame: &openpulse_core::ack::AckFrame) -> RateEvent {
        let (tx_event, payloads) = self.rate_policy.apply_ack_frame(frame);
        for p in payloads {
            self.emit_rate_change(p);
        }
        tx_event
    }

    fn emit_rate_change(&self, payload: RateChangePayload) {
        let _ = self.event_tx.send(EngineEvent::RateChange {
            event: payload.event,
            speed_level: payload.speed_level,
            mode: payload.mode,
            direction: payload.direction,
            trigger: payload.trigger,
        });
    }

    /// Return the mode string for the current TX speed level of the active adaptive session.
    ///
    /// Returns `None` when no profile is active or the current speed level has no
    /// mode assigned (e.g. SL1 chirp fallback, reserved levels).
    pub fn current_adaptive_mode(&self) -> Option<&str> {
        self.rate_policy.current_adaptive_mode()
    }

    /// Return the mode string for the current RX speed level.
    pub fn current_rx_mode(&self) -> Option<&str> {
        self.rate_policy.current_rx_mode()
    }

    /// Return the current TX [`SpeedLevel`](openpulse_core::rate::SpeedLevel).
    pub fn current_tx_level(&self) -> Option<openpulse_core::rate::SpeedLevel> {
        self.rate_policy.current_tx_level()
    }

    /// Return the SNR estimate (dB) measured during the most recent
    /// [`receive`](Self::receive) or [`receive_with_ack_hint`](Self::receive_with_ack_hint) call.
    ///
    /// Derived from mean absolute LLR magnitude; useful for display and logging.
    /// Returns `None` if no receive call that supports soft demodulation has completed yet.
    pub fn last_rx_snr_db(&self) -> Option<f32> {
        self.rate_policy.last_rx_snr_db()
    }

    /// Apply a raw SNR estimate to the TX-direction rate adapter.
    ///
    /// If `snr_db` drops below the per-level SNR floor in the active session
    /// profile, the TX speed level is stepped down immediately — without waiting
    /// for a NACK — and a [`EngineEvent::RateChange`] is emitted with
    /// `trigger: Some(SnrFloor)`.  If `snr_db` rises above the ceiling, the
    /// upgrade-candidate flag is set; no level change occurs until the next
    /// ACK-UP is received.
    ///
    /// Does nothing when no adaptive session is active.
    pub fn apply_snr_hint(&mut self, snr_db: f32) {
        if let Some(payload) = self.rate_policy.apply_snr_hint(snr_db) {
            self.emit_rate_change(payload);
        }
    }

    // ── Receiver-led OTA adaptive rate-stepping ────────────────────────────────

    /// Start a receiver-led, per-direction OTA rate session for `profile`.
    ///
    /// Pairs [`respond_arq_ota`](Self::respond_arq_ota) (data receiver, leads its
    /// direction) with [`apply_ota_ack`](Self::apply_ota_ack) +
    /// [`ota_tx_mode`](Self::ota_tx_mode) (data sender, follows the peer).
    pub fn start_ota_session(&mut self, profile: SessionProfile) {
        self.ota = Some(OtaRateController::new(profile));
    }

    /// Stop the active OTA session (drops the controller). No-op if none active.
    pub fn stop_ota_session(&mut self) {
        self.ota = None;
    }

    /// Whether a receiver-led OTA session is active.
    pub fn ota_active(&self) -> bool {
        self.ota.is_some()
    }

    /// Mode string the local station should transmit data at under the OTA session.
    pub fn ota_tx_mode(&self) -> Option<&str> {
        self.ota.as_ref().and_then(|o| o.tx_mode())
    }

    /// FEC scheme to transmit data with at the current OTA TX level (MODCOD).
    /// Returns [`FecMode::None`] when no OTA session is active.
    pub fn ota_tx_fec(&self) -> FecMode {
        self.ota
            .as_ref()
            .map(|o| o.tx_fec())
            .unwrap_or(FecMode::None)
    }

    /// Current OTA TX speed level (the level the peer last recommended to us).
    pub fn ota_tx_level(&self) -> Option<SpeedLevel> {
        self.ota.as_ref().map(|o| o.tx_level())
    }

    /// Absolute level we are currently recommending to the peer (goes in our ACK).
    pub fn ota_rx_recommended_level(&self) -> Option<SpeedLevel> {
        self.ota.as_ref().map(|o| o.rx_recommended_level())
    }

    /// Highest level we have actually decoded (the lockstep anchor).
    pub fn ota_rx_confirmed_level(&self) -> Option<SpeedLevel> {
        self.ota.as_ref().map(|o| o.rx_confirmed_level())
    }

    /// Supply an external RX SNR estimate (dB) for OTA adaptive decisions, or
    /// `None` to fall back to the built-in silence-gated M2M4 moment estimator on
    /// the captured envelope.
    ///
    /// A channel-sim harness that knows the true SNR can feed it here to bypass
    /// the on-air estimate; otherwise the M2M4 estimate drives the rate ladder.
    pub fn set_rx_snr_estimate(&mut self, snr_db: Option<f32>) {
        self.rx_snr_estimate = snr_db;
    }

    /// Clamp the OTA rate ladder to `[min, max]` (each `None` = the profile bound).
    ///
    /// Use to cap the top rung (regulatory bandwidth / robustness) or floor the bottom.
    /// No-op without an active OTA session.
    pub fn ota_set_level_bounds(&mut self, min: Option<SpeedLevel>, max: Option<SpeedLevel>) {
        if let Some(o) = self.ota.as_mut() {
            o.set_level_bounds(min, max);
        }
    }

    /// Pin the OTA session to a fixed level (manual override; stops adapting).
    /// No-op without an active OTA session.
    pub fn ota_lock_level(&mut self, level: SpeedLevel) {
        if let Some(o) = self.ota.as_mut() {
            o.lock_level(level);
        }
    }

    /// Release an OTA level lock and resume adapting. No-op without a session.
    pub fn ota_unlock(&mut self) {
        if let Some(o) = self.ota.as_mut() {
            o.unlock();
        }
    }

    /// Whether the OTA session is locked to a fixed level.
    pub fn ota_is_locked(&self) -> bool {
        self.ota.as_ref().is_some_and(|o| o.is_locked())
    }

    /// Sender side: adopt the peer's absolute `recommended_level` from a received ACK.
    ///
    /// A no-op when the frame carries no recommendation or no OTA session is active.
    /// The absolute target means a lost ACK never desyncs — the next ACK re-states it.
    pub fn apply_ota_ack(&mut self, frame: &AckFrame) {
        if let (Some(o), Some(level)) = (self.ota.as_mut(), frame.recommended_level) {
            o.adopt_recommendation(level);
        }
    }

    /// Sender side (ISS): one-call OTA data frame — transmit at the current OTA
    /// mode+FEC, wait for the FSK4-ACK, adopt the peer's `recommended_level`, and
    /// retry on Nack / missing ACK.
    ///
    /// The half-duplex counterpart to [`respond_arq_ota`](Self::respond_arq_ota):
    /// it transmits then listens on the same device, so it suits a real radio (or a
    /// loopback that feeds TX back to RX) where the peer answers in-band. Returns the
    /// adopted TX [`SpeedLevel`] on success, or [`ModemError::ArqMaxRetries`] after
    /// `1 + max_retries` attempts. Always adopts a `recommended_level` carried by any
    /// ACK (even a Nack) so the absolute target can never drift.
    pub fn transmit_arq_ota(
        &mut self,
        data: &[u8],
        device: Option<&str>,
        max_retries: usize,
    ) -> Result<SpeedLevel, ModemError> {
        // Single-shot ACK receive (timeout 0): the ACK is already in the buffer for
        // synchronous in-process callers (tests). The daemon uses the timeout form.
        self.transmit_arq_ota_within(data, device, max_retries, 0)
    }

    /// As [`transmit_arq_ota`](Self::transmit_arq_ota), but each attempt waits up to
    /// `ack_timeout_ms` for the FSK4-ACK to arrive (re-capturing on the device until
    /// it decodes or the deadline passes). `0` = single-shot (the original behaviour).
    ///
    /// Needed by a free-running daemon: after the data frame is transmitted, the
    /// peer's ACK only returns after its own receive tick + the channel round-trip,
    /// so a single immediate read misses it. With a timeout the sender owns the RX
    /// for the turnaround and adopts the peer's absolute `recommended_level` — which
    /// is what steps the rate ladder.
    pub fn transmit_arq_ota_within(
        &mut self,
        data: &[u8],
        device: Option<&str>,
        max_retries: usize,
        ack_timeout_ms: u64,
    ) -> Result<SpeedLevel, ModemError> {
        let attempts = 1 + max_retries;
        let mut last_err: Option<ModemError> = None;
        for _ in 0..attempts {
            let mode = self
                .ota_tx_mode()
                .ok_or_else(|| ModemError::Configuration("no OTA session active".into()))?
                .to_owned();
            let fec = self.ota_tx_fec();
            self.transmit_with_fec_mode(data, &mode, fec, device)?;
            match self.receive_ack_with_short_fec_within(device, ack_timeout_ms) {
                Ok(ack) => {
                    self.apply_ota_ack(&ack);
                    if ack.ack_type != AckType::Nack {
                        return self.ota_tx_level().ok_or_else(|| {
                            ModemError::Configuration("no OTA session active".into())
                        });
                    }
                }
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap_or(ModemError::ArqMaxRetries(attempts)))
    }

    /// Receiver side: capture one data frame, demodulate with the OTA candidate
    /// fallback, reply with an ACK carrying the absolute `recommended_level`, and
    /// return the payload.
    ///
    /// Tries the candidate modes (`{recommended, confirmed}`, recommended first) on
    /// the *same* captured buffer, so a sender that has not yet adopted our last
    /// recommendation (lost ACK) is still decoded at the confirmed level. On total
    /// decode failure it replies `Nack` (still carrying the current recommendation)
    /// and returns the decode error.
    pub fn respond_arq_ota(
        &mut self,
        session_id: &str,
        device: Option<&str>,
    ) -> Result<Vec<u8>, ModemError> {
        let samples = self.stage_capture_input(None, device)?;
        let samples = self.route_audio_stage(PipelineStage::InputCapture, samples)?;
        self.ota_update_dcd(&samples);

        let (decoded, ack_frame, last_err) = self.ota_decode_and_ack(&samples, session_id)?;
        self.transmit_ack_with_short_fec(&ack_frame, device)?;

        match decoded {
            Some((payload, mode)) => {
                let _ = self.event_tx.send(EngineEvent::FrameReceived {
                    mode,
                    bytes: payload.len(),
                });
                Ok(payload)
            }
            None => Err(last_err.unwrap_or_else(|| {
                ModemError::Configuration("OTA receive: no candidate decoded".into())
            })),
        }
    }

    /// Daemon-facing OTA receive poll: capture one window and, **only if the
    /// channel carries energy**, run the receiver-led decode and return the
    /// decoded payload plus the ACK frame to transmit. Returns `Ok(None)` on an
    /// idle window so the caller never keys PTT to ACK silence.
    ///
    /// Unlike [`respond_arq_ota`](Self::respond_arq_ota) this does **not** transmit
    /// the ACK: a half-duplex caller keys PTT around
    /// `transmit_ack_with_short_fec(&result.ack)` so the radio receives with PTT
    /// released and only keys to answer. The idle gate uses the immediate-window
    /// RMS (not the held DCD busy flag) so the trailing DCD hold after a burst does
    /// not trigger a spurious ACK on silence.
    pub fn poll_ota_rx(
        &mut self,
        session_id: &str,
        device: Option<&str>,
    ) -> Result<Option<OtaRxResult>, ModemError> {
        let samples = self.stage_capture_input(None, device)?;
        let samples = self.route_audio_stage(PipelineStage::InputCapture, samples)?;
        self.ota_update_dcd(&samples);

        if samples.samples.is_empty() || self.dcd.energy() < self.dcd.threshold() {
            return Ok(None);
        }

        let (decoded, ack, last_err) = self.ota_decode_and_ack(&samples, session_id)?;
        let (payload, mode) = match decoded {
            Some((p, m)) => (Some(p), Some(m)),
            None => {
                if let Some(e) = &last_err {
                    debug!("poll_ota_rx: energetic window failed to decode: {e}");
                }
                (None, None)
            }
        };
        if let Some(p) = &payload {
            let _ = self.event_tx.send(EngineEvent::FrameReceived {
                mode: mode.clone().unwrap_or_default(),
                bytes: p.len(),
            });
        }
        Ok(Some(OtaRxResult { payload, ack, mode }))
    }

    /// Update DCD from a captured window, emitting a `DcdChange` event on a flip.
    fn ota_update_dcd(&mut self, samples: &AudioSamples) {
        let prev_busy = self.dcd.is_busy();
        self.dcd.update(&samples.samples);
        if self.dcd.is_busy() != prev_busy {
            let _ = self.event_tx.send(EngineEvent::DcdChange {
                busy: self.dcd.is_busy(),
                energy: self.dcd.energy(),
            });
        }
    }

    /// Tick-based burst capture for a free-running daemon over a streaming audio
    /// backend: accumulate captured samples while a carrier is present and return
    /// the **whole burst** once the carrier drops (or a safety cap is hit), else
    /// `None` while still accumulating or idle.
    ///
    /// On a real (cpal) backend a single frame spans many short receive-tick
    /// windows; decoding one partial window can't acquire the frame. Buffering the
    /// burst and decoding it as one unit — via [`decode_burst`](Self::decode_burst)
    /// or [`ota_decode_burst`](Self::ota_decode_burst) — fixes that. The
    /// in-process loopback delivers a frame atomically, so it flushes on the next
    /// (quiet) tick. Carrier presence is the per-window RMS vs the DCD squelch.
    pub fn capture_burst(
        &mut self,
        device: Option<&str>,
    ) -> Result<Option<AudioSamples>, ModemError> {
        let samples = self.stage_capture_input(None, device)?;
        self.accumulate_routed(samples)
    }

    /// Open a capture stream on `device` (or the engine's default device) for a
    /// caller that will own it across receive ticks and feed each `read()` to
    /// [`accumulate_capture`](Self::accumulate_capture). Returning the stream keeps
    /// it on the caller's thread — required for a streaming (cpal) backend, whose
    /// callback only fills its buffer while the stream is held open. A
    /// [`LoopbackBackend`](openpulse_audio::LoopbackBackend) stream clones the same
    /// shared buffers, so this is equivalent to per-tick reopen there.
    pub fn open_capture_stream(
        &self,
        device: Option<&str>,
    ) -> Result<Box<dyn AudioInputStream>, ModemError> {
        let audio_cfg = AudioConfig::default();
        self.audio
            .open_input(device.or(self.default_device.as_deref()), &audio_cfg)
            .map_err(|e| ModemError::Audio(e.to_string()))
    }

    /// Burst-accumulate samples the CALLER already captured from a persistent input
    /// stream, returning a complete burst when the carrier drops (same contract as
    /// [`capture_burst`](Self::capture_burst)).
    ///
    /// cpal is a callback backend whose stream needs tens of ms to start delivering
    /// after `play()`; reopening it every tick (as `capture_burst` does) never warms
    /// up on real hardware, so the buffer stays empty and no carrier is ever seen. A
    /// tick-based caller on a real audio backend should instead open one input
    /// stream, keep it open, and feed each `read()` here — the daemon receive loop
    /// does this. Records the spectrum/waterfall tap from these samples.
    pub fn accumulate_capture(
        &mut self,
        mode: Option<&str>,
        samples: Vec<f32>,
    ) -> Result<Option<AudioSamples>, ModemError> {
        self.record_audio(&samples); // RX window (raw channel audio) for the spectrum/waterfall tap
                                     // The notch is applied once, at the single `PipelineStage::InputCapture` seam in
                                     // `route_audio_stage` (reached via `accumulate_routed` below); just record the mode here.
        self.rx_mode = mode.map(|m| m.to_string());
        self.accumulate_routed(AudioSamples { samples })
    }

    /// Shared burst gather/flush over already-captured samples: route the input
    /// pipeline + DCD, accumulate while the carrier is present (per-window RMS vs the
    /// DCD squelch), and flush the whole burst when it drops or the cap is hit.
    fn accumulate_routed(
        &mut self,
        samples: AudioSamples,
    ) -> Result<Option<AudioSamples>, ModemError> {
        let samples = self.route_audio_stage(PipelineStage::InputCapture, samples)?;
        self.ota_update_dcd(&samples);

        let n = samples.samples.len();
        let rms = if n == 0 {
            0.0
        } else {
            (samples.samples.iter().map(|s| s * s).sum::<f32>() / n as f32).sqrt()
        };

        if n > 0 && rms >= self.dcd.threshold() {
            // Carrier present: keep accumulating this burst.
            self.rx_burst.extend_from_slice(&samples.samples);
            self.rx_capturing = true;
            if self.rx_burst.len() >= BURST_MAX_SAMPLES {
                self.rx_capturing = false;
                return Ok(Some(AudioSamples {
                    samples: std::mem::take(&mut self.rx_burst),
                }));
            }
            Ok(None)
        } else if self.rx_capturing && !self.rx_burst.is_empty() {
            // Carrier dropped after a burst → the frame is complete; flush it.
            self.rx_capturing = false;
            Ok(Some(AudioSamples {
                samples: std::mem::take(&mut self.rx_burst),
            }))
        } else {
            Ok(None)
        }
    }

    /// Onset-scan geometry for `mode`: (scan step, acquisition window, min frame
    /// samples, max frame samples). Prefers the plugin's `frame_geometry`; falls back
    /// to trailing-digit baud with a 32-symbol preamble for unregistered plugins.
    fn frame_scan_geometry(&self, mode: &str, sample_rate: u32) -> (usize, usize, usize, usize) {
        let geometry = self.plugins.get(mode).and_then(|p| {
            p.frame_geometry(&ModulationConfig {
                mode: mode.to_string(),
                sample_rate,
                ..ModulationConfig::default()
            })
        });
        match geometry {
            Some(g) => (
                g.symbol_period_samples.max(1),
                g.preamble_samples.max(g.symbol_period_samples).max(1),
                g.min_frame_samples.max(1),
                g.max_frame_samples.max(g.min_frame_samples),
            ),
            None => {
                let step = {
                    let baud: u32 = mode
                        .trim_end_matches("-RRC")
                        .bytes()
                        .rev()
                        .take_while(|b| b.is_ascii_digit())
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .fold(0u32, |acc, b| acc * 10 + (b - b'0') as u32);
                    if baud > 0 {
                        (sample_rate as f32 / baud as f32).round() as usize
                    } else {
                        32
                    }
                };
                (step, step * 32, step * 33, step * 2280)
            }
        }
    }

    /// Decode a captured burst (from [`capture_burst`](Self::capture_burst) or
    /// [`accumulate_capture`](Self::accumulate_capture)) as a data frame in `mode`.
    ///
    /// A DCD-detected burst is not sample-accurate, and the engine's single-window
    /// demod settles AFC on the window start (DSP playbook §3), so decoding only from
    /// sample 0 usually misframes. Scan onset offsets across the captured lead-in —
    /// reusing the same `decode_attempt` + frame geometry as the live timeout
    /// receiver — and return the first frame that validates.
    pub fn decode_burst(
        &mut self,
        mode: &str,
        burst: &AudioSamples,
    ) -> Result<Vec<u8>, ModemError> {
        let sr = AudioConfig::default().sample_rate;
        let (step, acq_samples, min_frame_samples, max_frame_samples) =
            self.frame_scan_geometry(mode, sr);
        let n = burst.samples.len();
        if n < min_frame_samples {
            // Too short to hold a frame: one direct attempt for the error/SNR path.
            return self.receive_from_samples(
                mode,
                AudioSamples {
                    samples: burst.samples.clone(),
                },
            );
        }
        let step = step.max(1);
        // The carrier onset sits within the captured lead-in; scan up to a few
        // acquisition windows past sample 0 (bounded so a noise burst can't spin).
        let scan_end = (n - min_frame_samples).min(acq_samples.saturating_mul(4));
        let mut start = 0usize;
        let last_err = loop {
            let end = (start + max_frame_samples).min(n);
            let afc_before = self.afc_correction_hz;
            match self.decode_attempt(
                mode,
                AudioSamples {
                    samples: burst.samples[start..end].to_vec(),
                },
                FecMode::None,
            ) {
                Ok(payload) => return Ok(payload),
                Err(e) => {
                    self.afc_correction_hz = afc_before; // undo the failed attempt's AFC drift
                    if start >= scan_end {
                        break e;
                    }
                    start = (start + step).min(scan_end);
                }
            }
        };
        Err(last_err)
    }

    /// Decode a captured burst with the OTA candidate fallback and build the ACK to
    /// send back (does not transmit it) — the burst-input counterpart of
    /// [`poll_ota_rx`](Self::poll_ota_rx) for a daemon using
    /// [`capture_burst`](Self::capture_burst).
    pub fn ota_decode_burst(
        &mut self,
        burst: &AudioSamples,
        session_id: &str,
    ) -> Result<OtaRxResult, ModemError> {
        let (decoded, ack, last_err) = self.ota_decode_and_ack(burst, session_id)?;
        let (payload, mode) = match decoded {
            Some((p, m)) => (Some(p), Some(m)),
            None => {
                if let Some(e) = &last_err {
                    debug!("ota_decode_burst: burst failed to decode: {e}");
                }
                (None, None)
            }
        };
        // `FrameReceived` is already emitted by the inner `decode_attempt` → `receive_from_samples`
        // on a successful decode; emitting again here double-counted it on the OTA path only.
        Ok(OtaRxResult { payload, ack, mode })
    }

    /// Shared OTA receive core: run the candidate-fallback decode on an already
    /// captured window, update the receiver-led controller, and build the ACK frame
    /// to send back. Captures and transmits nothing — callers own those, so the
    /// daemon can key PTT only around the ACK transmit. Returns the decoded
    /// payload+mode (if any), the ACK frame, and the last decode error.
    fn ota_decode_and_ack(
        &mut self,
        samples: &AudioSamples,
        session_id: &str,
    ) -> Result<OtaDecodeOutcome, ModemError> {
        let candidates: Vec<(SpeedLevel, String, FecMode)> = self
            .ota
            .as_ref()
            .ok_or_else(|| ModemError::Configuration("no OTA session active".into()))?
            .rx_candidates()
            .into_iter()
            .map(|(l, m, f)| (l, m.to_string(), f))
            .collect();

        // AFC accumulates across calls, so a failed wrong-mode candidate would
        // poison the correct candidate's correction. Isolate each attempt: reset to
        // the pre-frame AFC before every try, keeping only the successful update.
        // Each candidate carries its own MODCOD FEC, applied via decode_attempt.
        let afc_before = self.afc_correction_hz;
        let mut decoded: Option<(Vec<u8>, SpeedLevel, String)> = None;
        let mut last_err: Option<ModemError> = None;
        for (level, mode, fec) in &candidates {
            self.afc_correction_hz = afc_before;
            let slice = AudioSamples {
                samples: samples.samples.clone(),
            };
            match self.decode_attempt(mode, slice, *fec) {
                Ok(payload) => {
                    decoded = Some((payload, *level, mode.clone()));
                    break;
                }
                Err(e) => last_err = Some(e),
            }
        }

        // SNR for the receiver decision: prefer an external estimate; else the M2M4
        // moment estimator on the captured envelope (a real absolute SNR, unlike the
        // mean-|LLR| proxy), silence-gated to the active burst. Works whether or not
        // a candidate decoded.
        let snr = self.rx_snr_estimate.unwrap_or_else(|| {
            let fc = self.center_frequency + self.afc_correction_hz;
            let fs = AudioConfig::default().sample_rate as f32;
            openpulse_core::snr_estimate::m2m4_snr_db_gated_from_real(&samples.samples, fc, fs)
        });

        let ota = self
            .ota
            .as_mut()
            .ok_or_else(|| ModemError::Configuration("no OTA session active".into()))?;
        let (rx_ack, decoded) = match decoded {
            Some((payload, level, mode)) => {
                let ack = ota.on_rx_frame(RxOutcome::Decoded(level), snr);
                (ack, Some((payload, mode)))
            }
            None => {
                let ack = ota.on_rx_frame(RxOutcome::Failed, snr);
                (ack, None)
            }
        };
        let ack_frame = AckFrame::new(rx_ack.ack_type, session_id)
            .with_recommended_level(rx_ack.recommended_level);
        Ok((decoded, ack_frame, last_err))
    }

    /// Select HARQ retry parameters from SNR/fading state.
    ///
    /// This deterministic mapping is the Item 6 policy hook for choosing
    /// retry FEC mode and ACK timeout without mutating engine state.
    pub fn select_harq_decision(
        &self,
        snr_db: f32,
        fading_depth_db: f32,
        retry_index: u8,
    ) -> HarqDecision {
        HarqPolicy::default().select(snr_db, fading_depth_db, retry_index)
    }

    /// HARQ decision specialised to `mode`'s demodulator capability.
    ///
    /// Identical to [`select_harq_decision`](Self::select_harq_decision) except
    /// the high-rate-LDPC tier may engage when the mode's plugin produces genuine
    /// soft LLRs (the dense rungs).  Unknown modes fall back to hard-only.
    pub fn select_harq_decision_for_mode(
        &self,
        mode: &str,
        snr_db: f32,
        fading_depth_db: f32,
        retry_index: u8,
    ) -> HarqDecision {
        let soft_capable = self
            .plugins
            .get(mode)
            .map(|p| p.supports_soft_demod())
            .unwrap_or(false);
        HarqPolicy::default()
            .with_soft_capable(soft_capable)
            .select(snr_db, fading_depth_db, retry_index)
    }

    /// Returns the current HPX state for this engine session.
    pub fn hpx_state(&self) -> HpxState {
        self.hpx.state()
    }

    /// Returns the active HPX session id when a secure session is in progress.
    pub fn hpx_session_id(&self) -> Option<&str> {
        self.hpx.session_id()
    }

    /// Returns emitted HPX transitions for the current session lifecycle.
    pub fn hpx_transitions(&self) -> &[HpxTransition] {
        self.hpx.transitions()
    }

    /// Returns a snapshot of per-stage pipeline queue metrics.
    pub fn pipeline_metrics_snapshot(&self) -> PipelineMetricsSnapshot {
        self.scheduler.metrics_snapshot()
    }

    /// Returns the active handshake decision for the secure session, if present.
    pub fn active_handshake(&self) -> Option<&HandshakeDecision> {
        self.active_handshake.as_ref()
    }

    /// Starts a secure HPX session and enforces handshake policy before entering transfer.
    pub fn begin_secure_session(
        &mut self,
        params: SecureSessionParams,
        timestamp_ms: u64,
    ) -> Result<HandshakeDecision, ModemError> {
        self.hpx_apply_event(HpxEvent::StartSession, timestamp_ms)?;
        self.hpx_apply_event(HpxEvent::DiscoveryOk, timestamp_ms.saturating_add(1))?;

        let handshake = evaluate_handshake(
            self.trust_policy_profile,
            params.local_minimum_mode,
            &params.peer_supported_modes,
            params.key_trust,
            params.certificate_source,
            params.psk_validated,
        )
        .map_err(|e| {
            let _ = self.hpx_apply_event(
                HpxEvent::SignatureVerificationFailed,
                timestamp_ms.saturating_add(2),
            );
            ModemError::Configuration(format!("secure handshake rejected: {e:?}"))
        })?;

        let required = minimum_trust_for_profile(self.trust_policy_profile);
        if handshake.trust.decision < required {
            let _ = self.hpx_apply_event(
                HpxEvent::SignatureVerificationFailed,
                timestamp_ms.saturating_add(2),
            );
            return Err(ModemError::Configuration(format!(
                "secure handshake trust '{}' is below required '{}' for profile '{}', reason_code={}",
                format!("{:?}", handshake.trust.decision).to_lowercase(),
                format!("{:?}", required).to_lowercase(),
                format!("{:?}", self.trust_policy_profile).to_lowercase(),
                handshake.trust.reason_code
            )));
        }

        self.hpx_apply_event(HpxEvent::TrainingOk, timestamp_ms.saturating_add(3))?;
        self.active_handshake = Some(handshake.clone());
        let _ = self.event_tx.send(EngineEvent::SessionStarted {
            session_id: self.hpx_session_id().map(str::to_string),
            peer_modes: params
                .peer_supported_modes
                .iter()
                .map(|m| format!("{m:?}"))
                .collect::<Vec<_>>()
                .join(","),
        });
        Ok(handshake)
    }

    /// Gracefully closes an active secure HPX session.
    pub fn end_secure_session(&mut self, timestamp_ms: u64) -> Result<(), ModemError> {
        if self.hpx_state() == HpxState::Idle {
            self.active_handshake = None;
            return Ok(());
        }

        let session_id = self.hpx_session_id().map(str::to_string);
        self.hpx_apply_event(HpxEvent::LocalCancel, timestamp_ms)?;
        self.hpx_apply_event(HpxEvent::TransferComplete, timestamp_ms.saturating_add(1))?;
        self.active_handshake = None;
        let _ = self.event_tx.send(EngineEvent::SessionEnded {
            session_id,
            reason: "local cancel".to_string(),
        });
        Ok(())
    }

    /// Apply an HPX state-machine event and return the emitted transition event.
    pub fn hpx_apply_event(
        &mut self,
        event: HpxEvent,
        timestamp_ms: u64,
    ) -> Result<HpxTransition, ModemError> {
        let transition = self
            .hpx
            .apply_event(event, timestamp_ms)
            .map_err(|e| ModemError::Configuration(e.to_string()))?;
        let _ = self.event_tx.send(EngineEvent::HpxTransition {
            from: transition.from_state,
            to: transition.to_state,
            event: transition.event,
            session_id: transition.session_id.clone(),
        });
        Ok(transition)
    }

    /// Encode an application payload into a signed envelope wire blob.
    pub fn encode_signed_envelope(
        &self,
        payload: &[u8],
        signing_mode: SigningMode,
        signer_id: &str,
        key_id: &str,
        signature: &[u8],
    ) -> Result<Vec<u8>, ModemError> {
        let session_id = self.hpx_session_id().unwrap_or("unsessioned");
        SignedEnvelope::new(
            session_id,
            self.sequence as u64,
            signing_mode,
            payload.to_vec(),
            signer_id,
            key_id,
            signature.to_vec(),
        )
        .encode()
    }

    /// Decode and verify a signed envelope wire blob.
    pub fn decode_signed_envelope(
        &self,
        envelope_bytes: &[u8],
    ) -> Result<SignedEnvelope, ModemError> {
        SignedEnvelope::decode(envelope_bytes)
    }

    /// Register a modulation plugin.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the plugin's trait version is incompatible with the framework.
    pub fn register_plugin(
        &mut self,
        plugin: Box<dyn openpulse_core::plugin::ModulationPlugin>,
    ) -> Result<(), PluginError> {
        info!("registering plugin: {}", plugin.info().name);
        self.plugins.register(plugin)?;
        info!("plugin registered successfully");
        Ok(())
    }

    /// Return the underlying plugin registry (read-only).
    pub fn plugins(&self) -> &PluginRegistry {
        &self.plugins
    }

    /// Encode `data` into a [`Frame`], modulate it with the plugin that
    /// handles `mode`, and write the resulting audio to the output device.
    ///
    /// Pass `device = None` to use the backend's default output device.
    pub fn transmit(
        &mut self,
        data: &[u8],
        mode: &str,
        device: Option<&str>,
    ) -> Result<(), ModemError> {
        if self.hpx_state() != HpxState::Idle {
            if self.hpx_state() != HpxState::ActiveTransfer {
                return Err(ModemError::Configuration(
                    "cannot transmit: secure session is not in active transfer".to_string(),
                ));
            }
            if self.active_handshake.is_none() {
                return Err(ModemError::Configuration(
                    "cannot transmit: secure handshake not established".to_string(),
                ));
            }
        }

        // CSMA check before encoding so a deferral does not burn a sequence number.
        self.csma_check()?;

        let outbound = self.stage_encode_frame(data)?;
        let outbound = self.route_wire_stage(PipelineStage::EncodeModulate, outbound)?;

        debug!(
            "transmitting {} byte frame (seq={}, mode={mode})",
            outbound.bytes.len(),
            self.sequence.wrapping_sub(1)
        );

        let samples = {
            let plugin = self
                .plugins
                .get(mode)
                .ok_or_else(|| ModemError::PluginNotFound(mode.to_string()))?;
            self.stage_modulate_payload(plugin, mode, &outbound)?
        };
        let samples = self.route_audio_stage(PipelineStage::OutputEmit, samples)?;
        info!(
            "modulated {} bytes → {} audio samples",
            outbound.bytes.len(),
            samples.samples.len()
        );

        self.stage_emit_output(device, mode, &samples)?;
        // (Regulatory TX logging now happens for every frame inside `stage_emit_output`.)

        let _ = self.event_tx.send(EngineEvent::FrameTransmitted {
            mode: mode.to_string(),
            bytes: outbound.bytes.len(),
        });
        Ok(())
    }

    /// Encode `data`, modulate to baseband I/Q, and write to the IQ output stream.
    ///
    /// Requires the audio backend to support [`AudioBackend::open_iq_output`].
    /// Returns `ModemError::Configuration` when the backend has no IQ output.
    pub fn transmit_iq(
        &mut self,
        data: &[u8],
        mode: &str,
        device: Option<&str>,
    ) -> Result<(), ModemError> {
        self.csma_check()?;

        let outbound = self.stage_encode_frame(data)?;
        let outbound = self.route_wire_stage(PipelineStage::EncodeModulate, outbound)?;

        let (i_bb, q_bb) = {
            let plugin = self
                .plugins
                .get(mode)
                .ok_or_else(|| ModemError::PluginNotFound(mode.to_string()))?;
            let mod_cfg = ModulationConfig {
                mode: mode.to_string(),
                center_frequency: self.center_frequency,
                ..ModulationConfig::default()
            };
            plugin.modulate_iq(&outbound.bytes, &mod_cfg)?
        };

        let audio_cfg = AudioConfig::default();
        let mut stream = self
            .audio
            .open_iq_output(device.or(self.default_device.as_deref()), &audio_cfg)
            .ok_or_else(|| {
                ModemError::Configuration("audio backend does not support IQ output".to_string())
            })?
            .map_err(|e| ModemError::Audio(e.to_string()))?;

        stream
            .write_iq(&i_bb, &q_bb)
            .map_err(|e| ModemError::Audio(e.to_string()))?;
        stream
            .flush()
            .map_err(|e| ModemError::Audio(e.to_string()))?;

        let _ = self.event_tx.send(EngineEvent::FrameTransmitted {
            mode: mode.to_string(),
            bytes: outbound.bytes.len(),
        });
        Ok(())
    }

    /// Read audio from the input device, demodulate with the plugin for
    /// `mode`, and return the decoded frame payload.
    ///
    /// Pass `device = None` to use the backend's default input device.
    pub fn receive(&mut self, mode: &str, device: Option<&str>) -> Result<Vec<u8>, ModemError> {
        let samples = self.stage_capture_input(Some(mode), device)?;
        self.receive_from_samples(mode, samples)
    }

    /// Receive a frame by listening on the input stream until a decode succeeds
    /// or the timeout elapses (no FEC).
    pub fn receive_with_timeout(
        &mut self,
        mode: &str,
        device: Option<&str>,
        listen_for: Duration,
    ) -> Result<Vec<u8>, ModemError> {
        self.receive_with_timeout_fec(mode, device, listen_for, FecMode::None)
    }

    /// As [`receive_with_timeout`](Self::receive_with_timeout) but applies the FEC
    /// codec `fec` to each decode attempt — the timeout-scanning counterpart of
    /// [`receive_with_fec_mode`](Self::receive_with_fec_mode), needed for live
    /// (loopback / on-air) reception of FEC-protected frames.
    pub fn receive_with_fec_mode_timeout(
        &mut self,
        mode: &str,
        fec: FecMode,
        device: Option<&str>,
        listen_for: Duration,
    ) -> Result<Vec<u8>, ModemError> {
        self.receive_with_timeout_fec(mode, device, listen_for, fec)
    }

    fn receive_with_timeout_fec(
        &mut self,
        mode: &str,
        device: Option<&str>,
        listen_for: Duration,
        fec: FecMode,
    ) -> Result<Vec<u8>, ModemError> {
        let audio_cfg = AudioConfig::default();
        let mut stream = self
            .audio
            .open_input(device.or(self.default_device.as_deref()), &audio_cfg)
            .map_err(|e| ModemError::Audio(e.to_string()))?;

        let deadline = Instant::now() + listen_for;
        let start_time = Instant::now();
        let mut accumulated = Vec::new();
        let mut last_err: Option<ModemError> = None;

        // Frame geometry: scan step, acquisition window, and per-attempt slice
        // bounds.  Preferred source is the plugin itself via frame_geometry().
        // The legacy fallback (trailing mode-name digits as baud, 32-symbol
        // preamble) is only correct for modes named after their baud rate —
        // it parsed OFDM52's subcarrier count as baud and SCFDMA52-64QAM-P4
        // as 4 baud — and remains only for unregistered/external plugins.
        let geometry = self.plugins.get(mode).and_then(|p| {
            p.frame_geometry(&ModulationConfig {
                mode: mode.to_string(),
                sample_rate: audio_cfg.sample_rate,
                ..ModulationConfig::default()
            })
        });
        let (step, acq_samples, min_frame_samples, max_frame_samples) = match geometry {
            Some(g) => (
                g.symbol_period_samples.max(1),
                g.preamble_samples.max(g.symbol_period_samples).max(1),
                g.min_frame_samples.max(1),
                g.max_frame_samples.max(g.min_frame_samples),
            ),
            None => {
                let step = {
                    let baud: u32 = mode
                        .trim_end_matches("-RRC")
                        .bytes()
                        .rev()
                        .take_while(|b| b.is_ascii_digit())
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .fold(0u32, |acc, b| acc * 10 + (b - b'0') as u32);
                    if baud > 0 {
                        (audio_cfg.sample_rate as f32 / baud as f32).round() as usize
                    } else {
                        32
                    }
                };
                // 33 = PREAMBLE_SYMS(32) + 1 data symbol; 2280 = preamble +
                // full 255-byte RS frame at 1 bit/symbol + 10 % margin.
                (step, step * 32, step * 33, step * 2280)
            }
        };

        // A "long-frame" mode takes many seconds of real-time audio to buffer (the
        // slow BPSK rungs: BPSK31 ≈ 12 s).  For these the settled full-buffer retry
        // is skipped (its O(buffer) re-scan every 2 s outruns the read cadence and
        // starves the loop so the frame never finishes buffering — the first-energy
        // micro-sweep owns the decode instead).  Every other mode keeps the retry:
        // the wideband multicarrier modes (SCFDMA/OFDM) in particular have short
        // frames AND a marginal settle that the single-carrier first-energy path
        // can't decode, so they depend on the retry's per-position re-acquisition.
        // The raw (pre-FEC) geometry separates them cleanly: BPSK31/63/100 are
        // >180k samples, every other mode <=75k.
        let long_frame = max_frame_samples > 120_000;

        // FEC frames are larger than the un-coded frame the geometry describes
        // (conv rate-1/2 ≈ 2×, RS ≈ 1.15×); widen the per-attempt slice so the whole
        // coded frame is decoded rather than truncated at max_frame_samples.
        let max_frame_samples = if matches!(fec, FecMode::None) {
            max_frame_samples
        } else {
            max_frame_samples.saturating_mul(3)
        };

        // AFC settling window.  It must span at least one data symbol past the
        // preamble so the plugin's fine (IQ-squaring) estimator engages; on a
        // pure preamble-length window (`acq_samples`) the estimator falls back to
        // the coarse ±12.5 Hz Goertzel grid, whose ≤6.25 Hz residual is inside the
        // faster BPSK modes' tolerance but exceeds BPSK31's ±7.8 Hz (= baud/4 for
        // differential detection) — which is why 31.25-baud frames were
        // undecodable while BPSK63/100/250 passed.  `min_frame_samples`
        // (= preamble + 1 symbol) is exactly that fine-AFC threshold and is only
        // one symbol longer than `acq_samples`, so settling cost is unchanged.
        let afc_window = acq_samples.max(min_frame_samples);

        // Adaptive silence gate (absolute floor 1e-4 mean-square, raised above
        // an elevated band noise floor; see EnergyGate).  Silence is typically
        // < 2.5e-5 mean-square; a live BPSK carrier at 30 % full-scale gives
        // ≈ 0.045.
        let mut energy_gate = EnergyGate::new();
        // Maximum AFC correction magnitude accepted after settling.
        // The Goertzel acquisition range is ±400 Hz (range_hz = 800 in the
        // estimate_carrier_hz_wide implementation).  On-air measurements between
        // IC-9700 and FT-991A show a consistent ~400 Hz carrier offset between
        // the two rigs at the same nominal dial frequency — reject anything
        // beyond the full ±400 Hz Goertzel range plus a small margin.
        // The convergence guard (|change| > 5 Hz) still rejects flat noise
        // that produces a near-zero stable estimate.
        const AFC_MAX_CORRECTION_HZ: f32 = 450.0;

        // Scan/retry policy state (see ScanPlanner).  On first signal
        // detection the engine runs fast AFC settling passes in-place (no
        // decode), then the planner resets the scan to that position so the
        // first full decode attempt uses a converged AFC correction.  Without
        // this, afc_step=0.1 takes ~22 scan positions (~704 samples) to
        // converge, by which point the scan has advanced past the preamble
        // start and can never re-decode it.
        let mut planner = ScanPlanner::new(step, min_frame_samples);
        // Round-robin forward-onset offset for the first-energy re-decode (see the
        // fep block below).  Persisted across iterations so each iteration tries
        // exactly ONE onset — running all offsets per iteration starves the read
        // loop (each BPSK31 decode is a full demod of a multi-second window), so
        // the frame never finishes buffering.
        let mut fep_offset_k = 0usize;

        loop {
            let chunk = stream
                .read()
                .map_err(|e| ModemError::Audio(e.to_string()))?;
            if !chunk.is_empty() {
                accumulated.extend(chunk);
                debug!("received {} accumulated audio samples", accumulated.len());
            }

            // One-shot full-frame retry around the first-energy position.
            // Fires when accumulated ≥ fep + max_frame_samples.  By then the full
            // frame is in the buffer.  Retry positions span fep ± one symbol period
            // (step samples) only — NOT a full preamble lookback.  The preamble must
            // be near the START of each slice so that find_timing_offset (which only
            // searches within one symbol period) can locate it.  Earlier runs used
            // fep ± PREAMBLE_SYMS (1024 samples) which placed the preamble 32 symbols
            // into the slice for positions before fep, causing find_timing_offset to
            // return a garbage offset and decode the preamble bits as frame data.
            // Retry fires when enough audio has accumulated to guarantee the
            // full frame is in the buffer:
            //   accumulated ≥ signal_arrival_samples + frame_size
            //
            // For 8 kHz (loopback / IC-9700 USB):
            //   signal_arrival ≈ IRS_STARTUP_WAIT × 8000 = 40 000 samples
            //   frame_size     ≈ 20 224 samples  (BPSK250, 64 B payload)
            //   minimum        ≈ 60 000 → 7.5 s at 8 kHz
            //
            // For FT-991A PipeWire (~3 600 effective samples/s):
            //   signal_arrival ≈ 6 s × 3 600 = 21 600 samples
            //   frame_size     ≈ 20 224 samples
            //   minimum        ≈ 42 000; trigger at 60 000 → fires at 16.7 s
            //   IRS kill window ≈ 17 s — just within budget
            //
            // A fep-relative threshold (fep + N) fails when fep fires on
            // early noise far before the signal; the fixed count avoids that.
            // Wall-clock trigger: retry fires every 2 s starting at T=12 s.
            // The FT-991A PipeWire effective rate varies between 2 300 and
            // 7 600 samples/s, making sample-count thresholds unreliable.
            // At the first firing the signal may not yet be fully buffered
            // (slice too short → CRC fails).  Re-firing every 2 s lets each
            // subsequent attempt use a longer accumulated buffer until the
            // frame fits and the decode succeeds.
            // The full-buffer retry is the fallback for a missed settle.  For
            // long-frame modes only, skip it once settled: its O(buffer) re-scan
            // every 2 s outlasts the read cadence on a multi-second frame (BPSK31),
            // starving the loop so the frame never finishes buffering — and the
            // first-energy micro-sweep below already owns the decode there.  Other
            // modes (notably wideband SCFDMA/OFDM, whose marginal settle the
            // single-carrier micro-sweep can't decode) keep the retry; their short
            // frames re-scan cheaply, so it never starves them.
            let elapsed_secs = start_time.elapsed().as_secs();
            if (!long_frame || !planner.is_settled())
                && planner.retry_due(elapsed_secs, accumulated.len())
            {
                {
                    // Scan the entire accumulated buffer from the start.
                    // The AFC correction is kept from the settled value:
                    // when settling succeeded on the real signal the
                    // correction is valid (e.g. −43.8 Hz carrier offset).
                    // A 43.8 Hz offset at 250 baud causes a 63° phase ramp
                    // per symbol, which flips preamble bits after 2 symbols
                    // and prevents timing lock — resetting to 0 would cause
                    // all retry positions to fail even when the signal is
                    // present.  If settling was rejected (saved_afc = 0)
                    // the retry falls back to AFC=0 naturally.
                    let retry_end = accumulated.len().saturating_sub(min_frame_samples);
                    let saved_afc = self.afc_correction_hz;
                    for start in (0..=retry_end).step_by(step) {
                        let gate_end = (start + acq_samples).min(accumulated.len());
                        let gate_len = gate_end - start;
                        // Adaptive energy gate: skip silent positions.  The
                        // mini-settle AFC stability guard (divergence check
                        // below) handles noise positions that pass this gate
                        // by rejecting them before the expensive decode runs.
                        if gate_len > 0 {
                            let msq = accumulated[start..gate_end]
                                .iter()
                                .map(|s| s * s)
                                .sum::<f32>()
                                / gate_len as f32;
                            if !energy_gate.passes(msq) {
                                continue;
                            }
                        }
                        // Mini-settle: 6 fast AFC passes refine the carrier
                        // estimate before the full decode (anchor + fine, see
                        // afc_mini_settle).  Only skip if the result diverged
                        // past the Goertzel acquisition limit — a convergence
                        // guard on |change| would incorrectly block signals at
                        // exactly fc (0 Hz offset) and signals at the Goertzel
                        // boundary (which saturate and accumulate).
                        let settle_end = (start + afc_window).min(accumulated.len());
                        if settle_end - start >= afc_window {
                            let settle =
                                self.afc_mini_settle(mode, &accumulated[start..settle_end]);
                            // Stability guard: reject if the fine-track
                            // drifted >20 Hz from the anchor (unstable noise)
                            // or exceeded the Goertzel range.  The energy gate
                            // above already filters silence; this catches
                            // noise that slips through.
                            if (settle.fine - settle.anchor).abs() > 20.0
                                || settle.fine.abs() > AFC_MAX_CORRECTION_HZ
                            {
                                self.afc_correction_hz = saved_afc;
                                continue;
                            }
                        }
                        let end = (start + max_frame_samples).min(accumulated.len());
                        if end.saturating_sub(start) < min_frame_samples {
                            continue;
                        }
                        debug!(
                            "AFC full-retry: pos={start} correction={:.1}Hz",
                            self.afc_correction_hz
                        );
                        match self.decode_attempt(
                            mode,
                            AudioSamples {
                                samples: accumulated[start..end].to_vec(),
                            },
                            fec,
                        ) {
                            Ok(payload) => return Ok(payload),
                            Err(err) => {
                                debug!("AFC full-retry: pos={start} FAILED: {err}");
                                last_err = Some(err);
                                self.afc_correction_hz = saved_afc;
                            }
                        }
                    }
                    self.afc_correction_hz = saved_afc;
                }
            }

            // Once settling has located the preamble (first_energy_pos), re-decode
            // from there on EVERY iteration with the current — possibly grown —
            // buffer.  A long frame preceded by silence (e.g. BPSK31: ~12 s frame
            // after the IRS startup wait) may not have fully arrived when the
            // preamble position was first scanned, giving a truncated window ("no
            // data symbols after preamble"); the broad scan then advances past it
            // via commit_scan and never returns, so without this the frame never
            // decodes.  Bounded to one decode per iteration.
            if let Some(fep) = planner.first_energy_pos() {
                // Forward onset micro-sweep.  The settled onset (`fep`) lands at or
                // slightly before the true preamble, but the energy gate + refine
                // can sit up to ~1-2 symbols early on a clean turn-on, and a
                // demodulator only searches one symbol period for timing.  The
                // decodable onset window is narrow (~2 symbols) and asymmetric — a
                // start can be ~1.5 symbols early but barely a third of a symbol
                // late — so the lowest baud rate (BPSK31, 256 samples/symbol) sits
                // right at the boundary and fails on runs where the estimate lands
                // a touch too early.  `fep` is never *after* the onset (the gate
                // trips on the rising edge or before), so sweeping a few half-symbol
                // steps FORWARD reliably lands one attempt inside the window.  The
                // extra attempts only run once the frame is fully buffered (a short
                // buffer fails "frame too short" for every forward offset too, so we
                // skip the sweep in that case and just wait for more audio).
                // Forward-onset micro-sweep, ONE offset per iteration.  The settled
                // onset sits at or slightly before the true preamble (the gate trips
                // on the rising edge or earlier), but the energy gate + refine can be
                // up to ~1-2 symbols early on a clean turn-on, and the demodulator
                // only searches one symbol period for timing.  The decodable onset
                // window is narrow (~2 symbols) and asymmetric — a start may be ~1.5
                // symbols early but barely a third late — so the lowest baud rate
                // (BPSK31) sits at the boundary and fails on runs where the estimate
                // lands a touch early.  Stepping a few half-symbols FORWARD lands one
                // attempt in the window.  Critically this cycles ONE offset per
                // iteration (not all at once): each BPSK31 decode demodulates a
                // multi-second window, so sweeping every offset per read would starve
                // the loop and the long frame would never finish buffering.
                let half = (step / 2).max(1);
                let onset = fep + (fep_offset_k % 9) * half;
                fep_offset_k = fep_offset_k.wrapping_add(1);
                let end = (onset + max_frame_samples).min(accumulated.len());
                if end.saturating_sub(onset) >= min_frame_samples {
                    let afc_before = self.afc_correction_hz;
                    match self.decode_attempt(
                        mode,
                        AudioSamples {
                            samples: accumulated[onset..end].to_vec(),
                        },
                        fec,
                    ) {
                        Ok(payload) => return Ok(payload),
                        Err(err) => {
                            last_err = Some(err);
                            self.afc_correction_hz = afc_before;
                        }
                    }
                }
            }

            // Broad scan to LOCATE the first signal energy and settle AFC.  Once
            // settled, the first-energy re-decode above owns the decode (re-trying
            // the preamble as the buffer grows), so the broad scan stops: continuing
            // it would re-decode every forward position on a full-buffer window each
            // iteration, starving the loop so the frame never finishes buffering.
            // The T>=12 s full-buffer retry remains as a fallback for a bad settle.
            if !accumulated.is_empty() && !planner.is_settled() {
                'inner: for start in planner.scan_positions(accumulated.len()) {
                    // Fast energy gate: check the first 32 symbol periods at this
                    // position.  Silence costs < 0.1 ms; only emit the full
                    // demodulation call (≈ 90 ms on a Pi 4) when signal is present.
                    let gate_end = (start + acq_samples).min(accumulated.len());
                    let gate_len = gate_end - start;
                    let mean_sq = if gate_len > 0 {
                        accumulated[start..gate_end]
                            .iter()
                            .map(|s| s * s)
                            .sum::<f32>()
                            / gate_len as f32
                    } else {
                        0.0
                    };
                    if !energy_gate.passes(mean_sq) {
                        continue;
                    }

                    // On the very first signal-energy position, run 6 fast AFC
                    // estimation passes in-place before attempting any decode.
                    // A temporary step of 0.7 converges in 6 iterations:
                    // (1 − 0.3⁶) × 150 Hz ≈ 149.9 Hz — effectively one-shot for
                    // crystal errors up to ±300 Hz on 144 MHz (≈ ±2 ppm).
                    if !planner.is_settled() {
                        // Refine the coarse gate position to the true signal onset
                        // BEFORE settling AFC.  The energy gate can trip up to a full
                        // acquisition window early, with the signal entering only at the
                        // window tail (e.g. QPSK500: the gate trips ~240 samples before
                        // the frame).  Settling at the coarse position then runs the
                        // carrier estimator over a mostly-silent window, which yields a
                        // confident-but-bogus correction (QPSK500: a stable ~257 Hz from
                        // ~2 signal symbols, last_delta≈0 so it passes the convergence
                        // guard) that breaks the decode at the correct onset.  Settling
                        // from the onset keeps the window on signal.
                        let onset = refine_onset(&accumulated, start, acq_samples, step);
                        // Settle over `afc_window` (preamble + 1 symbol) from the onset,
                        // NOT max_frame_samples: the latter makes settling O(N²) in buffer
                        // length when the noise floor is above the gate (every position
                        // fires the gate, each runs 6 Goertzel passes on the full slice)
                        // and the scan falls behind live audio.  afc_window is
                        // ~preamble-sized (fast) yet long enough to engage the plugin's
                        // fine AFC stage — see its definition above.
                        let settle_end = (onset + afc_window).min(accumulated.len());
                        if settle_end - onset < afc_window {
                            // The onset's signal window is not fully buffered yet; wait for
                            // the next read (the broad scan re-runs as the buffer grows).
                            continue;
                        }
                        let settle = self.afc_mini_settle(mode, &accumulated[onset..settle_end]);
                        // Stability check: the final fine pass must have converged (small
                        // last delta), the fine track must agree with the anchor within
                        // 20 Hz (real carrier), and the magnitude must not exceed the
                        // Goertzel acquisition range.
                        let converged =
                            settle.last_delta < 5.0 && (settle.fine - settle.anchor).abs() <= 20.0;
                        let plausible = settle.fine.abs() <= AFC_MAX_CORRECTION_HZ;
                        if !converged || !plausible {
                            debug!(
                                "AFC settling rejected at onset={onset} (coarse={start}): \
                                 converged={converged} plausible={plausible} \
                                 correction={:.1}Hz",
                                self.afc_correction_hz
                            );
                            self.afc_correction_hz = 0.0;
                            continue;
                        }
                        planner.note_settled(onset);
                        info!(
                            "AFC settling done: correction={:.1}Hz onset={onset} (coarse={start}) buf_len={}",
                            self.afc_correction_hz,
                            accumulated.len()
                        );
                        break 'inner;
                    }

                    // Bound the demodulation window to one maximum-length frame so
                    // the per-attempt cost does not grow with accumulated buffer size.
                    let end = (start + max_frame_samples).min(accumulated.len());
                    // Save AFC state before each decode attempt: on failure the
                    // attempted demodulation has already called update_afc_estimate
                    // (step=0.1 per call).  Without the restore, ~1744 failed
                    // attempts per outer loop accumulate >1000 Hz of drift.
                    let afc_before = self.afc_correction_hz;
                    debug!("AFC decode: pos={} correction={:.1}Hz", start, afc_before);
                    match self.decode_attempt(
                        mode,
                        AudioSamples {
                            samples: accumulated[start..end].to_vec(),
                        },
                        fec,
                    ) {
                        Ok(payload) => return Ok(payload),
                        Err(err) => {
                            last_err = Some(err);
                            self.afc_correction_hz = afc_before;
                        }
                    }
                }
                planner.commit_scan(accumulated.len());
            }

            if Instant::now() >= deadline {
                break;
            }
        }

        Err(last_err.unwrap_or_else(|| {
            ModemError::Demodulation(format!(
                "no decodable frame within {} ms",
                listen_for.as_millis()
            ))
        }))
    }

    fn receive_from_samples(
        &mut self,
        mode: &str,
        samples: AudioSamples,
    ) -> Result<Vec<u8>, ModemError> {
        let samples = self.route_audio_stage(PipelineStage::InputCapture, samples)?;

        let prev_busy = self.dcd.is_busy();
        self.dcd.update(&samples.samples);
        let now_busy = self.dcd.is_busy();
        if now_busy != prev_busy {
            let _ = self.event_tx.send(EngineEvent::DcdChange {
                busy: now_busy,
                energy: self.dcd.energy(),
            });
        }

        info!("received {} audio samples", samples.samples.len());

        let (wire, snr_opt) = {
            let plugin = self
                .plugins
                .get(mode)
                .ok_or_else(|| ModemError::PluginNotFound(mode.to_string()))?;
            let mod_cfg = ModulationConfig {
                mode: mode.to_string(),
                center_frequency: self.center_frequency + self.afc_correction_hz,
                afc_correction_hz: self.afc_correction_hz,
                ..ModulationConfig::default()
            };
            // Prefer soft demodulation: a single pass yields both LLRs (for SNR)
            // and hard bits (via sign decision), avoiding a redundant demodulate() call.
            // Only plugins that declare soft support take this path; for them a soft
            // error is a genuine demodulation failure, not a cue to re-demodulate hard
            // (which would double the per-attempt cost and can't succeed where the
            // soft pass failed — both share the same acquisition front end).
            if plugin.supports_soft_demod() {
                let llrs = plugin.demodulate_soft(&samples.samples, &mod_cfg)?;
                // Absolute RX SNR for rate adaptation: silence-gated M2M4 on the
                // captured envelope. The old mean-|LLR| proxy reads ≈ −2 dB on a
                // clean path (it is only a relative confidence indicator) and so
                // can't drive the SNR-hint ladder; M2M4 is a real dB estimate.
                let snr = openpulse_core::snr_estimate::m2m4_snr_db_gated_from_real(
                    &samples.samples,
                    self.center_frequency + self.afc_correction_hz,
                    AudioConfig::default().sample_rate as f32,
                );
                let wire_bytes: Vec<u8> = llrs
                    .chunks(8)
                    .map(|byte_llrs| {
                        byte_llrs
                            .iter()
                            .enumerate()
                            .fold(0u8, |acc, (i, &llr)| acc | (u8::from(llr <= 0.0) << i))
                    })
                    .collect();
                (WirePayload { bytes: wire_bytes }, Some(snr))
            } else {
                let wire = self.stage_demodulate_payload(plugin, mode, &samples)?;
                (wire, None)
            }
        };
        if let Some(snr) = snr_opt {
            self.rate_policy.record_rx_snr(snr);
        }
        let wire = self.route_wire_stage(PipelineStage::DemodulateDecode, wire)?;
        debug!("demodulated {} bytes", wire.bytes.len());

        self.update_afc_estimate(mode, &samples.samples);
        if let Some(hz) = self.last_afc_offset_hz {
            let _ = self.event_tx.send(EngineEvent::AfcUpdate {
                offset_hz: hz,
                correction_hz: self.afc_correction_hz,
                mode: mode.to_string(),
            });
        }

        let frame = self.stage_decode_frame(&wire)?;
        let frame = self.route_decoded_stage(PipelineStage::HpxStateUpdate, frame)?;
        info!("received frame seq={}", frame.sequence);

        let _ = self.event_tx.send(EngineEvent::FrameReceived {
            mode: mode.to_string(),
            bytes: frame.payload.len(),
        });
        Ok(frame.payload)
    }

    /// Dispatch one decode attempt: no-FEC uses the unchanged
    /// [`receive_from_samples`](Self::receive_from_samples); otherwise the
    /// FEC-aware path. Keeps the `FecMode::None` behaviour byte-identical.
    fn decode_attempt(
        &mut self,
        mode: &str,
        samples: AudioSamples,
        fec: FecMode,
    ) -> Result<Vec<u8>, ModemError> {
        match fec {
            FecMode::None => self.receive_from_samples(mode, samples),
            _ => self.receive_from_samples_with_fec(mode, samples, fec),
        }
    }

    /// FEC-aware counterpart of [`receive_from_samples`](Self::receive_from_samples):
    /// demodulate the slice, apply codec `fec`, then decode the frame. Mirrors the
    /// one-shot `receive_with_*_fec` methods but operates on a provided sample slice
    /// so the timeout-scanning loop can apply FEC per attempt.
    fn receive_from_samples_with_fec(
        &mut self,
        mode: &str,
        samples: AudioSamples,
        fec: FecMode,
    ) -> Result<Vec<u8>, ModemError> {
        let samples = self.route_audio_stage(PipelineStage::InputCapture, samples)?;

        let prev_busy = self.dcd.is_busy();
        self.dcd.update(&samples.samples);
        if self.dcd.is_busy() != prev_busy {
            let _ = self.event_tx.send(EngineEvent::DcdChange {
                busy: self.dcd.is_busy(),
                energy: self.dcd.energy(),
            });
        }

        let mod_cfg = ModulationConfig {
            mode: mode.to_string(),
            center_frequency: self.center_frequency + self.afc_correction_hz,
            afc_correction_hz: self.afc_correction_hz,
            ..ModulationConfig::default()
        };

        // Turbo is a soft code but a *fixed-block* one (the QPP interleaver block
        // size is `llrs.len()/3`), so the scanning slice's trailing-noise LLRs make
        // the block size wrong — it can't decode through this path and is rejected
        // below. It is deliberately excluded here so it does not pay for a soft
        // demodulation it cannot use.
        let soft = matches!(
            fec,
            FecMode::SoftConcatenated | FecMode::Ldpc | FecMode::LdpcHighRate
        );

        // Soft codecs consume LLRs; hard codecs consume demodulated wire bytes.
        let (llrs, raw_wire) = {
            let plugin = self
                .plugins
                .get(mode)
                .ok_or_else(|| ModemError::PluginNotFound(mode.to_string()))?;
            if soft {
                (
                    Some(plugin.demodulate_soft(&samples.samples, &mod_cfg)?),
                    None,
                )
            } else {
                (
                    None,
                    Some(self.stage_demodulate_payload(plugin, mode, &samples)?),
                )
            }
        };

        // Feed the rate policy an absolute RX SNR whenever soft demod ran — same as the no-FEC
        // path (`receive_from_samples`) and `receive_with_ack_hint`. Without this, an adaptive
        // session that uses FEC got no SNR feedback (the FEC receive path skipped it).
        if llrs.is_some() {
            let snr_db = openpulse_core::snr_estimate::m2m4_snr_db_gated_from_real(
                &samples.samples,
                self.center_frequency + self.afc_correction_hz,
                AudioConfig::default().sample_rate as f32,
            );
            self.rate_policy.record_rx_snr(snr_db);
        }

        self.update_afc_estimate(mode, &samples.samples);
        if let Some(hz) = self.last_afc_offset_hz {
            let _ = self.event_tx.send(EngineEvent::AfcUpdate {
                offset_hz: hz,
                correction_hz: self.afc_correction_hz,
                mode: mode.to_string(),
            });
        }

        let corrected = match fec {
            FecMode::Rs => {
                let wire =
                    self.route_wire_stage(PipelineStage::DemodulateDecode, raw_wire.unwrap())?;
                WirePayload {
                    bytes: FecCodec::new().decode(&wire.bytes)?,
                }
            }
            FecMode::RsInterleaved => {
                let wire =
                    self.route_wire_stage(PipelineStage::DemodulateDecode, raw_wire.unwrap())?;
                let deint = Interleaver::new(DEFAULT_INTERLEAVER_DEPTH).deinterleave(&wire.bytes);
                WirePayload {
                    bytes: FecCodec::new().decode(&deint)?,
                }
            }
            FecMode::SoftConcatenated => {
                let llrs = llrs.unwrap();
                let sv = SoftViterbiCodec.decode_soft(&llrs)?;
                let rs = FecCodec::new().decode(&sv)?;
                self.route_wire_stage(PipelineStage::DemodulateDecode, WirePayload { bytes: rs })?
            }
            FecMode::Ldpc => {
                let llrs = llrs.unwrap();
                let info = decode_ldpc_llrs(&LdpcCodec::new(), &llrs)?;
                self.route_wire_stage(PipelineStage::DemodulateDecode, WirePayload { bytes: info })?
            }
            FecMode::LdpcHighRate => {
                let llrs = llrs.unwrap();
                let info = decode_ldpc_llrs(&LdpcCodec::high_rate(), &llrs)?;
                self.route_wire_stage(PipelineStage::DemodulateDecode, WirePayload { bytes: info })?
            }
            FecMode::Concatenated => {
                let wire =
                    self.route_wire_stage(PipelineStage::DemodulateDecode, raw_wire.unwrap())?;
                let conv = ConvCodec::new().decode(&wire.bytes)?;
                WirePayload {
                    bytes: FecCodec::new().decode(&conv)?,
                }
            }
            FecMode::RsStrong => {
                let wire =
                    self.route_wire_stage(PipelineStage::DemodulateDecode, raw_wire.unwrap())?;
                WirePayload {
                    bytes: FecCodec::strong().decode(&wire.bytes)?,
                }
            }
            // ShortRs (byte-exact, no length prefix) and Turbo (fixed QPP block size
            // = llrs.len()/3) both need the exact frame length, which the scanning
            // receive can't guarantee (trailing-noise samples inflate the count), so
            // they stay single-shot.
            other => {
                return Err(ModemError::Demodulation(format!(
                    "FEC mode {other:?} is not supported by the timeout receive; \
                     use receive_with_fec_mode for a single-shot decode"
                )))
            }
        };

        let frame = self.stage_decode_frame(&corrected)?;
        let frame = self.route_decoded_stage(PipelineStage::HpxStateUpdate, frame)?;
        let _ = self.event_tx.send(EngineEvent::FrameReceived {
            mode: mode.to_string(),
            bytes: frame.payload.len(),
        });
        Ok(frame.payload)
    }

    /// Receive a data frame and derive an ACK type recommendation for the sender.
    ///
    /// This is the full adaptive receive path:
    /// 1. Captures audio samples, demodulates, and decodes the payload (identical
    ///    to [`receive`](Self::receive)).
    /// 2. Estimates receive-path SNR from the mean absolute LLR magnitude.
    /// 3. Applies the SNR estimate to the RX direction of the active rate adapter.
    /// 4. Returns the decoded payload together with the [`AckType`] the caller
    ///    should transmit back to the sender via
    ///    [`transmit_ack_with_short_fec`](Self::transmit_ack_with_short_fec).
    ///
    /// When no adaptive session is active the returned `AckType` is always
    /// [`AckType::AckOk`].
    ///
    /// On decode failure returns `Err`; the caller should transmit
    /// [`AckType::Nack`] in that case.
    pub fn receive_with_ack_hint(
        &mut self,
        mode: &str,
        device: Option<&str>,
    ) -> Result<(Vec<u8>, AckType), ModemError> {
        let samples = self.stage_capture_input(Some(mode), device)?;
        let samples = self.route_audio_stage(PipelineStage::InputCapture, samples)?;

        let prev_busy = self.dcd.is_busy();
        self.dcd.update(&samples.samples);
        if self.dcd.is_busy() != prev_busy {
            let _ = self.event_tx.send(EngineEvent::DcdChange {
                busy: self.dcd.is_busy(),
                energy: self.dcd.energy(),
            });
        }

        self.update_afc_estimate(mode, &samples.samples);
        if let Some(hz) = self.last_afc_offset_hz {
            let _ = self.event_tx.send(EngineEvent::AfcUpdate {
                offset_hz: hz,
                correction_hz: self.afc_correction_hz,
                mode: mode.to_string(),
            });
        }

        let mod_cfg = ModulationConfig {
            mode: mode.to_string(),
            center_frequency: self.center_frequency + self.afc_correction_hz,
            afc_correction_hz: self.afc_correction_hz,
            ..ModulationConfig::default()
        };

        let plugin = self
            .plugins
            .get(mode)
            .ok_or_else(|| ModemError::PluginNotFound(mode.to_string()))?;

        let llrs = plugin.demodulate_soft(&samples.samples, &mod_cfg)?;
        // Absolute SNR for the rate decision: silence-gated M2M4 on the captured
        // envelope (the mean-|LLR| proxy reads ~-2 dB on a clean path and can't drive
        // the ladder).
        let snr_db = openpulse_core::snr_estimate::m2m4_snr_db_gated_from_real(
            &samples.samples,
            self.center_frequency + self.afc_correction_hz,
            AudioConfig::default().sample_rate as f32,
        );
        self.rate_policy.record_rx_snr(snr_db);

        let wire_bytes: Vec<u8> = llrs
            .chunks(8)
            .map(|byte_llrs| {
                byte_llrs
                    .iter()
                    .enumerate()
                    .fold(0u8, |acc, (i, &llr)| acc | (u8::from(llr <= 0.0) << i))
            })
            .collect();

        let wire = WirePayload { bytes: wire_bytes };
        let wire = self.route_wire_stage(PipelineStage::DemodulateDecode, wire)?;
        let frame = self.stage_decode_frame(&wire)?;
        let frame = self.route_decoded_stage(PipelineStage::HpxStateUpdate, frame)?;
        info!(
            "receive_with_ack_hint: seq={} snr={:.1}dB",
            frame.sequence, snr_db
        );

        let _ = self.event_tx.send(EngineEvent::FrameReceived {
            mode: mode.to_string(),
            bytes: frame.payload.len(),
        });

        let ack_type = self.rate_policy.select_rx_ack_type(snr_db);
        Ok((frame.payload, ack_type))
    }

    /// ISS ARQ transmit: send `data`, wait for a FSK4-ACK reply, retry on Nack.
    ///
    /// Transmits the frame up to `1 + max_retries` times (initial attempt plus
    /// retries).  On each attempt:
    /// - A successful ACK (`AckOk`, `AckUp`, `AckDown`) is applied to the TX
    ///   rate adapter and the call returns `Ok(rate_event)`.
    /// - A `Nack` or a receive error is treated as a delivery failure; the TX
    ///   adapter is stepped down and the frame is retransmitted.
    ///
    /// Returns [`ModemError::ArqMaxRetries`] if no ACK is received after all
    /// attempts are exhausted.
    ///
    /// Pass `max_retries = 0` to transmit once with no retry (equivalent to
    /// `transmit` followed by a single `receive_ack_with_short_fec`).
    pub fn transmit_arq(
        &mut self,
        data: &[u8],
        mode: &str,
        device: Option<&str>,
        max_retries: usize,
    ) -> Result<RateEvent, ModemError> {
        let attempts = 1 + max_retries;
        for attempt in 0..attempts {
            let current_mode = self.current_adaptive_mode().unwrap_or(mode).to_owned();
            self.transmit(data, &current_mode, device)?;

            match self.receive_ack_with_short_fec(device) {
                Ok(ack_frame) if ack_frame.ack_type != AckType::Nack => {
                    let rate_event = self.apply_ack_frame(&ack_frame);
                    info!(
                        "ARQ: ACK {:?} after attempt {}/{}",
                        ack_frame.ack_type,
                        attempt + 1,
                        attempts
                    );
                    return Ok(rate_event);
                }
                Ok(_nack) => {
                    // Nack: step down TX rate and retry.
                    let _ = self.apply_ack(AckType::AckDown);
                    info!(
                        "ARQ: Nack on attempt {}/{}, retrying",
                        attempt + 1,
                        attempts
                    );
                }
                Err(e) => {
                    // No ACK received at all: treat as implicit Nack.
                    let _ = self.apply_ack(AckType::AckDown);
                    info!(
                        "ARQ: no ACK on attempt {}/{} ({e}), retrying",
                        attempt + 1,
                        attempts
                    );
                }
            }
        }
        Err(ModemError::ArqMaxRetries(attempts))
    }

    /// IRS side of an ARQ exchange: receive one data frame and reply with an ACK.
    ///
    /// Receives at the current RX adaptive mode when a session is active, else at
    /// `mode`. On a clean decode it replies with the SNR-derived [`AckType`] (always
    /// [`AckType::AckOk`] without an adaptive session) and returns the payload; on
    /// decode failure it replies [`AckType::Nack`] and returns the error, so the
    /// transmitting [`transmit_arq`](Self::transmit_arq) peer retransmits.
    ///
    /// This is the reliable, fixed-mode counterpart to `transmit_arq`. Adaptive
    /// rate-stepping in the RX direction (keeping the IRS RX level in lockstep with
    /// the ISS TX level across an `AckUp`) is layered on top separately.
    pub fn respond_arq(
        &mut self,
        mode: &str,
        session_id: &str,
        device: Option<&str>,
    ) -> Result<Vec<u8>, ModemError> {
        let rx_mode = self.current_rx_mode().unwrap_or(mode).to_owned();
        match self.receive_with_ack_hint(&rx_mode, device) {
            Ok((payload, ack_type)) => {
                let ack = AckFrame::new(ack_type, session_id);
                self.transmit_ack_with_short_fec(&ack, device)?;
                Ok(payload)
            }
            Err(e) => {
                let nack = AckFrame::new(AckType::Nack, session_id);
                let _ = self.transmit_ack_with_short_fec(&nack, device);
                Err(e)
            }
        }
    }

    /// Like [`transmit`](Self::transmit) but wraps the encoded frame bytes
    /// with Reed-Solomon FEC before modulation.
    ///
    /// Set the station callsign used in broadcast frame headers.
    pub fn set_callsign(&mut self, callsign: impl Into<String>) {
        self.callsign = callsign.into();
        self.update_tx_session_callsign();
    }

    /// Set the TX attenuation applied to all transmitted audio (dB; 0.0 = no attenuation).
    ///
    /// Negative values reduce output level; e.g. `-6.0` halves the amplitude.
    /// Call this whenever the rig frequency changes to restore the per-band setting.
    pub fn set_tx_attenuation_db(&mut self, db: f32) {
        self.tx_attenuation_db = db;
    }

    /// Return the current TX attenuation in dB.
    pub fn tx_attenuation_db(&self) -> f32 {
        self.tx_attenuation_db
    }

    /// Set the DCD/squelch RMS threshold — the carrier-present level used by
    /// channel-busy detection, CSMA, and [`capture_burst`](Self::capture_burst)'s
    /// burst-flush. Raise it on a noisy band so the noise floor doesn't read as a
    /// permanent carrier; call on frequency change to restore the per-band value.
    pub fn set_dcd_squelch(&mut self, threshold: f32) {
        self.dcd.set_threshold(threshold);
    }

    /// Return the current DCD/squelch RMS threshold.
    pub fn dcd_squelch(&self) -> f32 {
        self.dcd.threshold()
    }

    /// Set the soft TX limiter threshold (0.0 disables the limiter).
    pub fn set_tx_limiter_threshold(&mut self, threshold: f32) {
        self.tx_limiter_threshold = threshold;
    }

    /// Set the maximum TX power in watts for regulatory compliance (0.0 = no limit).
    pub fn set_max_power_watts(&mut self, watts: f32) {
        self.max_power_watts = watts.max(0.0);
    }

    /// Return the current maximum TX power limit in watts.
    pub fn max_power_watts(&self) -> f32 {
        self.max_power_watts
    }

    /// Return reference to the transmission session log for regulatory compliance.
    pub fn tx_session_log(&self) -> &TxSessionLog {
        &self.tx_session_log
    }

    /// Clear the transmission session log.
    pub fn clear_tx_session_log(&mut self) {
        self.tx_session_log = TxSessionLog::new(self.callsign.clone());
    }

    /// Update callsign in active TX session log.
    fn update_tx_session_callsign(&mut self) {
        self.tx_session_log.station_id = self.callsign.clone();
    }

    /// Unlike [`transmit`](Self::transmit), this method bypasses the CSMA
    /// persistence check — broadcasts are short, and the sender is responsible
    /// for scheduling.  No ACK is expected; no session state is updated.
    ///
    /// The frame is wrapped in a `BroadcastFrame` payload inside a `WireEnvelope`
    /// with `dst_peer_id = [0; 32]` (broadcast address) and `hop_index = 0`.
    /// `ttl` limits how many times relay nodes may re-broadcast the frame.
    pub fn broadcast(
        &mut self,
        payload: &[u8],
        mode: &str,
        ttl: u8,
        device: Option<&str>,
    ) -> Result<(), ModemError> {
        let seq = self.broadcast_seq;
        self.broadcast_seq = self.broadcast_seq.wrapping_add(1);

        let frame = BroadcastFrame {
            callsign_hash: callsign_hash(&self.callsign),
            seq,
            ttl,
            flags: 0,
            payload: payload.to_vec(),
        };

        let envelope = WireEnvelope {
            msg_type: WireMsgType::BroadcastFrame,
            flags: 0,
            session_id: 0,
            src_peer_id: [0u8; 32],
            dst_peer_id: [0u8; 32], // broadcast address
            nonce: nonce_from_seq(seq),
            timestamp_ms: 0,
            hop_limit: ttl,
            hop_index: 0,
            payload: frame.encode(),
            auth_tag: [0u8; 16],
        };

        let wire_bytes = envelope
            .encode()
            .map_err(|e| ModemError::Configuration(e.to_string()))?;

        let outbound = self.stage_encode_frame(&wire_bytes)?;
        let outbound = self.route_wire_stage(PipelineStage::EncodeModulate, outbound)?;

        let samples = {
            let plugin = self
                .plugins
                .get(mode)
                .ok_or_else(|| ModemError::PluginNotFound(mode.to_string()))?;
            self.stage_modulate_payload(plugin, mode, &outbound)?
        };
        let samples = self.route_audio_stage(PipelineStage::OutputEmit, samples)?;
        self.stage_emit_output(device, mode, &samples)?;

        let _ = self.event_tx.send(EngineEvent::FrameTransmitted {
            mode: mode.to_string(),
            bytes: payload.len(),
        });
        Ok(())
    }

    /// On a noisy channel the receiver can use [`receive_with_fec`](Self::receive_with_fec)
    /// to correct up to **16 byte errors per 255-byte RS block** after
    /// demodulation.
    pub fn transmit_with_fec(
        &mut self,
        data: &[u8],
        mode: &str,
        device: Option<&str>,
    ) -> Result<(), ModemError> {
        self.csma_check()?;

        let frame_wire = self.stage_encode_frame(data)?;
        let fec_bytes = FecCodec::new().encode(&frame_wire.bytes);
        let fec_wire = WirePayload { bytes: fec_bytes };
        let fec_wire = self.route_wire_stage(PipelineStage::EncodeModulate, fec_wire)?;

        debug!(
            "FEC transmitting {} byte FEC block (seq={}, mode={mode})",
            fec_wire.bytes.len(),
            self.sequence.wrapping_sub(1)
        );

        let samples = {
            let plugin = self
                .plugins
                .get(mode)
                .ok_or_else(|| ModemError::PluginNotFound(mode.to_string()))?;
            self.stage_modulate_payload(plugin, mode, &fec_wire)?
        };
        let samples = self.route_audio_stage(PipelineStage::OutputEmit, samples)?;
        self.stage_emit_output(device, mode, &samples)?;
        let _ = self.event_tx.send(EngineEvent::FrameTransmitted {
            mode: mode.to_string(),
            bytes: fec_wire.bytes.len(),
        });
        Ok(())
    }

    /// Like [`receive`](Self::receive) but applies Reed-Solomon FEC error
    /// correction after demodulation before decoding the frame.
    pub fn receive_with_fec(
        &mut self,
        mode: &str,
        device: Option<&str>,
    ) -> Result<Vec<u8>, ModemError> {
        let samples = self.stage_capture_input(Some(mode), device)?;
        let samples = self.route_audio_stage(PipelineStage::InputCapture, samples)?;

        let prev_busy = self.dcd.is_busy();
        self.dcd.update(&samples.samples);
        if self.dcd.is_busy() != prev_busy {
            let _ = self.event_tx.send(EngineEvent::DcdChange {
                busy: self.dcd.is_busy(),
                energy: self.dcd.energy(),
            });
        }

        let raw_wire = {
            let plugin = self
                .plugins
                .get(mode)
                .ok_or_else(|| ModemError::PluginNotFound(mode.to_string()))?;
            self.stage_demodulate_payload(plugin, mode, &samples)?
        };
        let raw_wire = self.route_wire_stage(PipelineStage::DemodulateDecode, raw_wire)?;

        self.update_afc_estimate(mode, &samples.samples);
        if let Some(hz) = self.last_afc_offset_hz {
            let _ = self.event_tx.send(EngineEvent::AfcUpdate {
                offset_hz: hz,
                correction_hz: self.afc_correction_hz,
                mode: mode.to_string(),
            });
        }

        let corrected_bytes = FecCodec::new().decode(&raw_wire.bytes)?;
        let corrected_wire = WirePayload {
            bytes: corrected_bytes,
        };

        let frame = self.stage_decode_frame(&corrected_wire)?;
        let frame = self.route_decoded_stage(PipelineStage::HpxStateUpdate, frame)?;
        info!("FEC receive: frame seq={}", frame.sequence);

        let _ = self.event_tx.send(EngineEvent::FrameReceived {
            mode: mode.to_string(),
            bytes: frame.payload.len(),
        });
        Ok(frame.payload)
    }

    /// Like [`transmit_with_fec`](Self::transmit_with_fec) but also applies a
    /// stride interleaver after RS encoding so that burst channel errors are
    /// dispersed across blocks before the receiver corrects them.
    pub fn transmit_with_fec_interleaved(
        &mut self,
        data: &[u8],
        mode: &str,
        device: Option<&str>,
        interleaver_depth: usize,
    ) -> Result<(), ModemError> {
        self.csma_check()?;

        let frame_wire = self.stage_encode_frame(data)?;
        let fec_bytes = FecCodec::new().encode(&frame_wire.bytes);
        let interleaved = Interleaver::new(interleaver_depth).interleave(&fec_bytes);
        let il_wire = WirePayload { bytes: interleaved };
        let il_wire = self.route_wire_stage(PipelineStage::EncodeModulate, il_wire)?;

        let samples = {
            let plugin = self
                .plugins
                .get(mode)
                .ok_or_else(|| ModemError::PluginNotFound(mode.to_string()))?;
            self.stage_modulate_payload(plugin, mode, &il_wire)?
        };
        let samples = self.route_audio_stage(PipelineStage::OutputEmit, samples)?;
        self.stage_emit_output(device, mode, &samples)?;
        let _ = self.event_tx.send(EngineEvent::FrameTransmitted {
            mode: mode.to_string(),
            bytes: il_wire.bytes.len(),
        });
        Ok(())
    }
    /// Like [`receive_with_fec`](Self::receive_with_fec) but deinterleaves the
    /// received bytes before RS decoding to undo the transmitter's interleaving.
    pub fn receive_with_fec_interleaved(
        &mut self,
        mode: &str,
        device: Option<&str>,
        interleaver_depth: usize,
    ) -> Result<Vec<u8>, ModemError> {
        let samples = self.stage_capture_input(Some(mode), device)?;
        let samples = self.route_audio_stage(PipelineStage::InputCapture, samples)?;

        let prev_busy = self.dcd.is_busy();
        self.dcd.update(&samples.samples);
        if self.dcd.is_busy() != prev_busy {
            let _ = self.event_tx.send(EngineEvent::DcdChange {
                busy: self.dcd.is_busy(),
                energy: self.dcd.energy(),
            });
        }

        let raw_wire = {
            let plugin = self
                .plugins
                .get(mode)
                .ok_or_else(|| ModemError::PluginNotFound(mode.to_string()))?;
            self.stage_demodulate_payload(plugin, mode, &samples)?
        };
        let raw_wire = self.route_wire_stage(PipelineStage::DemodulateDecode, raw_wire)?;

        self.update_afc_estimate(mode, &samples.samples);
        if let Some(hz) = self.last_afc_offset_hz {
            let _ = self.event_tx.send(EngineEvent::AfcUpdate {
                offset_hz: hz,
                correction_hz: self.afc_correction_hz,
                mode: mode.to_string(),
            });
        }

        let deinterleaved = Interleaver::new(interleaver_depth).deinterleave(&raw_wire.bytes);
        let corrected_bytes = FecCodec::new().decode(&deinterleaved)?;
        let corrected_wire = WirePayload {
            bytes: corrected_bytes,
        };

        let frame = self.stage_decode_frame(&corrected_wire)?;
        let frame = self.route_decoded_stage(PipelineStage::HpxStateUpdate, frame)?;
        let _ = self.event_tx.send(EngineEvent::FrameReceived {
            mode: mode.to_string(),
            bytes: frame.payload.len(),
        });
        Ok(frame.payload)
    }

    /// Transmit with concatenated Conv(rate-1/2) inner + RS outer FEC.
    ///
    /// TX chain: frame encode → RS encode → Conv encode → modulate → emit.
    /// Use [`receive_with_concatenated_fec`](Self::receive_with_concatenated_fec)
    /// on the receive side.
    pub fn transmit_with_concatenated_fec(
        &mut self,
        data: &[u8],
        mode: &str,
        device: Option<&str>,
    ) -> Result<(), ModemError> {
        self.csma_check()?;

        let frame_wire = self.stage_encode_frame(data)?;
        let rs_bytes = FecCodec::new().encode(&frame_wire.bytes);
        let conv_bytes = ConvCodec::new().encode(&rs_bytes);
        let fec_wire = WirePayload { bytes: conv_bytes };
        let fec_wire = self.route_wire_stage(PipelineStage::EncodeModulate, fec_wire)?;

        debug!(
            "concatenated FEC transmitting {} bytes (seq={}, mode={mode})",
            fec_wire.bytes.len(),
            self.sequence.wrapping_sub(1)
        );

        let samples = {
            let plugin = self
                .plugins
                .get(mode)
                .ok_or_else(|| ModemError::PluginNotFound(mode.to_string()))?;
            self.stage_modulate_payload(plugin, mode, &fec_wire)?
        };
        let samples = self.route_audio_stage(PipelineStage::OutputEmit, samples)?;
        self.stage_emit_output(device, mode, &samples)?;
        let _ = self.event_tx.send(EngineEvent::FrameTransmitted {
            mode: mode.to_string(),
            bytes: fec_wire.bytes.len(),
        });
        Ok(())
    }

    /// Receive with concatenated Conv(rate-1/2) inner + RS outer FEC.
    ///
    /// RX chain: capture → demodulate → Conv decode → RS decode → frame decode.
    pub fn receive_with_concatenated_fec(
        &mut self,
        mode: &str,
        device: Option<&str>,
    ) -> Result<Vec<u8>, ModemError> {
        let samples = self.stage_capture_input(Some(mode), device)?;
        let samples = self.route_audio_stage(PipelineStage::InputCapture, samples)?;

        let prev_busy = self.dcd.is_busy();
        self.dcd.update(&samples.samples);
        if self.dcd.is_busy() != prev_busy {
            let _ = self.event_tx.send(EngineEvent::DcdChange {
                busy: self.dcd.is_busy(),
                energy: self.dcd.energy(),
            });
        }

        let raw_wire = {
            let plugin = self
                .plugins
                .get(mode)
                .ok_or_else(|| ModemError::PluginNotFound(mode.to_string()))?;
            self.stage_demodulate_payload(plugin, mode, &samples)?
        };
        let raw_wire = self.route_wire_stage(PipelineStage::DemodulateDecode, raw_wire)?;

        self.update_afc_estimate(mode, &samples.samples);
        if let Some(hz) = self.last_afc_offset_hz {
            let _ = self.event_tx.send(EngineEvent::AfcUpdate {
                offset_hz: hz,
                correction_hz: self.afc_correction_hz,
                mode: mode.to_string(),
            });
        }

        let conv_decoded = ConvCodec::new().decode(&raw_wire.bytes)?;
        let rs_decoded = FecCodec::new().decode(&conv_decoded)?;
        let corrected_wire = WirePayload { bytes: rs_decoded };

        let frame = self.stage_decode_frame(&corrected_wire)?;
        let frame = self.route_decoded_stage(PipelineStage::HpxStateUpdate, frame)?;
        info!("concatenated FEC receive: frame seq={}", frame.sequence);
        let _ = self.event_tx.send(EngineEvent::FrameReceived {
            mode: mode.to_string(),
            bytes: frame.payload.len(),
        });
        Ok(frame.payload)
    }

    /// Transmit with K=7 soft-decision Conv inner + RS outer FEC (BL-FEC-5).
    ///
    /// TX chain: frame encode → RS encode → SoftViterbiCodec encode → modulate → emit.
    pub fn transmit_with_soft_viterbi_fec(
        &mut self,
        data: &[u8],
        mode: &str,
        device: Option<&str>,
    ) -> Result<(), ModemError> {
        self.csma_check()?;

        let frame_wire = self.stage_encode_frame(data)?;
        let rs_bytes = FecCodec::new().encode(&frame_wire.bytes);
        let sv_bytes = SoftViterbiCodec.encode(&rs_bytes);
        let fec_wire = WirePayload { bytes: sv_bytes };
        let fec_wire = self.route_wire_stage(PipelineStage::EncodeModulate, fec_wire)?;

        debug!(
            "soft-Viterbi FEC transmitting {} bytes (seq={}, mode={mode})",
            fec_wire.bytes.len(),
            self.sequence.wrapping_sub(1)
        );

        let samples = {
            let plugin = self
                .plugins
                .get(mode)
                .ok_or_else(|| ModemError::PluginNotFound(mode.to_string()))?;
            self.stage_modulate_payload(plugin, mode, &fec_wire)?
        };
        let samples = self.route_audio_stage(PipelineStage::OutputEmit, samples)?;
        self.stage_emit_output(device, mode, &samples)?;
        let _ = self.event_tx.send(EngineEvent::FrameTransmitted {
            mode: mode.to_string(),
            bytes: fec_wire.bytes.len(),
        });
        Ok(())
    }

    /// Receive with K=7 soft-decision Conv inner + RS outer FEC (BL-FEC-5).
    ///
    /// RX chain: capture → demodulate_soft → SoftViterbiCodec decode → RS decode → frame decode.
    pub fn receive_with_soft_viterbi_fec(
        &mut self,
        mode: &str,
        device: Option<&str>,
    ) -> Result<Vec<u8>, ModemError> {
        let samples = self.stage_capture_input(Some(mode), device)?;
        let samples = self.route_audio_stage(PipelineStage::InputCapture, samples)?;

        let prev_busy = self.dcd.is_busy();
        self.dcd.update(&samples.samples);
        if self.dcd.is_busy() != prev_busy {
            let _ = self.event_tx.send(EngineEvent::DcdChange {
                busy: self.dcd.is_busy(),
                energy: self.dcd.energy(),
            });
        }

        let llrs = {
            let plugin = self
                .plugins
                .get(mode)
                .ok_or_else(|| ModemError::PluginNotFound(mode.to_string()))?;
            let mod_cfg = ModulationConfig {
                mode: mode.to_string(),
                center_frequency: self.center_frequency + self.afc_correction_hz,
                ..ModulationConfig::default()
            };
            plugin.demodulate_soft(&samples.samples, &mod_cfg)?
        };

        self.update_afc_estimate(mode, &samples.samples);
        if let Some(hz) = self.last_afc_offset_hz {
            let _ = self.event_tx.send(EngineEvent::AfcUpdate {
                offset_hz: hz,
                correction_hz: self.afc_correction_hz,
                mode: mode.to_string(),
            });
        }

        let sv_decoded = SoftViterbiCodec.decode_soft(&llrs)?;
        let rs_decoded = FecCodec::new().decode(&sv_decoded)?;
        let corrected_wire = WirePayload { bytes: rs_decoded };
        let corrected_wire =
            self.route_wire_stage(PipelineStage::DemodulateDecode, corrected_wire)?;

        let frame = self.stage_decode_frame(&corrected_wire)?;
        let frame = self.route_decoded_stage(PipelineStage::HpxStateUpdate, frame)?;
        info!("soft-Viterbi FEC receive: frame seq={}", frame.sequence);
        let _ = self.event_tx.send(EngineEvent::FrameReceived {
            mode: mode.to_string(),
            bytes: frame.payload.len(),
        });
        Ok(frame.payload)
    }

    /// Transmit with RS(255,191) t=32 strong FEC (corrects up to 32 byte errors/block).
    ///
    /// TX chain: frame encode → RS strong encode → modulate → emit.
    pub fn transmit_with_strong_fec(
        &mut self,
        data: &[u8],
        mode: &str,
        device: Option<&str>,
    ) -> Result<(), ModemError> {
        self.csma_check()?;
        let wire = self.stage_encode_frame(data)?;
        let fec_wire = WirePayload {
            bytes: FecCodec::strong().encode(&wire.bytes),
        };
        let fec_wire = self.route_wire_stage(PipelineStage::EncodeModulate, fec_wire)?;
        let samples = {
            let plugin = self
                .plugins
                .get(mode)
                .ok_or_else(|| ModemError::PluginNotFound(mode.to_string()))?;
            self.stage_modulate_payload(plugin, mode, &fec_wire)?
        };
        let samples = self.route_audio_stage(PipelineStage::OutputEmit, samples)?;
        self.stage_emit_output(device, mode, &samples)?;
        let _ = self.event_tx.send(EngineEvent::FrameTransmitted {
            mode: mode.to_string(),
            bytes: fec_wire.bytes.len(),
        });
        Ok(())
    }

    /// Receive with RS(255,191) t=32 strong FEC.
    ///
    /// RX chain: capture → demodulate → RS strong decode → frame decode.
    pub fn receive_with_strong_fec(
        &mut self,
        mode: &str,
        device: Option<&str>,
    ) -> Result<Vec<u8>, ModemError> {
        let samples = self.stage_capture_input(Some(mode), device)?;
        let samples = self.route_audio_stage(PipelineStage::InputCapture, samples)?;

        let prev_busy = self.dcd.is_busy();
        self.dcd.update(&samples.samples);
        if self.dcd.is_busy() != prev_busy {
            let _ = self.event_tx.send(EngineEvent::DcdChange {
                busy: self.dcd.is_busy(),
                energy: self.dcd.energy(),
            });
        }

        let raw_wire = {
            let plugin = self
                .plugins
                .get(mode)
                .ok_or_else(|| ModemError::PluginNotFound(mode.to_string()))?;
            self.stage_demodulate_payload(plugin, mode, &samples)?
        };
        let raw_wire = self.route_wire_stage(PipelineStage::DemodulateDecode, raw_wire)?;

        self.update_afc_estimate(mode, &samples.samples);
        if let Some(hz) = self.last_afc_offset_hz {
            let _ = self.event_tx.send(EngineEvent::AfcUpdate {
                offset_hz: hz,
                correction_hz: self.afc_correction_hz,
                mode: mode.to_string(),
            });
        }

        let rs_decoded = FecCodec::strong().decode(&raw_wire.bytes)?;
        let frame = self.stage_decode_frame(&WirePayload { bytes: rs_decoded })?;
        let frame = self.route_decoded_stage(PipelineStage::HpxStateUpdate, frame)?;
        let _ = self.event_tx.send(EngineEvent::FrameReceived {
            mode: mode.to_string(),
            bytes: frame.payload.len(),
        });
        Ok(frame.payload)
    }

    /// Transmit with rate-1/2 LDPC FEC (1024 info bits → 2048 codeword bits, min-sum BP).
    ///
    /// TX chain: frame encode → LDPC encode (128 B → 256 B) → modulate → emit.
    ///
    /// The encoded frame must fit in one LDPC block (≤ 128 bytes).  For larger
    /// payloads split them at the session layer before calling this method.
    pub fn transmit_with_ldpc(
        &mut self,
        data: &[u8],
        mode: &str,
        device: Option<&str>,
    ) -> Result<(), ModemError> {
        self.transmit_with_ldpc_codec(data, mode, &LdpcCodec::new(), device)
    }

    /// Transmit with high-rate LDPC FEC (rate ≈8/9, 1024 info bits → 1152 codeword
    /// bits) for the dense, high-SNR rungs (8PSK / 16QAM / 32APSK).
    ///
    /// TX chain: frame encode → LDPC encode (128 B → 144 B) → modulate → emit.
    /// Same single-block limit (≤ 128 bytes) as [`transmit_with_ldpc`](Self::transmit_with_ldpc).
    pub fn transmit_with_ldpc_high_rate(
        &mut self,
        data: &[u8],
        mode: &str,
        device: Option<&str>,
    ) -> Result<(), ModemError> {
        self.transmit_with_ldpc_codec(data, mode, &LdpcCodec::high_rate(), device)
    }

    /// Transmit one frame through the given LDPC codec preset.  Shared by the
    /// rate-1/2 and high-rate public methods; the single-block limit comes from
    /// the codec's own `info_bytes()`.
    fn transmit_with_ldpc_codec(
        &mut self,
        data: &[u8],
        mode: &str,
        codec: &LdpcCodec,
        device: Option<&str>,
    ) -> Result<(), ModemError> {
        self.csma_check()?;

        let frame_wire = self.stage_encode_frame(data)?;
        if frame_wire.bytes.len() > codec.info_bytes() {
            return Err(ModemError::Frame(format!(
                "LDPC: encoded frame {} B exceeds one-block limit of {} B; split payload at call site",
                frame_wire.bytes.len(),
                codec.info_bytes(),
            )));
        }
        let codeword = codec.encode(&frame_wire.bytes);
        let fec_wire = WirePayload { bytes: codeword };
        let fec_wire = self.route_wire_stage(PipelineStage::EncodeModulate, fec_wire)?;

        debug!(
            "LDPC transmitting {} B codeword (seq={}, mode={mode})",
            fec_wire.bytes.len(),
            self.sequence.wrapping_sub(1)
        );

        let samples = {
            let plugin = self
                .plugins
                .get(mode)
                .ok_or_else(|| ModemError::PluginNotFound(mode.to_string()))?;
            self.stage_modulate_payload(plugin, mode, &fec_wire)?
        };
        let samples = self.route_audio_stage(PipelineStage::OutputEmit, samples)?;
        self.stage_emit_output(device, mode, &samples)?;
        let _ = self.event_tx.send(EngineEvent::FrameTransmitted {
            mode: mode.to_string(),
            bytes: fec_wire.bytes.len(),
        });
        Ok(())
    }

    /// Receive with rate-1/2 LDPC FEC via min-sum belief propagation.
    ///
    /// RX chain: capture → demodulate_soft → LDPC decode_soft → frame decode.
    pub fn receive_with_ldpc(
        &mut self,
        mode: &str,
        device: Option<&str>,
    ) -> Result<Vec<u8>, ModemError> {
        self.receive_with_ldpc_codec(mode, &LdpcCodec::new(), device)
    }

    /// Receive with high-rate LDPC FEC (rate ≈8/9) for the dense, high-SNR rungs.
    ///
    /// Mirror of [`receive_with_ldpc`](Self::receive_with_ldpc) with the
    /// [`LdpcCodec::high_rate`] preset.
    pub fn receive_with_ldpc_high_rate(
        &mut self,
        mode: &str,
        device: Option<&str>,
    ) -> Result<Vec<u8>, ModemError> {
        self.receive_with_ldpc_codec(mode, &LdpcCodec::high_rate(), device)
    }

    /// Receive one frame through the given LDPC codec preset.  Shared by the
    /// rate-1/2 and high-rate public methods; the LLR slice length comes from the
    /// codec's own `codeword_bytes()`.
    fn receive_with_ldpc_codec(
        &mut self,
        mode: &str,
        codec: &LdpcCodec,
        device: Option<&str>,
    ) -> Result<Vec<u8>, ModemError> {
        let samples = self.stage_capture_input(Some(mode), device)?;
        let samples = self.route_audio_stage(PipelineStage::InputCapture, samples)?;

        let prev_busy = self.dcd.is_busy();
        self.dcd.update(&samples.samples);
        if self.dcd.is_busy() != prev_busy {
            let _ = self.event_tx.send(EngineEvent::DcdChange {
                busy: self.dcd.is_busy(),
                energy: self.dcd.energy(),
            });
        }

        let llrs = {
            let plugin = self
                .plugins
                .get(mode)
                .ok_or_else(|| ModemError::PluginNotFound(mode.to_string()))?;
            let mod_cfg = ModulationConfig {
                mode: mode.to_string(),
                center_frequency: self.center_frequency + self.afc_correction_hz,
                ..ModulationConfig::default()
            };
            plugin.demodulate_soft(&samples.samples, &mod_cfg)?
        };

        self.update_afc_estimate(mode, &samples.samples);
        if let Some(hz) = self.last_afc_offset_hz {
            let _ = self.event_tx.send(EngineEvent::AfcUpdate {
                offset_hz: hz,
                correction_hz: self.afc_correction_hz,
                mode: mode.to_string(),
            });
        }

        // LDPC block is codeword_bytes × 8 coded bits; trim any excess LLRs.
        let info_bytes = decode_ldpc_llrs(codec, &llrs)?;

        let corrected_wire = WirePayload { bytes: info_bytes };
        let corrected_wire =
            self.route_wire_stage(PipelineStage::DemodulateDecode, corrected_wire)?;

        let frame = self.stage_decode_frame(&corrected_wire)?;
        let frame = self.route_decoded_stage(PipelineStage::HpxStateUpdate, frame)?;
        info!("LDPC receive: frame seq={}", frame.sequence);
        let _ = self.event_tx.send(EngineEvent::FrameReceived {
            mode: mode.to_string(),
            bytes: frame.payload.len(),
        });
        Ok(frame.payload)
    }

    /// Encode `data` with rate-1/3 PCCC turbo FEC and transmit.
    ///
    /// TX chain: frame encode → turbo encode → modulate → emit.
    pub fn transmit_with_turbo(
        &mut self,
        data: &[u8],
        mode: &str,
        device: Option<&str>,
    ) -> Result<(), ModemError> {
        self.csma_check()?;

        let frame_wire = self.stage_encode_frame(data)?;
        if frame_wire.bytes.len() > TURBO_MAX_INFO_BYTES {
            return Err(ModemError::Frame(format!(
                "turbo: encoded frame {} B exceeds one-block limit of {} B; split payload at call site",
                frame_wire.bytes.len(),
                TURBO_MAX_INFO_BYTES,
            )));
        }
        let codeword = turbo_encode(&frame_wire.bytes)?;
        let fec_wire = WirePayload { bytes: codeword };
        let fec_wire = self.route_wire_stage(PipelineStage::EncodeModulate, fec_wire)?;

        debug!(
            "Turbo transmitting {} B codeword (mode={mode})",
            fec_wire.bytes.len()
        );

        let samples = {
            let plugin = self
                .plugins
                .get(mode)
                .ok_or_else(|| ModemError::PluginNotFound(mode.to_string()))?;
            self.stage_modulate_payload(plugin, mode, &fec_wire)?
        };
        let samples = self.route_audio_stage(PipelineStage::OutputEmit, samples)?;
        self.stage_emit_output(device, mode, &samples)?;
        let _ = self.event_tx.send(EngineEvent::FrameTransmitted {
            mode: mode.to_string(),
            bytes: fec_wire.bytes.len(),
        });
        Ok(())
    }

    /// Receive with rate-1/3 PCCC turbo FEC (Max-Log-MAP BCJR, 8 iterations).
    ///
    /// RX chain: capture → demodulate_soft → turbo decode → frame decode.
    pub fn receive_with_turbo(
        &mut self,
        mode: &str,
        device: Option<&str>,
    ) -> Result<Vec<u8>, ModemError> {
        let samples = self.stage_capture_input(Some(mode), device)?;
        let samples = self.route_audio_stage(PipelineStage::InputCapture, samples)?;

        let prev_busy = self.dcd.is_busy();
        self.dcd.update(&samples.samples);
        if self.dcd.is_busy() != prev_busy {
            let _ = self.event_tx.send(EngineEvent::DcdChange {
                busy: self.dcd.is_busy(),
                energy: self.dcd.energy(),
            });
        }

        let llrs = {
            let plugin = self
                .plugins
                .get(mode)
                .ok_or_else(|| ModemError::PluginNotFound(mode.to_string()))?;
            let mod_cfg = ModulationConfig {
                mode: mode.to_string(),
                center_frequency: self.center_frequency + self.afc_correction_hz,
                ..ModulationConfig::default()
            };
            plugin.demodulate_soft(&samples.samples, &mod_cfg)?
        };

        self.update_afc_estimate(mode, &samples.samples);
        if let Some(hz) = self.last_afc_offset_hz {
            let _ = self.event_tx.send(EngineEvent::AfcUpdate {
                offset_hz: hz,
                correction_hz: self.afc_correction_hz,
                mode: mode.to_string(),
            });
        }

        // Timing recovery can yield ±1–2 fewer symbols than transmitted; pad to the
        // next multiple of 3 so turbo_decode_soft's divisibility check always passes.
        // Padded LLRs are 0.0 (maximum uncertainty), which the BCJR decoder handles
        // gracefully — they correspond to the padding bits the encoder added to reach
        // the QPP block size.
        let llrs = if llrs.len() % 3 == 0 {
            llrs
        } else {
            let pad = 3 - (llrs.len() % 3);
            let mut v = llrs;
            v.extend(std::iter::repeat_n(0.0f32, pad));
            v
        };
        let info_bytes = turbo_decode_soft(&llrs)?;

        let corrected_wire = WirePayload { bytes: info_bytes };
        let corrected_wire =
            self.route_wire_stage(PipelineStage::DemodulateDecode, corrected_wire)?;

        let frame = self.stage_decode_frame(&corrected_wire)?;
        let frame = self.route_decoded_stage(PipelineStage::HpxStateUpdate, frame)?;
        info!("Turbo receive: frame seq={}", frame.sequence);
        let _ = self.event_tx.send(EngineEvent::FrameReceived {
            mode: mode.to_string(),
            bytes: frame.payload.len(),
        });
        Ok(frame.payload)
    }

    /// Receive via Memory-ARQ soft combining: capture `n_frames` sample buffers,
    /// average them element-wise, then demodulate and RS-decode the combined signal.
    ///
    /// Combining N identical retransmissions improves effective SNR by ~3 dB per
    /// doubling of N (10 log₁₀ N dB total gain over a single reception).
    ///
    /// Decodes using the standard RS codec (t=16).  For frames transmitted with
    /// [`transmit_with_strong_fec`](Self::transmit_with_strong_fec) use
    /// [`receive_with_strong_fec`](Self::receive_with_strong_fec) instead.
    pub fn receive_with_soft_combining(
        &mut self,
        mode: &str,
        device: Option<&str>,
        n_frames: usize,
    ) -> Result<Vec<u8>, ModemError> {
        if n_frames == 0 {
            return Err(ModemError::Frame(
                "soft combining: n_frames must be ≥ 1".to_string(),
            ));
        }
        let mut combiner = SoftCombiner::new();
        for _ in 0..n_frames {
            let samples = self.stage_capture_input(Some(mode), device)?;
            let samples = self.route_audio_stage(PipelineStage::InputCapture, samples)?;

            let prev_busy = self.dcd.is_busy();
            self.dcd.update(&samples.samples);
            if self.dcd.is_busy() != prev_busy {
                let _ = self.event_tx.send(EngineEvent::DcdChange {
                    busy: self.dcd.is_busy(),
                    energy: self.dcd.energy(),
                });
            }

            combiner.push(&samples.samples);
        }

        let combined = AudioSamples {
            samples: combiner.combine(),
        };

        self.update_afc_estimate(mode, &combined.samples);
        if let Some(hz) = self.last_afc_offset_hz {
            let _ = self.event_tx.send(EngineEvent::AfcUpdate {
                offset_hz: hz,
                correction_hz: self.afc_correction_hz,
                mode: mode.to_string(),
            });
        }

        let raw_wire = {
            let plugin = self
                .plugins
                .get(mode)
                .ok_or_else(|| ModemError::PluginNotFound(mode.to_string()))?;
            self.stage_demodulate_payload(plugin, mode, &combined)?
        };
        let raw_wire = self.route_wire_stage(PipelineStage::DemodulateDecode, raw_wire)?;

        let rs_decoded = FecCodec::new().decode(&raw_wire.bytes)?;
        let frame = self.stage_decode_frame(&WirePayload { bytes: rs_decoded })?;
        let frame = self.route_decoded_stage(PipelineStage::HpxStateUpdate, frame)?;
        let _ = self.event_tx.send(EngineEvent::FrameReceived {
            mode: mode.to_string(),
            bytes: frame.payload.len(),
        });
        Ok(frame.payload)
    }

    /// Receive via SNR-weighted LLR combining: demodulate each attempt separately,
    /// weight the resulting soft LLRs by inverse-noise-variance, combine, then
    /// RS decode.
    ///
    /// Each attempt yields a LLR vector from `plugin.demodulate_soft`; the attempts are combined by
    /// [`combine_llrs_map`] — their plain sum, which is the exact MAP combine for repeated
    /// observations of the same bits. Hard decisions are taken from the combined LLRs before RS decode.
    ///
    /// A calibrated demodulator's LLRs already carry `1/σ²`, so the sum *is* inverse-noise weighting;
    /// a good attempt dominates a faded one on the strength of its own LLR magnitudes. This used to
    /// re-weight the sum by a `1 / mean(|LLR|)` "noise-variance proxy", which applied `σ⁻²` a second
    /// time and threw away information from the weaker attempts (measured: 0.75 dB of threshold on a
    /// graded 0/−4/−8 dB attempt set).
    ///
    /// This provides ~2–4 dB improvement over equal-weight *sample* combining when
    /// different attempts experience different SNR (e.g., Watterson fading).
    ///
    /// TX chain: `transmit_with_fec` (RS-protected).  For Conv+RS frames use
    /// `receive_with_soft_viterbi_fec` on the combined samples instead.
    pub fn receive_with_llr_combining(
        &mut self,
        mode: &str,
        device: Option<&str>,
        n_frames: usize,
    ) -> Result<Vec<u8>, ModemError> {
        if n_frames == 0 {
            return Err(ModemError::Frame(
                "llr combining: n_frames must be ≥ 1".to_string(),
            ));
        }

        let mod_cfg = ModulationConfig {
            mode: mode.to_string(),
            center_frequency: self.center_frequency + self.afc_correction_hz,
            ..ModulationConfig::default()
        };

        let mut attempts: Vec<Vec<f32>> = Vec::with_capacity(n_frames);

        for i in 0..n_frames {
            let samples = self.stage_capture_input(Some(mode), device)?;
            let samples = self.route_audio_stage(PipelineStage::InputCapture, samples)?;

            let prev_busy = self.dcd.is_busy();
            self.dcd.update(&samples.samples);
            if self.dcd.is_busy() != prev_busy {
                let _ = self.event_tx.send(EngineEvent::DcdChange {
                    busy: self.dcd.is_busy(),
                    energy: self.dcd.energy(),
                });
            }

            // Update AFC from the first captured frame; no extra clone needed.
            if i == 0 {
                self.update_afc_estimate(mode, &samples.samples);
                if let Some(hz) = self.last_afc_offset_hz {
                    let _ = self.event_tx.send(EngineEvent::AfcUpdate {
                        offset_hz: hz,
                        correction_hz: self.afc_correction_hz,
                        mode: mode.to_string(),
                    });
                }
            }

            let llrs = {
                let plugin = self
                    .plugins
                    .get(mode)
                    .ok_or_else(|| ModemError::PluginNotFound(mode.to_string()))?;
                plugin.demodulate_soft(&samples.samples, &mod_cfg)?
            };

            attempts.push(llrs);
        }

        let attempt_refs: Vec<&[f32]> = attempts.iter().map(|l| l.as_slice()).collect();
        let combined_llrs = combine_llrs_map(&attempt_refs);

        // Hard-decision bytes from combined LLRs: negative LLR → bit 1, positive → bit 0.
        // Pack bit-pairs in the same order the plugin's `demodulate_soft` emits LLRs.
        let hard_bytes: Vec<u8> = combined_llrs
            .chunks(8)
            .map(|chunk| {
                chunk.iter().enumerate().fold(0u8, |acc, (i, &llr)| {
                    acc | ((llr.is_sign_negative() as u8) << i)
                })
            })
            .collect();

        let hard_wire = WirePayload { bytes: hard_bytes };
        let hard_wire = self.route_wire_stage(PipelineStage::DemodulateDecode, hard_wire)?;

        let rs_decoded = FecCodec::new().decode(&hard_wire.bytes)?;

        let frame = self.stage_decode_frame(&WirePayload { bytes: rs_decoded })?;
        let frame = self.route_decoded_stage(PipelineStage::HpxStateUpdate, frame)?;
        let _ = self.event_tx.send(EngineEvent::FrameReceived {
            mode: mode.to_string(),
            bytes: frame.payload.len(),
        });
        Ok(frame.payload)
    }

    /// Receive via Window-ARQ range-limited MAP LLR combining.
    ///
    /// Captures `n_frames` receive attempts, combines soft LLRs only inside
    /// `feedback.ranges` via [`combine_llrs_map_in_ranges`], then takes hard decisions and RS-decodes
    /// the combined protected frame. Outside selected ranges, the first attempt is preserved.
    ///
    /// This path is mode-agnostic and works for any registered plugin that
    /// implements `demodulate_soft`.
    pub fn receive_with_window_arq(
        &mut self,
        mode: &str,
        device: Option<&str>,
        n_frames: usize,
        feedback: &WindowArqFeedback,
    ) -> Result<Vec<u8>, ModemError> {
        if n_frames == 0 {
            return Err(ModemError::Frame(
                "window-arq combining: n_frames must be >= 1".to_string(),
            ));
        }

        let mod_cfg = ModulationConfig {
            mode: mode.to_string(),
            center_frequency: self.center_frequency + self.afc_correction_hz,
            ..ModulationConfig::default()
        };

        let mut attempts: Vec<Vec<f32>> = Vec::with_capacity(n_frames);

        for i in 0..n_frames {
            let samples = self.stage_capture_input(Some(mode), device)?;
            let samples = self.route_audio_stage(PipelineStage::InputCapture, samples)?;

            let prev_busy = self.dcd.is_busy();
            self.dcd.update(&samples.samples);
            if self.dcd.is_busy() != prev_busy {
                let _ = self.event_tx.send(EngineEvent::DcdChange {
                    busy: self.dcd.is_busy(),
                    energy: self.dcd.energy(),
                });
            }

            if i == 0 {
                self.update_afc_estimate(mode, &samples.samples);
                if let Some(hz) = self.last_afc_offset_hz {
                    let _ = self.event_tx.send(EngineEvent::AfcUpdate {
                        offset_hz: hz,
                        correction_hz: self.afc_correction_hz,
                        mode: mode.to_string(),
                    });
                }
            }

            let llrs = {
                let plugin = self
                    .plugins
                    .get(mode)
                    .ok_or_else(|| ModemError::PluginNotFound(mode.to_string()))?;
                plugin.demodulate_soft(&samples.samples, &mod_cfg)?
            };

            attempts.push(llrs);
        }

        let attempt_refs: Vec<&[f32]> = attempts.iter().map(|l| l.as_slice()).collect();
        let combined_llrs = combine_llrs_map_in_ranges(&attempt_refs, feedback);

        let hard_bytes: Vec<u8> = combined_llrs
            .chunks(8)
            .map(|chunk| {
                chunk.iter().enumerate().fold(0u8, |acc, (i, &llr)| {
                    acc | ((llr.is_sign_negative() as u8) << i)
                })
            })
            .collect();

        // OFDM/SC-FDMA pad the last symbol to a whole subcarrier boundary; the
        // resulting hard_bytes may be a few bytes longer than an exact RS multiple.
        // Trim to the nearest multiple of 255 (RS BLOCK_TOTAL) so FecCodec::decode
        // doesn't reject the buffer.
        const RS_BLOCK: usize = 255;
        let rs_len = (hard_bytes.len() / RS_BLOCK) * RS_BLOCK;
        let mut hard_bytes = hard_bytes;
        if rs_len > 0 && rs_len < hard_bytes.len() {
            hard_bytes.truncate(rs_len);
        }
        let hard_wire = WirePayload { bytes: hard_bytes };
        let hard_wire = self.route_wire_stage(PipelineStage::DemodulateDecode, hard_wire)?;

        let rs_decoded = FecCodec::new().decode(&hard_wire.bytes)?;
        let frame = self.stage_decode_frame(&WirePayload { bytes: rs_decoded })?;
        let frame = self.route_decoded_stage(PipelineStage::HpxStateUpdate, frame)?;
        let _ = self.event_tx.send(EngineEvent::FrameReceived {
            mode: mode.to_string(),
            bytes: frame.payload.len(),
        });
        Ok(frame.payload)
    }

    /// Build and transmit a selective Window-ARQ retransmit packet.
    ///
    /// The sender provides the original RS-protected frame bytes and the
    /// receiver-provided `feedback` failed ranges. Only failed byte windows are
    /// emitted, reducing retry airtime compared to full-frame retransmit.
    ///
    /// Returns the encoded retransmit packet bytes that were emitted.
    pub fn transmit_window_retransmit_packet(
        &mut self,
        protected_frame: &[u8],
        feedback: &WindowArqFeedback,
        mode: &str,
        device: Option<&str>,
    ) -> Result<Vec<u8>, ModemError> {
        self.csma_check()?;

        let packet = encode_window_retransmit(protected_frame, feedback)?;
        let wire = WirePayload {
            bytes: packet.clone(),
        };
        let wire = self.route_wire_stage(PipelineStage::EncodeModulate, wire)?;

        let samples = {
            let plugin = self
                .plugins
                .get(mode)
                .ok_or_else(|| ModemError::PluginNotFound(mode.to_string()))?;
            self.stage_modulate_payload(plugin, mode, &wire)?
        };
        let samples = self.route_audio_stage(PipelineStage::OutputEmit, samples)?;
        self.stage_emit_output(device, mode, &samples)?;

        let _ = self.event_tx.send(EngineEvent::FrameTransmitted {
            mode: mode.to_string(),
            bytes: wire.bytes.len(),
        });
        Ok(packet)
    }

    /// Receive one selective Window-ARQ retransmit packet for `mode`.
    ///
    /// This method demodulates raw retransmit bytes and does not attempt frame
    /// decode. The returned packet is consumed by
    /// [`receive_with_window_arq_selective`](Self::receive_with_window_arq_selective)
    /// or call-site patch logic.
    pub fn receive_window_retransmit_packet(
        &mut self,
        mode: &str,
        device: Option<&str>,
    ) -> Result<Vec<u8>, ModemError> {
        let samples = self.stage_capture_input(Some(mode), device)?;
        let samples = self.route_audio_stage(PipelineStage::InputCapture, samples)?;

        let prev_busy = self.dcd.is_busy();
        self.dcd.update(&samples.samples);
        if self.dcd.is_busy() != prev_busy {
            let _ = self.event_tx.send(EngineEvent::DcdChange {
                busy: self.dcd.is_busy(),
                energy: self.dcd.energy(),
            });
        }

        let wire = {
            let plugin = self
                .plugins
                .get(mode)
                .ok_or_else(|| ModemError::PluginNotFound(mode.to_string()))?;
            self.stage_demodulate_payload(plugin, mode, &samples)?
        };
        let wire = self.route_wire_stage(PipelineStage::DemodulateDecode, wire)?;

        self.update_afc_estimate(mode, &samples.samples);
        if let Some(hz) = self.last_afc_offset_hz {
            let _ = self.event_tx.send(EngineEvent::AfcUpdate {
                offset_hz: hz,
                correction_hz: self.afc_correction_hz,
                mode: mode.to_string(),
            });
        }

        Ok(wire.bytes)
    }

    /// Full selective Window-ARQ receive path.
    ///
    /// Applies `n_packets` retransmit packets to `protected_frame` using
    /// `apply_window_retransmit`, then RS-decodes and frame-decodes the repaired
    /// buffer.
    pub fn receive_with_window_arq_selective(
        &mut self,
        mode: &str,
        device: Option<&str>,
        protected_frame: &mut [u8],
        n_packets: usize,
    ) -> Result<Vec<u8>, ModemError> {
        if n_packets == 0 {
            return Err(ModemError::Frame(
                "window-arq selective: n_packets must be >= 1".to_string(),
            ));
        }

        for _ in 0..n_packets {
            let packet = self.receive_window_retransmit_packet(mode, device)?;
            apply_window_retransmit(protected_frame, &packet)?;
        }

        let rs_decoded = FecCodec::new().decode(protected_frame)?;
        let frame = self.stage_decode_frame(&WirePayload { bytes: rs_decoded })?;
        let frame = self.route_decoded_stage(PipelineStage::HpxStateUpdate, frame)?;
        let _ = self.event_tx.send(EngineEvent::FrameReceived {
            mode: mode.to_string(),
            bytes: frame.payload.len(),
        });
        Ok(frame.payload)
    }

    /// Transmit one HARQ attempt selected from SNR/fading state.
    ///
    /// Returns the [`HarqDecision`] that was applied for this attempt.
    pub fn transmit_with_harq_attempt(
        &mut self,
        data: &[u8],
        mode: &str,
        snr_db: f32,
        fading_depth_db: f32,
        retry_index: u8,
        device: Option<&str>,
    ) -> Result<HarqDecision, ModemError> {
        let decision =
            self.select_harq_decision_for_mode(mode, snr_db, fading_depth_db, retry_index);
        self.transmit_with_fec_mode(data, mode, decision.fec_mode, device)?;
        Ok(decision)
    }

    /// Receive one HARQ attempt selected from SNR/fading state.
    ///
    /// Returns `(payload, decision)` where `decision` is the FEC/timeout policy
    /// that was applied to decode this attempt.
    pub fn receive_with_harq_attempt(
        &mut self,
        mode: &str,
        snr_db: f32,
        fading_depth_db: f32,
        retry_index: u8,
        device: Option<&str>,
    ) -> Result<(Vec<u8>, HarqDecision), ModemError> {
        let decision =
            self.select_harq_decision_for_mode(mode, snr_db, fading_depth_db, retry_index);
        let payload = self.receive_with_fec_mode(mode, decision.fec_mode, device)?;
        Ok((payload, decision))
    }

    /// Transmit with the codec selected by `fec`.
    ///
    /// This is the single-call dispatch over every `FecMode` variant so callers
    /// can drive FEC selection from the negotiated `FecMode` without a match
    /// statement at every call site.
    ///
    /// `FecMode::None` maps to plain [`transmit`](Self::transmit).
    /// `FecMode::RsInterleaved` and `FecMode::Concatenated` use
    /// [`DEFAULT_INTERLEAVER_DEPTH`].
    /// `FecMode::Ldpc` calls [`transmit_with_ldpc`](Self::transmit_with_ldpc) and
    /// is subject to the same single-block limit.
    /// `FecMode::ShortRs` is supported for both ACK frames (5-byte fixed) and
    /// data frames (≤ 223 bytes). Data frames are dispatched to
    /// [`transmit_with_short_fec_data`](Self::transmit_with_short_fec_data);
    /// ACK frames should call
    /// [`transmit_ack_with_short_fec`](Self::transmit_ack_with_short_fec) directly.
    pub fn transmit_with_fec_mode(
        &mut self,
        data: &[u8],
        mode: &str,
        fec: FecMode,
        device: Option<&str>,
    ) -> Result<(), ModemError> {
        match fec {
            FecMode::None => self.transmit(data, mode, device),
            FecMode::Rs => self.transmit_with_fec(data, mode, device),
            FecMode::RsInterleaved => {
                self.transmit_with_fec_interleaved(data, mode, device, DEFAULT_INTERLEAVER_DEPTH)
            }
            FecMode::Concatenated => self.transmit_with_concatenated_fec(data, mode, device),
            FecMode::ShortRs => self.transmit_with_short_fec_data(data, mode, device),
            FecMode::RsStrong => self.transmit_with_strong_fec(data, mode, device),
            FecMode::SoftConcatenated => self.transmit_with_soft_viterbi_fec(data, mode, device),
            FecMode::Ldpc => self.transmit_with_ldpc(data, mode, device),
            FecMode::LdpcHighRate => self.transmit_with_ldpc_high_rate(data, mode, device),
            FecMode::Turbo => self.transmit_with_turbo(data, mode, device),
        }
    }

    /// Receive with the codec selected by `fec`.
    ///
    /// Mirror of [`transmit_with_fec_mode`](Self::transmit_with_fec_mode).
    /// `FecMode::ShortRs` dispatches to
    /// [`receive_with_short_fec_data`](Self::receive_with_short_fec_data); for
    /// ACK frames call
    /// [`receive_ack_with_short_fec`](Self::receive_ack_with_short_fec) directly.
    pub fn receive_with_fec_mode(
        &mut self,
        mode: &str,
        fec: FecMode,
        device: Option<&str>,
    ) -> Result<Vec<u8>, ModemError> {
        // Warn when a soft-input FEC mode is paired with a plugin that only
        // produces hard-decision ±1.0 LLRs — the decoder gains nothing.
        let is_soft_fec = matches!(
            fec,
            FecMode::SoftConcatenated | FecMode::Ldpc | FecMode::LdpcHighRate | FecMode::Turbo
        );
        if is_soft_fec {
            if let Some(plugin) = self.plugins.get(mode) {
                if !plugin.supports_soft_demod() {
                    tracing::warn!(
                        mode,
                        fec = ?fec,
                        "soft-FEC mode paired with a plugin that provides only hard-decision LLRs; \
                         iteration gain will be zero — consider a plugin that overrides supports_soft_demod()"
                    );
                }
            }
        }
        match fec {
            FecMode::None => self.receive(mode, device),
            FecMode::Rs => self.receive_with_fec(mode, device),
            FecMode::RsInterleaved => {
                self.receive_with_fec_interleaved(mode, device, DEFAULT_INTERLEAVER_DEPTH)
            }
            FecMode::Concatenated => self.receive_with_concatenated_fec(mode, device),
            FecMode::ShortRs => self.receive_with_short_fec_data(mode, device),
            FecMode::RsStrong => self.receive_with_strong_fec(mode, device),
            FecMode::SoftConcatenated => self.receive_with_soft_viterbi_fec(mode, device),
            FecMode::Ldpc => self.receive_with_ldpc(mode, device),
            FecMode::LdpcHighRate => self.receive_with_ldpc_high_rate(mode, device),
            FecMode::Turbo => self.receive_with_turbo(mode, device),
        }
    }

    /// Encode `ack` with ShortFecCodec (5 → 13 bytes) and emit via FSK4-ACK.
    pub fn transmit_ack_with_short_fec(
        &mut self,
        ack: &AckFrame,
        device: Option<&str>,
    ) -> Result<(), ModemError> {
        self.csma_check()?;
        let raw = ack.encode();
        let fec_bytes = ShortFecCodec::new().encode(&raw)?;
        let wire = WirePayload { bytes: fec_bytes };
        let mode = "FSK4-ACK";
        let samples = {
            let plugin = self
                .plugins
                .get(mode)
                .ok_or_else(|| ModemError::PluginNotFound(mode.to_string()))?;
            self.stage_modulate_payload(plugin, mode, &wire)?
        };
        let samples = self.route_audio_stage(PipelineStage::OutputEmit, samples)?;
        self.stage_emit_output(device, mode, &samples)
    }

    /// Demodulate FSK4-ACK, ShortFecCodec decode (13 → 5 bytes), return `AckFrame`.
    /// Receive an FSK4 short-FEC ACK, re-capturing until it decodes or `timeout_ms`
    /// elapses. `0` falls back to a single immediate read
    /// ([`receive_ack_with_short_fec`](Self::receive_ack_with_short_fec)).
    pub fn receive_ack_with_short_fec_within(
        &mut self,
        device: Option<&str>,
        timeout_ms: u64,
    ) -> Result<AckFrame, ModemError> {
        if timeout_ms == 0 {
            return self.receive_ack_with_short_fec(device);
        }
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        loop {
            match self.receive_ack_with_short_fec(device) {
                Ok(ack) => return Ok(ack),
                Err(e) => {
                    if Instant::now() >= deadline {
                        return Err(e);
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(30));
        }
    }

    pub fn receive_ack_with_short_fec(
        &mut self,
        device: Option<&str>,
    ) -> Result<AckFrame, ModemError> {
        let samples = self.stage_capture_input(None, device)?;
        let samples = self.route_audio_stage(PipelineStage::InputCapture, samples)?;

        let prev_busy = self.dcd.is_busy();
        self.dcd.update(&samples.samples);
        if self.dcd.is_busy() != prev_busy {
            let _ = self.event_tx.send(EngineEvent::DcdChange {
                busy: self.dcd.is_busy(),
                energy: self.dcd.energy(),
            });
        }

        let mode = "FSK4-ACK";
        let wire = {
            let plugin = self
                .plugins
                .get(mode)
                .ok_or_else(|| ModemError::PluginNotFound(mode.to_string()))?;
            self.stage_demodulate_payload(plugin, mode, &samples)?
        };

        self.update_afc_estimate(mode, &samples.samples);
        if let Some(hz) = self.last_afc_offset_hz {
            let _ = self.event_tx.send(EngineEvent::AfcUpdate {
                offset_hz: hz,
                correction_hz: self.afc_correction_hz,
                mode: mode.to_string(),
            });
        }

        let decoded = ShortFecCodec::new().decode(&wire.bytes)?;
        let n = decoded.len();
        let arr: [u8; 5] = decoded.try_into().map_err(|_| {
            ModemError::Frame(format!("ShortFEC ACK decode: expected 5 bytes, got {n}"))
        })?;
        AckFrame::decode(&arr).map_err(|e| ModemError::Frame(format!("AckFrame decode: {e:?}")))
    }

    /// ECC bytes appended by the ShortRs data-frame codec (t = 16).
    const SHORT_FEC_DATA_ECC_LEN: usize = 32;

    /// Frame envelope (magic + ver + seq + len + CRC) bytes added by
    /// [`stage_encode_frame`]. Mirrors `openpulse_core::frame::Frame::encode`.
    const FRAME_ENVELOPE_LEN: usize = 4 + 1 + 2 + 1 + 2;

    /// Maximum user payload accepted by [`transmit_with_short_fec_data`].
    ///
    /// The on-air buffer is `Frame(payload) + 32 B ECC`, which must fit in
    /// `ShortFecCodec`'s 255-byte block, i.e.
    /// `FRAME_ENVELOPE_LEN + payload + ECC_LEN ≤ 255`.
    const SHORT_FEC_DATA_MAX_PAYLOAD: usize =
        255 - Self::SHORT_FEC_DATA_ECC_LEN - Self::FRAME_ENVELOPE_LEN;

    /// Transmit `payload` using the short-block RS codec.
    ///
    /// The bytes on the wire are `Frame(payload) + 32 B ECC` —
    /// `payload.len() + 42` bytes — instead of the full 255-byte block
    /// produced by [`transmit_with_fec`](Self::transmit_with_fec). Strength is
    /// t = 16 byte errors per frame.
    ///
    /// Maximum payload is
    /// [`SHORT_FEC_DATA_MAX_PAYLOAD`](Self::SHORT_FEC_DATA_MAX_PAYLOAD)
    /// (213 bytes); larger payloads return `ModemError::Frame`.
    ///
    /// The receiver determines the data length from the demodulated byte count,
    /// so this path only round-trips reliably when the modulation plugin emits
    /// the exact number of bytes corresponding to the transmitted frame
    /// (loopback and well-framed half-duplex paths). Paths that pad to a
    /// subcarrier boundary (OFDM/SC-FDMA) are not supported.
    pub fn transmit_with_short_fec_data(
        &mut self,
        payload: &[u8],
        mode: &str,
        device: Option<&str>,
    ) -> Result<(), ModemError> {
        if payload.len() > Self::SHORT_FEC_DATA_MAX_PAYLOAD {
            return Err(ModemError::Frame(format!(
                "ShortRs data frame: payload {} bytes exceeds maximum {}",
                payload.len(),
                Self::SHORT_FEC_DATA_MAX_PAYLOAD
            )));
        }
        self.csma_check()?;

        let frame_wire = self.stage_encode_frame(payload)?;
        let fec_bytes =
            ShortFecCodec::with_ecc_len(Self::SHORT_FEC_DATA_ECC_LEN).encode(&frame_wire.bytes)?;
        let wire = WirePayload { bytes: fec_bytes };
        let wire = self.route_wire_stage(PipelineStage::EncodeModulate, wire)?;

        let samples = {
            let plugin = self
                .plugins
                .get(mode)
                .ok_or_else(|| ModemError::PluginNotFound(mode.to_string()))?;
            self.stage_modulate_payload(plugin, mode, &wire)?
        };
        let samples = self.route_audio_stage(PipelineStage::OutputEmit, samples)?;
        self.stage_emit_output(device, mode, &samples)?;
        let _ = self.event_tx.send(EngineEvent::FrameTransmitted {
            mode: mode.to_string(),
            bytes: wire.bytes.len(),
        });
        Ok(())
    }

    /// Demodulate and decode a frame emitted by
    /// [`transmit_with_short_fec_data`](Self::transmit_with_short_fec_data).
    pub fn receive_with_short_fec_data(
        &mut self,
        mode: &str,
        device: Option<&str>,
    ) -> Result<Vec<u8>, ModemError> {
        let samples = self.stage_capture_input(Some(mode), device)?;
        let samples = self.route_audio_stage(PipelineStage::InputCapture, samples)?;

        let prev_busy = self.dcd.is_busy();
        self.dcd.update(&samples.samples);
        if self.dcd.is_busy() != prev_busy {
            let _ = self.event_tx.send(EngineEvent::DcdChange {
                busy: self.dcd.is_busy(),
                energy: self.dcd.energy(),
            });
        }

        let wire = {
            let plugin = self
                .plugins
                .get(mode)
                .ok_or_else(|| ModemError::PluginNotFound(mode.to_string()))?;
            self.stage_demodulate_payload(plugin, mode, &samples)?
        };
        let wire = self.route_wire_stage(PipelineStage::DemodulateDecode, wire)?;

        self.update_afc_estimate(mode, &samples.samples);
        if let Some(hz) = self.last_afc_offset_hz {
            let _ = self.event_tx.send(EngineEvent::AfcUpdate {
                offset_hz: hz,
                correction_hz: self.afc_correction_hz,
                mode: mode.to_string(),
            });
        }

        let corrected_bytes =
            ShortFecCodec::with_ecc_len(Self::SHORT_FEC_DATA_ECC_LEN).decode(&wire.bytes)?;
        let corrected_wire = WirePayload {
            bytes: corrected_bytes,
        };
        let frame = self.stage_decode_frame(&corrected_wire)?;
        let frame = self.route_decoded_stage(PipelineStage::HpxStateUpdate, frame)?;
        let _ = self.event_tx.send(EngineEvent::FrameReceived {
            mode: mode.to_string(),
            bytes: frame.payload.len(),
        });
        Ok(frame.payload)
    }

    fn stage_encode_frame(&mut self, data: &[u8]) -> Result<WirePayload, ModemError> {
        let _stage = PipelineStage::EncodeModulate;
        let frame = Frame::new(self.sequence, data.to_vec())
            .map_err(|e| ModemError::Frame(e.to_string()))?;
        self.sequence = self.sequence.wrapping_add(1);
        Ok(WirePayload {
            bytes: frame.encode(),
        })
    }

    fn stage_modulate_payload(
        &self,
        plugin: &dyn openpulse_core::plugin::ModulationPlugin,
        mode: &str,
        wire: &WirePayload,
    ) -> Result<AudioSamples, ModemError> {
        let _stage = PipelineStage::EncodeModulate;
        let mod_cfg = ModulationConfig {
            mode: mode.to_string(),
            center_frequency: self.center_frequency,
            ..ModulationConfig::default()
        };
        let samples = plugin.modulate(&wire.bytes, &mod_cfg)?;
        Ok(AudioSamples { samples })
    }

    fn stage_emit_output(
        &mut self,
        device: Option<&str>,
        mode: &str,
        samples: &AudioSamples,
    ) -> Result<(), ModemError> {
        let _stage = PipelineStage::OutputEmit;

        let audio_cfg = AudioConfig::default();
        let mut stream = self
            .audio
            .open_output(device.or(self.default_device.as_deref()), &audio_cfg)
            .map_err(|e| ModemError::Audio(e.to_string()))?;

        let atten_linear = 10.0f32.powf(self.tx_attenuation_db / 20.0);
        let mut write_samples: Vec<f32> = if (atten_linear - 1.0).abs() < 1e-6 {
            samples.samples.clone()
        } else {
            samples.samples.iter().map(|s| s * atten_linear).collect()
        };
        // CE-SSB envelope conditioning: only for high-PAPR modes that benefit
        // (multicarrier), and only when enabled. Raises average power at the same
        // peak; a no-op for single-carrier modes. See `cessb_condition_tx`.
        if self.cessb_enabled && Self::cessb_benefits(mode) {
            write_samples = self.cessb_condition_tx(&write_samples);
        }
        let threshold = self.tx_limiter_threshold;
        if threshold > 0.0 {
            tanh_limit(&mut write_samples, threshold);
        }

        self.record_audio(&write_samples); // TX window for the spectrum/waterfall tap
        stream
            .write(&write_samples)
            .map_err(|e| ModemError::Audio(e.to_string()))?;
        stream
            .flush()
            .map_err(|e| ModemError::Audio(e.to_string()))?;

        // Regulatory compliance log lives at this single emit seam, so EVERY transmitted frame
        // (data, FEC, ACK, retransmit, QSY, …) is recorded — not just the plain `transmit()` path.
        self.update_tx_session_callsign();
        let tx_seq = self.sequence.wrapping_sub(1);
        let metadata = TxMetadata::new(&self.callsign, mode, self.max_power_watts, tx_seq);
        self.tx_session_log
            .log_frame(metadata.clone())
            .map_err(|err| ModemError::Configuration(err.to_string()))?;
        debug!("logged TX metadata: {}", metadata.to_log_line());
        self.frames_transmitted = self.frames_transmitted.wrapping_add(1);

        Ok(())
    }

    fn stage_capture_input(
        &mut self,
        mode: Option<&str>,
        device: Option<&str>,
    ) -> Result<AudioSamples, ModemError> {
        let _stage = PipelineStage::InputCapture;
        let audio_cfg = AudioConfig::default();
        let mut stream = self
            .audio
            .open_input(device.or(self.default_device.as_deref()), &audio_cfg)
            .map_err(|e| ModemError::Audio(e.to_string()))?;

        let samples = stream
            .read()
            .map_err(|e| ModemError::Audio(e.to_string()))?;
        self.record_audio(&samples); // RX window (raw channel audio) for the spectrum/waterfall tap
                                     // Record the mode for the receiver front end; the notch is applied once, at the single
                                     // `PipelineStage::InputCapture` seam in `route_audio_stage`, which every receive path hits.
        self.rx_mode = mode.map(|m| m.to_string());
        Ok(AudioSamples { samples })
    }

    /// Fast AFC settle over one acquisition window: a one-shot wide-scan
    /// anchor pass (`afc_step = 1.0` sets the correction directly to the
    /// Goertzel peak — iterative passes diverge for carriers at the scan
    /// boundary) followed by 5 fine-tracking passes at `afc_step = 0.7`.
    ///
    /// Saves and restores `afc_step` internally; `afc_correction_hz` is left
    /// at the fine estimate so the caller can accept it or restore its own
    /// saved value.  This is the ONLY place that temporarily mutates the AFC
    /// state for settling — the previous inline copies of this sequence each
    /// hand-rolled the save/restore and had already caused >1000 Hz of
    /// accumulated drift once (review E5).
    fn afc_mini_settle(&mut self, mode: &str, window: &[f32]) -> AfcSettleOutcome {
        let saved_step = self.afc_step;
        self.afc_step = 1.0;
        self.afc_correction_hz = 0.0;
        self.update_afc_estimate(mode, window);
        let anchor = self.afc_correction_hz;
        self.afc_step = 0.7;
        let mut prev = anchor;
        for _ in 0..5 {
            prev = self.afc_correction_hz;
            self.update_afc_estimate(mode, window);
        }
        self.afc_step = saved_step;
        let last_delta = (self.afc_correction_hz - prev).abs();
        // Snap a sub-noise-floor correction to zero (see AFC_SETTLE_DEADBAND_HZ):
        // applying a spurious few-tenths-of-a-Hz correction over-corrects a
        // zero-offset frame and breaks 8PSK's preamble phase re-fit.
        if self.afc_correction_hz.abs() < AFC_SETTLE_DEADBAND_HZ {
            self.afc_correction_hz = 0.0;
        }
        AfcSettleOutcome {
            anchor,
            fine: self.afc_correction_hz,
            last_delta,
        }
    }

    fn update_afc_estimate(&mut self, mode: &str, samples: &[f32]) {
        let mod_cfg = ModulationConfig {
            mode: mode.to_string(),
            center_frequency: self.center_frequency + self.afc_correction_hz,
            afc_correction_hz: self.afc_correction_hz,
            ..ModulationConfig::default()
        };
        let estimate = self
            .plugins
            .get(mode)
            .and_then(|p| p.estimate_afc_hz(samples, &mod_cfg));
        self.last_afc_offset_hz = estimate;
        if self.afc_enabled {
            if let Some(offset) = estimate {
                self.afc_correction_hz += self.afc_step * offset;
                debug!(
                    "AFC: raw_estimate={:.1}Hz correction={:.1}Hz total_offset={:.1}Hz",
                    offset,
                    self.afc_correction_hz,
                    offset + self.afc_correction_hz
                );
            }
        }
    }

    fn stage_demodulate_payload(
        &self,
        plugin: &dyn openpulse_core::plugin::ModulationPlugin,
        mode: &str,
        samples: &AudioSamples,
    ) -> Result<WirePayload, ModemError> {
        let _stage = PipelineStage::DemodulateDecode;
        let mod_cfg = ModulationConfig {
            mode: mode.to_string(),
            center_frequency: self.center_frequency + self.afc_correction_hz,
            afc_correction_hz: self.afc_correction_hz,
            ..ModulationConfig::default()
        };
        let wire_bytes = plugin.demodulate(&samples.samples, &mod_cfg)?;
        Ok(WirePayload { bytes: wire_bytes })
    }

    fn stage_decode_frame(&self, wire: &WirePayload) -> Result<DecodedFrame, ModemError> {
        let _stage = PipelineStage::DemodulateDecode;
        let frame = Frame::decode(&wire.bytes)?;
        Ok(DecodedFrame {
            sequence: frame.sequence,
            payload: frame.payload,
        })
    }

    fn route_wire_stage(
        &mut self,
        stage: PipelineStage,
        payload: WirePayload,
    ) -> Result<WirePayload, ModemError> {
        self.scheduler
            .route_wire(stage, payload)
            .map_err(|e| ModemError::Configuration(e.to_string()))
    }

    fn route_audio_stage(
        &mut self,
        stage: PipelineStage,
        payload: AudioSamples,
    ) -> Result<AudioSamples, ModemError> {
        let routed = self
            .scheduler
            .route_audio(stage, payload)
            .map_err(|e| ModemError::Configuration(e.to_string()))?;
        // The receiver front end lives at this single seam: every capture path funnels its raw
        // samples through `route_audio_stage(InputCapture)` exactly once, so placing front-end
        // transforms here (rather than in any one capture entry function) covers them all by
        // construction. Order: notch (remove interference) → AGC (normalise the cleaned level).
        if stage == PipelineStage::InputCapture {
            let mut samples = routed.samples;
            // REQ-PHY-02: remove DC bias (SSB audio paths / soundcard offset) before demod.
            // Per-burst mean subtraction is a transient-free high-pass at ~1/burst Hz (≪10 Hz for
            // any real burst): the heterodyne PSK/QAM demods already reject a 0 Hz offset, but the
            // DCD/CSMA energy gate and AGC use mean-square/RMS, which a DC offset inflates — so this
            // de-biases those. A constant shift leaves all AC content (carrier band) bit-identical,
            // so it never perturbs acquisition; on a zero-DC signal (loopback) the mean is ~0 → no-op.
            self.dc_blocks_processed = self.dc_blocks_processed.wrapping_add(1);
            samples = apply_dc_block(samples);
            if self.notch_enabled {
                self.notch_blocks_processed = self.notch_blocks_processed.wrapping_add(1);
                let mode = self.rx_mode.clone();
                samples = self.apply_rx_notch(mode.as_deref(), samples);
            }
            if self.agc_enabled {
                self.agc_blocks_processed = self.agc_blocks_processed.wrapping_add(1);
                samples = self.apply_rx_agc(samples);
            }
            return Ok(AudioSamples { samples });
        }
        Ok(routed)
    }

    fn route_decoded_stage(
        &mut self,
        stage: PipelineStage,
        payload: DecodedFrame,
    ) -> Result<DecodedFrame, ModemError> {
        self.scheduler
            .route_decoded(stage, payload)
            .map_err(|e| ModemError::Configuration(e.to_string()))
    }
}

fn nonce_from_seq(seq: u16) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[..2].copy_from_slice(&seq.to_le_bytes());
    n
}

/// Decode one LDPC codeword from a soft-LLR stream, trimming to the codec's own
/// codeword length so both the rate-1/2 and high-rate (rate ≈8/9) presets share
/// one slice rule.
fn decode_ldpc_llrs(codec: &LdpcCodec, llrs: &[f32]) -> Result<Vec<u8>, ModemError> {
    let n_bits = codec.codeword_bytes() * 8;
    codec.decode_soft(&llrs[..n_bits.min(llrs.len())])
}

fn minimum_trust_for_profile(profile: PolicyProfile) -> ConnectionTrustLevel {
    match profile {
        PolicyProfile::Strict => ConnectionTrustLevel::Verified,
        PolicyProfile::Balanced => ConnectionTrustLevel::PskVerified,
        PolicyProfile::Permissive => ConnectionTrustLevel::Reduced,
    }
}

/// Remove the DC component of a captured burst by subtracting its mean (REQ-PHY-02).
///
/// This is a transient-free high-pass at ≈ `sample_rate / len` Hz — far below 10 Hz for any real
/// burst. A constant offset is the only thing removed, so the carrier-band (AC) content is
/// bit-identical and demodulation/acquisition is unaffected; on a near-zero-DC signal the mean is
/// ~0 and this is a no-op. Its value is de-biasing the mean-square energy the DCD/CSMA gate and the
/// AGC RMS compute, which a soundcard/SSB DC offset would otherwise inflate.
fn apply_dc_block(mut samples: Vec<f32>) -> Vec<f32> {
    let n = samples.len();
    if n == 0 {
        return samples;
    }
    let mean = samples.iter().sum::<f32>() / n as f32;
    if mean != 0.0 {
        for s in samples.iter_mut() {
            *s -= mean;
        }
    }
    samples
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use bpsk_plugin::BpskPlugin;
    use openpulse_audio::LoopbackBackend;

    fn make_engine() -> ModemEngine {
        let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
        engine
            .register_plugin(Box::new(BpskPlugin::new()))
            .expect("failed to register BPSK plugin");
        engine
    }

    #[test]
    fn transmit_then_receive() {
        let mut engine = make_engine();
        engine.transmit(b"Hello", "BPSK100", None).unwrap();
        let received = engine.receive("BPSK100", None).unwrap();
        assert_eq!(received, b"Hello");
    }

    #[test]
    fn capture_burst_accumulates_fragmented_frame_then_decodes() {
        // Simulate a streaming backend delivering one frame across several tick
        // windows: capture_burst must accumulate (returning None) until the carrier
        // drops, then flush the whole burst so decode_burst recovers the payload.
        let tx_lb = LoopbackBackend::new();
        let mut tx = ModemEngine::new(Box::new(tx_lb.clone_shared()));
        tx.register_plugin(Box::new(BpskPlugin::new())).unwrap();
        tx.transmit(b"burst capture", "BPSK250", None).unwrap();
        let frame = tx_lb.drain_samples();
        assert!(!frame.is_empty());

        let rx_lb = LoopbackBackend::new_split();
        let mut rx = ModemEngine::new(Box::new(rx_lb.clone_shared()));
        rx.register_plugin(Box::new(BpskPlugin::new())).unwrap();

        // Feed the frame in 4 fragments across 4 ticks — each must keep accumulating.
        let chunk = frame.len() / 4 + 1;
        for frag in frame.chunks(chunk) {
            rx_lb.fill_samples(frag);
            assert!(
                rx.capture_burst(None).unwrap().is_none(),
                "mid-burst tick must keep accumulating"
            );
        }
        // A quiet tick (no carrier) flushes the complete burst.
        let burst = rx
            .capture_burst(None)
            .unwrap()
            .expect("carrier drop must flush the accumulated burst");
        assert_eq!(burst.samples.len(), frame.len(), "burst is the whole frame");
        let decoded = rx.decode_burst("BPSK250", &burst).unwrap();
        assert_eq!(&decoded[..b"burst capture".len()], b"burst capture");
    }

    #[test]
    fn accumulate_capture_streams_burst_and_feeds_spectrum_tap() {
        // The daemon owns ONE persistent input stream (a per-tick reopen never warms
        // up cpal) and feeds each read() to accumulate_capture. Verify it accumulates
        // across reads, flushes on carrier drop, decodes, and feeds the spectrum tap.
        let tx_lb = LoopbackBackend::new();
        let mut tx = ModemEngine::new(Box::new(tx_lb.clone_shared()));
        tx.register_plugin(Box::new(BpskPlugin::new())).unwrap();
        tx.transmit(b"streamed burst", "BPSK250", None).unwrap();
        let frame = tx_lb.drain_samples();
        assert!(!frame.is_empty());

        let mut rx = ModemEngine::new(Box::new(LoopbackBackend::new()));
        rx.register_plugin(Box::new(BpskPlugin::new())).unwrap();
        assert!(rx.last_audio().is_empty(), "no audio captured yet");

        let chunk = frame.len() / 4 + 1;
        for frag in frame.chunks(chunk) {
            assert!(
                rx.accumulate_capture(None, frag.to_vec())
                    .unwrap()
                    .is_none(),
                "mid-burst read must keep accumulating"
            );
        }
        assert!(
            !rx.last_audio().is_empty(),
            "accumulate_capture must feed the spectrum/waterfall tap"
        );
        // A silent read (carrier dropped) flushes the complete burst.
        let burst = rx
            .accumulate_capture(None, vec![0.0; 256])
            .unwrap()
            .expect("carrier drop must flush the accumulated burst");
        assert_eq!(burst.samples.len(), frame.len(), "burst is the whole frame");
        let decoded = rx.decode_burst("BPSK250", &burst).unwrap();
        assert_eq!(&decoded[..b"streamed burst".len()], b"streamed burst");
    }

    #[test]
    fn decode_burst_scans_onset_when_frame_not_at_sample_zero() {
        // A DCD-detected hardware burst starts before the true frame onset, so the
        // engine's single-window demod (which settles AFC on the window start)
        // misframes from sample 0. decode_burst must scan onset offsets to recover it.
        let tx_lb = LoopbackBackend::new();
        let mut tx = ModemEngine::new(Box::new(tx_lb.clone_shared()));
        tx.register_plugin(Box::new(BpskPlugin::new())).unwrap();
        tx.transmit(b"onset scan", "BPSK250", None).unwrap();
        let frame = tx_lb.drain_samples();
        assert!(!frame.is_empty());

        let mut rx = ModemEngine::new(Box::new(LoopbackBackend::new()));
        rx.register_plugin(Box::new(BpskPlugin::new())).unwrap();

        // Prepend lead-in (one BPSK250 symbol period = 32 samples × 3) so the frame
        // onset is not at sample 0; decoding only from 0 would fail.
        let mut buf = vec![0.0f32; 32 * 3];
        buf.extend_from_slice(&frame);
        let decoded = rx
            .decode_burst("BPSK250", &AudioSamples { samples: buf })
            .expect("onset scan must recover a frame that does not start at sample 0");
        assert_eq!(&decoded[..b"onset scan".len()], b"onset scan");
    }

    #[test]
    fn last_audio_window_is_populated_for_the_spectrum_tap() {
        // The spectrum/waterfall tap reads last_audio(); it must hold real samples
        // after a transmit (TX window) and after a receive (RX window), not stay
        // empty — otherwise the daemon FFTs silence and the panel is flat.
        let mut engine = make_engine();
        assert!(
            engine.last_audio().is_empty(),
            "no audio captured/emitted yet"
        );
        engine.transmit(b"spectrum", "BPSK100", None).unwrap();
        assert!(
            !engine.last_audio().is_empty(),
            "transmit must populate the spectrum-tap window"
        );
        let _ = engine.receive("BPSK100", None).unwrap();
        assert!(
            !engine.last_audio().is_empty(),
            "receive must populate the spectrum-tap window"
        );
    }

    #[test]
    fn default_device_is_used_as_fallback_without_breaking_loopback() {
        // The default-device fallback (per-call None → engine default) must route
        // through the same open path; LoopbackBackend ignores the device name, so a
        // round-trip with a default device set still succeeds. This guards the
        // `device.or(self.default_device...)` plumbing the real-audio rig relies on.
        let mut engine = make_engine();
        engine.set_default_device(Some("snd-aloop-pcm".into()));
        engine.transmit(b"Hello", "BPSK100", None).unwrap();
        let received = engine.receive("BPSK100", None).unwrap();
        assert_eq!(received, b"Hello");
    }

    #[test]
    fn transmit_then_receive_with_timeout() {
        let mut engine = make_engine();
        engine.transmit(b"Hello", "BPSK100", None).unwrap();
        // Use a generous timeout — this test validates correctness, not speed.
        // AFC settling (6 Goertzel scans) plus the full RS-FEC decode can take
        // several hundred milliseconds in debug builds.
        let received = engine
            .receive_with_timeout("BPSK100", None, Duration::from_secs(30))
            .unwrap();
        assert_eq!(received, b"Hello");
    }

    #[test]
    fn unknown_mode_returns_error() {
        let mut engine = make_engine();
        assert!(engine.transmit(b"x", "UNKNOWN", None).is_err());
    }

    #[test]
    fn default_trust_policy_is_balanced() {
        let engine = make_engine();
        assert_eq!(engine.trust_policy_profile(), PolicyProfile::Balanced);
    }

    #[test]
    fn trust_policy_profile_can_be_updated() {
        let mut engine = make_engine();
        engine.set_trust_policy_profile(PolicyProfile::Strict);
        assert_eq!(engine.trust_policy_profile(), PolicyProfile::Strict);
    }

    #[test]
    fn secure_session_success_enters_active_transfer() {
        let mut engine = make_engine();
        engine.set_trust_policy_profile(PolicyProfile::Balanced);

        let decision = engine
            .begin_secure_session(
                SecureSessionParams {
                    local_minimum_mode: SigningMode::Normal,
                    peer_supported_modes: vec![SigningMode::Normal, SigningMode::Psk],
                    key_trust: PublicKeyTrustLevel::Full,
                    certificate_source: CertificateSource::OutOfBand,
                    psk_validated: false,
                },
                1_000,
            )
            .expect("secure session should start");

        assert_eq!(engine.hpx_state(), HpxState::ActiveTransfer);
        assert_eq!(decision.trust.decision, ConnectionTrustLevel::Verified);
        assert!(engine.active_handshake().is_some());
    }

    #[test]
    fn balanced_profile_rejects_unverified_handshake() {
        let mut engine = make_engine();
        engine.set_trust_policy_profile(PolicyProfile::Balanced);

        let err = engine
            .begin_secure_session(
                SecureSessionParams {
                    local_minimum_mode: SigningMode::Normal,
                    peer_supported_modes: vec![SigningMode::Normal],
                    key_trust: PublicKeyTrustLevel::Unknown,
                    certificate_source: CertificateSource::OutOfBand,
                    psk_validated: false,
                },
                2_000,
            )
            .expect_err("balanced should reject unverified trust");

        assert!(err.to_string().contains("below required 'pskverified'"));
        assert_eq!(engine.hpx_state(), HpxState::Failed);
    }

    #[test]
    fn strict_profile_rejects_psk_verified_but_not_verified() {
        let mut engine = make_engine();
        engine.set_trust_policy_profile(PolicyProfile::Strict);

        let err = engine
            .begin_secure_session(
                SecureSessionParams {
                    local_minimum_mode: SigningMode::Normal,
                    peer_supported_modes: vec![SigningMode::Normal],
                    key_trust: PublicKeyTrustLevel::Marginal,
                    certificate_source: CertificateSource::OverAir,
                    psk_validated: true,
                },
                2_500,
            )
            .expect_err("strict should reject trust below verified");

        assert!(err.to_string().contains("below required 'verified'"));
        assert_eq!(engine.hpx_state(), HpxState::Failed);
    }

    #[test]
    fn transmit_rejected_when_secure_session_not_active_transfer() {
        let mut engine = make_engine();
        engine.hpx_apply_event(HpxEvent::StartSession, 10).unwrap();

        let err = engine.transmit(b"hello", "BPSK100", None).unwrap_err();
        assert!(err
            .to_string()
            .contains("secure session is not in active transfer"));
    }

    #[test]
    fn transmit_allowed_after_secure_handshake() {
        let mut engine = make_engine();
        engine.set_trust_policy_profile(PolicyProfile::Permissive);

        engine
            .begin_secure_session(
                SecureSessionParams {
                    local_minimum_mode: SigningMode::Relaxed,
                    peer_supported_modes: vec![SigningMode::Normal, SigningMode::Relaxed],
                    key_trust: PublicKeyTrustLevel::Marginal,
                    certificate_source: CertificateSource::OutOfBand,
                    psk_validated: false,
                },
                3_000,
            )
            .unwrap();

        assert!(engine.transmit(b"payload", "BPSK100", None).is_ok());
    }

    #[test]
    fn signed_envelope_round_trip_helpers() {
        let engine = make_engine();
        let bytes = engine
            .encode_signed_envelope(
                b"payload",
                SigningMode::Normal,
                "peer-a",
                "key-1",
                &[1, 2, 3, 4],
            )
            .expect("encode envelope");

        let decoded = engine
            .decode_signed_envelope(&bytes)
            .expect("decode envelope");
        assert_eq!(decoded.payload, b"payload");
        assert_eq!(decoded.signature.signer_id, "peer-a");
        assert_eq!(decoded.signature.key_id, "key-1");
    }

    #[test]
    fn energy_gate_uses_absolute_floor_until_history_fills() {
        let mut g = EnergyGate::new();
        // Loopback silence well below the absolute floor: always gated.
        for _ in 0..10 {
            assert!(!g.passes(0.000_025));
        }
        // A loopback-level signal passes regardless of history fill.
        assert!(g.passes(0.002));
    }

    #[test]
    fn energy_gate_rises_above_elevated_noise_floor() {
        let mut g = EnergyGate::new();
        // On-air QRM floor ≈ 1.5e-3 passes the fixed 1e-4 gate at every
        // position; after the history fills the adaptive threshold must gate
        // it out (threshold clamps at 3.2e-3 ≥ 1.5e-3).
        for _ in 0..EnergyGate::HISTORY {
            g.passes(0.0015);
        }
        assert!(!g.passes(0.0015), "steady QRM floor must be gated out");
        // A genuine signal above the clamped threshold still passes.
        assert!(g.passes(0.0045));
    }

    #[test]
    fn scan_planner_incremental_positions_never_repeat() {
        let mut p = ScanPlanner::new(32, 1056);
        let first: Vec<usize> = p.scan_positions(3000).collect();
        assert_eq!(first.first(), Some(&0));
        // Last position still fits a minimal frame: largest step ≤ 3000−1056.
        assert_eq!(*first.last().unwrap(), 1920);
        p.commit_scan(3000);
        // More audio: the scan resumes at the committed boundary.
        let second: Vec<usize> = p.scan_positions(4000).collect();
        assert_eq!(second.first(), Some(&(3000 - 1056)));
        // Largest 1944 + k·32 that still fits a minimal frame (≤ 2944).
        assert_eq!(*second.last().unwrap(), 2936);
    }

    #[test]
    fn scan_planner_settle_records_first_energy_without_rewind() {
        let mut p = ScanPlanner::new(32, 1056);
        p.commit_scan(50_000);
        p.note_settled(1234);
        // Settling records the first-energy position for the dedicated re-decode,
        // but must NOT rewind the scan — rewinding made the broad scan re-decode
        // the whole buffer each iteration and stalled the loop.
        assert!(p.is_settled());
        assert_eq!(p.first_energy_pos(), Some(1234));
        assert_eq!(p.scan_positions(50_000).next(), Some(50_000 - 1056));
    }

    #[test]
    fn scan_planner_retry_cadence() {
        let mut p = ScanPlanner::new(32, 1056);
        // Before T=12 s: never.
        assert!(!p.retry_due(0, 10_000));
        assert!(!p.retry_due(11, 10_000));
        // Empty buffer: never.
        assert!(!p.retry_due(20, 0));
        // First firing at T>=12 with data.
        assert!(p.retry_due(12, 10_000));
        // Within the 2 s interval: no.
        assert!(!p.retry_due(13, 10_000));
        // After the interval: fires again.
        assert!(p.retry_due(14, 10_000));
        assert!(!p.retry_due(15, 10_000));
        assert!(p.retry_due(16, 10_000));
    }
}
