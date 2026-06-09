//! The core [`ModemEngine`] struct.

use openpulse_audio::tanh_limit;
use rand::Rng;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tracing::{debug, info};

use openpulse_core::ack::AckFrame;
use openpulse_core::ack::AckType;
use openpulse_core::audio::{AudioBackend, AudioConfig};
use openpulse_core::conv::ConvCodec;
use openpulse_core::dcd::DcdState;
use openpulse_core::error::{ModemError, PluginError};
use openpulse_core::fec::{
    apply_window_retransmit, combine_llrs_weighted, combine_llrs_weighted_in_ranges,
    encode_window_retransmit, FecCodec, FecMode, Interleaver, ShortFecCodec, SoftCombiner,
    WindowArqFeedback, DEFAULT_INTERLEAVER_DEPTH,
};
use openpulse_core::frame::Frame;
use openpulse_core::hpx::{HpxEvent, HpxSession, HpxState, HpxTransition};
use openpulse_core::ldpc::{IterativeDecoder, LdpcCodec, LDPC_CODEWORD_BYTES, LDPC_MAX_INFO_BYTES};
use openpulse_core::plugin::{ModulationConfig, PluginRegistry};
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::RateEvent;
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
}

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
        }
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

        self.stage_emit_output(device, &samples)?;

        // Log transmission metadata for regulatory compliance
        self.update_tx_session_callsign();
        let tx_seq = self.sequence.wrapping_sub(1);
        let metadata = TxMetadata::new(&self.callsign, mode, self.max_power_watts, tx_seq);
        self.tx_session_log
            .log_frame(metadata.clone())
            .map_err(|err| ModemError::Configuration(err.to_string()))?;
        debug!("logged TX metadata: {}", metadata.to_log_line());

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
            .open_iq_output(device, &audio_cfg)
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
        let samples = self.stage_capture_input(device)?;
        self.receive_from_samples(mode, samples)
    }

    /// Receive a frame by listening on the input stream until a decode succeeds
    /// or the timeout elapses.
    pub fn receive_with_timeout(
        &mut self,
        mode: &str,
        device: Option<&str>,
        listen_for: Duration,
    ) -> Result<Vec<u8>, ModemError> {
        let audio_cfg = AudioConfig::default();
        let mut stream = self
            .audio
            .open_input(device, &audio_cfg)
            .map_err(|e| ModemError::Audio(e.to_string()))?;

        let deadline = Instant::now() + listen_for;
        let mut accumulated = Vec::new();
        let mut last_err: Option<ModemError> = None;

        // Symbol period in samples, used as the sliding-search step.
        // Derived from the numeric baud rate embedded in the mode name (e.g. "BPSK250" → 250);
        // falls back to 32 (BPSK250 at 8 kHz) so the step is always ≤ one symbol period.
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
        // Minimum sample count required for at least one decodable frame.
        // 33 = PREAMBLE_SYMS(32) + 1 data symbol.
        let min_frame_samples = step * 33;

        // Maximum window passed to each demodulation attempt.
        // PREAMBLE_SYMS(32) + full RS FEC frame (255 bytes = 2040 bits BPSK) = 2072 symbols,
        // plus 10 % margin.  Bounding the slice keeps per-attempt cost O(1) regardless of
        // how much silence has accumulated before the signal arrived.
        let max_frame_samples = step * 2280;

        // RMS energy threshold for the fast silence gate (same as DcdState default: 0.01 RMS
        // → 0.0001 mean-square).  Silence/noise floor is typically < 0.000025 mean-square;
        // a live BPSK carrier at 30 % full-scale gives mean-square ≈ 0.045.
        const ENERGY_GATE_THRESHOLD: f32 = 0.0001;
        // AFC settling requires stronger signal energy than the fast energy gate.
        // The main gate (0.0001 ≈ -40 dBFS RMS) skips obvious silence; the AFC
        // gate (0.001 ≈ -30 dBFS RMS) ensures settling fires on the actual BPSK
        // carrier, not on receiver noise or audio system artefacts whose Goertzel
        // response can lock to a wrong frequency.
        const AFC_SETTLE_THRESHOLD: f32 = 0.001;
        // Maximum AFC correction magnitude accepted after settling.  The Goertzel
        // scan covers ±300 Hz; well-calibrated rigs should be within ±200 Hz of
        // the nominal carrier.  A larger correction indicates the AFC locked onto
        // noise or an artefact rather than the real signal.
        const AFC_MAX_CORRECTION_HZ: f32 = 250.0;

        // Incremental scan: only try start positions not yet attempted.
        let mut last_tried_end: usize = 0;

        // AFC pre-convergence state.
        // On first signal detection, run fast AFC settling passes in-place (no decode),
        // then reset the scan to that position so the first full decode attempt uses
        // a converged AFC correction.  Without this, afc_step=0.1 takes ~22 scan
        // positions (~704 samples) to converge, by which point we have already
        // advanced past the preamble start and can never re-decode it.
        let mut first_energy_pos: Option<usize> = None;
        let mut scan_reset_pending = false;
        // When the incremental scan advances past first_energy_pos before the full
        // frame has buffered, we schedule one targeted retry over a ±PREAMBLE_SYMS
        // window around fep once the frame has ended.  last_signal_pos tracks the
        // last main-scan position where the energy gate passed, which marks the
        // approximate end of the transmitted frame.  The retry fires as soon as the
        // scan moves past last_signal_pos (signal gone → frame complete) and the
        // buffer holds one preamble of margin past last_signal_pos.  Fallback: if
        // last_signal_pos was never set (signal too short), fire at fep+max_frame_samples.
        let mut fep_full_retry_done = false;
        let mut last_signal_pos: usize = 0;

        loop {
            let chunk = stream
                .read()
                .map_err(|e| ModemError::Audio(e.to_string()))?;
            if !chunk.is_empty() {
                accumulated.extend(chunk);
                debug!("received {} accumulated audio samples", accumulated.len());
            }

            // After AFC settling, reset the scan back to the first energy position.
            // This fires even when no new audio arrived so that the decode pass runs
            // immediately against the already-accumulated buffer (avoids stalling when
            // the audio backend returns empty after the initial fill, e.g. loopback).
            if scan_reset_pending {
                scan_reset_pending = false;
                if let Some(fep) = first_energy_pos {
                    last_tried_end = last_tried_end.min(fep);
                }
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
            if !fep_full_retry_done {
                if let Some(fep) = first_energy_pos {
                    if accumulated.len() >= fep.saturating_add(max_frame_samples) {
                        fep_full_retry_done = true;
                        // Scan fep .. fep + PREAMBLE_SYMS×step (one full preamble
                        // length forward).  The energy gate fires when the signal
                        // carrier is first detectable, but ISS CPAL startup latency
                        // can delay the actual first preamble symbol by up to one
                        // preamble length (~88 ms / 1024 samples at 8 kHz).
                        // Scanning forward from fep covers that delay window.
                        // find_timing_offset handles the sub-symbol (<32 sample)
                        // misalignment within each candidate start position.
                        let lookback = step; // one symbol back in case gate fires slightly late
                        let retry_start = fep.saturating_sub(lookback);
                        let retry_end = fep + step * 32; // one preamble length forward
                        // Keep the settled AFC correction for the retry attempts.
                        // The initial settling happens on the signal (or near-zero
                        // noise), so the correction is valid or safely near 0.
                        let saved_afc = self.afc_correction_hz;
                        for start in (retry_start..=retry_end).step_by(step) {
                            let end = (start + max_frame_samples).min(accumulated.len());
                            if end.saturating_sub(start) < min_frame_samples {
                                continue;
                            }
                            debug!("AFC full-retry: pos={start} correction={:.1}Hz", self.afc_correction_hz);
                            match self.receive_from_samples(
                                mode,
                                AudioSamples {
                                    samples: accumulated[start..end].to_vec(),
                                },
                            ) {
                                Ok(payload) => return Ok(payload),
                                Err(err) => {
                                    debug!("AFC full-retry: pos={start} FAILED: {err}");
                                    last_err = Some(err);
                                    self.afc_correction_hz = saved_afc;
                                }
                            }
                        }
                    }
                }
            }

            // Scan start positions: covers both new positions added by the latest chunk
            // and (after AFC reset) positions from the first-energy point onward.
            if !accumulated.is_empty() {
                let new_end = accumulated.len().saturating_sub(min_frame_samples);
                'inner: for start in (last_tried_end..=new_end).step_by(step) {
                    // Fast energy gate: check the first 32 symbol periods at this
                    // position.  Silence costs < 0.1 ms; only emit the full
                    // demodulation call (≈ 90 ms on a Pi 4) when signal is present.
                    let gate_end = (start + step * 32).min(accumulated.len());
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
                    if mean_sq < ENERGY_GATE_THRESHOLD {
                        continue;
                    }

                    // On the very first signal-energy position, run 6 fast AFC
                    // estimation passes in-place before attempting any decode.
                    // A temporary step of 0.7 converges in 6 iterations:
                    // (1 − 0.3⁶) × 150 Hz ≈ 149.9 Hz — effectively one-shot for
                    // crystal errors up to ±300 Hz on 144 MHz (≈ ±2 ppm).
                    if first_energy_pos.is_none() {
                        let end = (start + max_frame_samples).min(accumulated.len());
                        // Require higher energy for AFC settling than for the main
                        // scan gate.  This prevents locking to receiver noise or audio
                        // artefacts before the real BPSK carrier arrives.
                        if mean_sq < AFC_SETTLE_THRESHOLD {
                            continue;
                        }
                        // estimate_carrier_hz_wide needs at least PREAMBLE_SYMS
                        // symbol periods (32 × step = 1024 samples for BPSK250).
                        // Defer settling until the window is large enough.
                        if end - start < step * 32 {
                            continue;
                        }
                        let saved_step = self.afc_step;
                        self.afc_step = 0.7;
                        let mut prev_correction = self.afc_correction_hz;
                        for _ in 0..6 {
                            prev_correction = self.afc_correction_hz;
                            self.update_afc_estimate(mode, &accumulated[start..end]);
                        }
                        self.afc_step = saved_step;
                        // Reject if the last estimate jumped by ≥ 5 Hz (oscillating
                        // noise) or if the correction magnitude is implausibly large
                        // (noise artefact locked to wrong frequency).
                        let converged = (self.afc_correction_hz - prev_correction).abs() < 5.0;
                        let plausible = self.afc_correction_hz.abs() <= AFC_MAX_CORRECTION_HZ;
                        if !converged || !plausible {
                            debug!(
                                "AFC settling rejected at pos={start}: \
                                 converged={converged} plausible={plausible} \
                                 correction={:.1}Hz",
                                self.afc_correction_hz
                            );
                            self.afc_correction_hz = 0.0;
                            continue;
                        }
                        first_energy_pos = Some(start);
                        info!(
                            "AFC settling done: correction={:.1}Hz buf_len={}",
                            self.afc_correction_hz,
                            accumulated.len()
                        );
                        scan_reset_pending = true;
                        break 'inner;
                    }

                    // Record the furthest signal-energy position seen so we know
                    // when the frame has ended (energy gate will fail past this).
                    if start > last_signal_pos {
                        last_signal_pos = start;
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
                    match self.receive_from_samples(
                        mode,
                        AudioSamples {
                            samples: accumulated[start..end].to_vec(),
                        },
                    ) {
                        Ok(payload) => return Ok(payload),
                        Err(err) => {
                            last_err = Some(err);
                            self.afc_correction_hz = afc_before;
                        }
                    }
                }
                if new_end > last_tried_end {
                    last_tried_end = new_end;
                }
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
                ..ModulationConfig::default()
            };
            // Prefer soft demodulation: a single pass yields both LLRs (for SNR)
            // and hard bits (via sign decision), avoiding a redundant demodulate() call.
            match plugin.demodulate_soft(&samples.samples, &mod_cfg) {
                Ok(llrs) => {
                    let snr = RateAdaptationPolicy::snr_from_llrs(&llrs);
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
                }
                Err(_) => {
                    // Plugin does not support soft demodulation; use hard path.
                    let wire = self.stage_demodulate_payload(plugin, mode, &samples)?;
                    (wire, None)
                }
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
        let samples = self.stage_capture_input(device)?;
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
            ..ModulationConfig::default()
        };

        let plugin = self
            .plugins
            .get(mode)
            .ok_or_else(|| ModemError::PluginNotFound(mode.to_string()))?;

        let llrs = plugin.demodulate_soft(&samples.samples, &mod_cfg)?;
        let snr_db = RateAdaptationPolicy::snr_from_llrs(&llrs);
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
        self.stage_emit_output(device, &samples)?;

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
        self.stage_emit_output(device, &samples)?;
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
        let samples = self.stage_capture_input(device)?;
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
        self.stage_emit_output(device, &samples)?;
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
        let samples = self.stage_capture_input(device)?;
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
        self.stage_emit_output(device, &samples)?;
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
        let samples = self.stage_capture_input(device)?;
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
        self.stage_emit_output(device, &samples)?;
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
        let samples = self.stage_capture_input(device)?;
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
        self.stage_emit_output(device, &samples)?;
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
        let samples = self.stage_capture_input(device)?;
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
        self.csma_check()?;

        let frame_wire = self.stage_encode_frame(data)?;
        if frame_wire.bytes.len() > LDPC_MAX_INFO_BYTES {
            return Err(ModemError::Frame(format!(
                "LDPC: encoded frame {} B exceeds one-block limit of {} B; split payload at call site",
                frame_wire.bytes.len(),
                LDPC_MAX_INFO_BYTES,
            )));
        }
        let codeword = LdpcCodec::new().encode(&frame_wire.bytes);
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
        self.stage_emit_output(device, &samples)?;
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
        let samples = self.stage_capture_input(device)?;
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

        // LDPC block is LDPC_CODEWORD_BYTES × 8 coded bits; trim any excess LLRs.
        let ldpc_llrs = &llrs[..(LDPC_CODEWORD_BYTES * 8).min(llrs.len())];
        let info_bytes = LdpcCodec::new().decode_soft(ldpc_llrs)?;

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
        self.stage_emit_output(device, &samples)?;
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
        let samples = self.stage_capture_input(device)?;
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
            let samples = self.stage_capture_input(device)?;
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
    /// Each attempt yields a LLR vector from `plugin.demodulate_soft`.  The
    /// per-frame noise-variance proxy is `1 / mean(|LLR|)` — frames with higher
    /// confidence (larger magnitude LLRs) receive proportionally more weight.
    /// Hard decisions are taken from the combined LLRs before RS decode.
    ///
    /// This provides ~2–4 dB improvement over equal-weight sample combining when
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

        let mut attempts: Vec<(Vec<f32>, f32)> = Vec::with_capacity(n_frames);

        for i in 0..n_frames {
            let samples = self.stage_capture_input(device)?;
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

            // Noise-variance proxy: 1 / mean(|LLR|).  High-confidence frames have
            // large-magnitude LLRs → small noise_var → high weight.
            let mean_abs = if llrs.is_empty() {
                1.0
            } else {
                llrs.iter().map(|v| v.abs()).sum::<f32>() / llrs.len() as f32
            };
            let noise_var = 1.0 / mean_abs.max(1e-6);

            attempts.push((llrs, noise_var));
        }

        let attempt_refs: Vec<(&[f32], f32)> = attempts
            .iter()
            .map(|(llrs, nv)| (llrs.as_slice(), *nv))
            .collect();
        let combined_llrs = combine_llrs_weighted(&attempt_refs);

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

    /// Receive via Window-ARQ range-limited weighted LLR combining.
    ///
    /// Captures `n_frames` receive attempts, combines soft LLRs only inside
    /// `feedback.ranges`, then takes hard decisions and RS-decodes the combined
    /// protected frame. Outside selected ranges, the first attempt is preserved.
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

        let mut attempts: Vec<(Vec<f32>, f32)> = Vec::with_capacity(n_frames);

        for i in 0..n_frames {
            let samples = self.stage_capture_input(device)?;
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

            let mean_abs = if llrs.is_empty() {
                1.0
            } else {
                llrs.iter().map(|v| v.abs()).sum::<f32>() / llrs.len() as f32
            };
            let noise_var = 1.0 / mean_abs.max(1e-6);
            attempts.push((llrs, noise_var));
        }

        let attempt_refs: Vec<(&[f32], f32)> = attempts
            .iter()
            .map(|(llrs, nv)| (llrs.as_slice(), *nv))
            .collect();
        let combined_llrs = combine_llrs_weighted_in_ranges(&attempt_refs, feedback);

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
        self.stage_emit_output(device, &samples)?;

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
        let samples = self.stage_capture_input(device)?;
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
        let decision = self.select_harq_decision(snr_db, fading_depth_db, retry_index);
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
        let decision = self.select_harq_decision(snr_db, fading_depth_db, retry_index);
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
            FecMode::SoftConcatenated | FecMode::Ldpc | FecMode::Turbo
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
        self.stage_emit_output(device, &samples)
    }

    /// Demodulate FSK4-ACK, ShortFecCodec decode (13 → 5 bytes), return `AckFrame`.
    pub fn receive_ack_with_short_fec(
        &mut self,
        device: Option<&str>,
    ) -> Result<AckFrame, ModemError> {
        let samples = self.stage_capture_input(device)?;
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
        self.stage_emit_output(device, &samples)?;
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
        let samples = self.stage_capture_input(device)?;
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
        samples: &AudioSamples,
    ) -> Result<(), ModemError> {
        let _stage = PipelineStage::OutputEmit;

        let audio_cfg = AudioConfig::default();
        let mut stream = self
            .audio
            .open_output(device, &audio_cfg)
            .map_err(|e| ModemError::Audio(e.to_string()))?;

        let atten_linear = 10.0f32.powf(self.tx_attenuation_db / 20.0);
        let mut write_samples: Vec<f32> = if (atten_linear - 1.0).abs() < 1e-6 {
            samples.samples.clone()
        } else {
            samples.samples.iter().map(|s| s * atten_linear).collect()
        };
        let threshold = self.tx_limiter_threshold;
        if threshold > 0.0 {
            tanh_limit(&mut write_samples, threshold);
        }

        stream
            .write(&write_samples)
            .map_err(|e| ModemError::Audio(e.to_string()))?;
        stream
            .flush()
            .map_err(|e| ModemError::Audio(e.to_string()))?;

        Ok(())
    }

    fn stage_capture_input(&mut self, device: Option<&str>) -> Result<AudioSamples, ModemError> {
        let _stage = PipelineStage::InputCapture;
        let audio_cfg = AudioConfig::default();
        let mut stream = self
            .audio
            .open_input(device, &audio_cfg)
            .map_err(|e| ModemError::Audio(e.to_string()))?;

        let samples = stream
            .read()
            .map_err(|e| ModemError::Audio(e.to_string()))?;
        Ok(AudioSamples { samples })
    }

    fn update_afc_estimate(&mut self, mode: &str, samples: &[f32]) {
        let mod_cfg = ModulationConfig {
            mode: mode.to_string(),
            center_frequency: self.center_frequency + self.afc_correction_hz,
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
        self.scheduler
            .route_audio(stage, payload)
            .map_err(|e| ModemError::Configuration(e.to_string()))
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

fn minimum_trust_for_profile(profile: PolicyProfile) -> ConnectionTrustLevel {
    match profile {
        PolicyProfile::Strict => ConnectionTrustLevel::Verified,
        PolicyProfile::Balanced => ConnectionTrustLevel::PskVerified,
        PolicyProfile::Permissive => ConnectionTrustLevel::Reduced,
    }
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
}
