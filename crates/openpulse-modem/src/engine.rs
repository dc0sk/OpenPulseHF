//! The core [`ModemEngine`] struct.

use openpulse_audio::tanh_limit;
use rand::Rng;
use tokio::sync::broadcast;
use tracing::{debug, info};

use openpulse_core::ack::AckFrame;
use openpulse_core::ack::AckType;
use openpulse_core::audio::{AudioBackend, AudioConfig};
use openpulse_core::conv::ConvCodec;
use openpulse_core::dcd::DcdState;
use openpulse_core::error::{ModemError, PluginError};
use openpulse_core::fec::{
    combine_llrs_weighted, FecCodec, FecMode, Interleaver, ShortFecCodec, SoftCombiner,
    DEFAULT_INTERLEAVER_DEPTH,
};
use openpulse_core::frame::Frame;
use openpulse_core::hpx::{HpxEvent, HpxSession, HpxState, HpxTransition};
use openpulse_core::ldpc::{IterativeDecoder, LdpcCodec, LDPC_CODEWORD_BYTES, LDPC_MAX_INFO_BYTES};
use openpulse_core::plugin::{ModulationConfig, PluginRegistry};
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::{BiDirRateAdapter, RateEvent, RateTrigger};
use openpulse_core::signed_envelope::SignedEnvelope;
use openpulse_core::soft_viterbi::SoftViterbiCodec;
use openpulse_core::trust::{
    evaluate_handshake, CertificateSource, ConnectionTrustLevel, HandshakeDecision, PolicyProfile,
    PublicKeyTrustLevel, SigningMode,
};
use openpulse_core::wire_query::{callsign_hash, BroadcastFrame, WireEnvelope, WireMsgType};

use crate::event::{EngineEvent, RateDirection};
use crate::pipeline::{
    AudioSamples, BackpressurePolicy, DecodedFrame, PipelineMetricsSnapshot, PipelineScheduler,
    PipelineStage, WirePayload,
};

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
    rate_adapter: Option<BiDirRateAdapter>,
    session_profile: Option<SessionProfile>,
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
            rate_adapter: None,
            session_profile: None,
            dcd: DcdState::new(0.01, 800), // 100 ms hold at 8 kHz
            csma_enabled: false,
            csma_persistence: 0.3,
            event_tx,
            broadcast_seq: 0,
            callsign: String::new(),
            tx_attenuation_db: 0.0,
            tx_limiter_threshold: 0.0,
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
    /// Initialises a [`BiDirRateAdapter`] at `profile.initial_level` and stores the
    /// profile so that [`current_adaptive_mode`](Self::current_adaptive_mode)
    /// can resolve the current mode string on each transmit/receive cycle.
    pub fn start_adaptive_session(&mut self, profile: SessionProfile) {
        let initial = profile.initial_level;
        let threshold = profile.nack_threshold;
        self.rate_adapter = Some(BiDirRateAdapter::new(initial, threshold));
        self.session_profile = Some(profile);
    }

    /// Apply a received ACK type to the TX-direction rate adapter.
    ///
    /// Returns [`RateEvent::Maintained`] when no adaptive session is active.
    pub fn apply_ack(&mut self, ack: AckType) -> RateEvent {
        self.apply_ack_internal(ack, None)
    }

    /// Apply a received ACK frame, updating both TX and RX directions.
    ///
    /// When the frame carries a `reverse_ack`, the RX-direction adapter is also
    /// updated and a second `RateChange` event is emitted.
    pub fn apply_ack_frame(&mut self, frame: &openpulse_core::ack::AckFrame) -> RateEvent {
        let tx_event = self.apply_ack_internal(frame.ack_type, Some(RateDirection::Tx));
        if let Some(rev) = frame.reverse_ack {
            if let Some(adapter) = self.rate_adapter.as_mut() {
                let rx_event = adapter.apply_reverse_ack(rev);
                let rx_level = adapter.rx_level();
                let mode = self
                    .session_profile
                    .as_ref()
                    .and_then(|p| p.mode_for(rx_level))
                    .unwrap_or("unknown")
                    .to_string();
                let _ = self.event_tx.send(EngineEvent::RateChange {
                    event: rx_event,
                    speed_level: rx_level,
                    mode,
                    direction: Some(RateDirection::Rx),
                    trigger: None,
                });
            }
        }
        tx_event
    }

    fn apply_ack_internal(&mut self, ack: AckType, direction: Option<RateDirection>) -> RateEvent {
        let rate_event = match self.rate_adapter.as_mut() {
            Some(adapter) => adapter.apply_ack(ack),
            None => RateEvent::Maintained,
        };
        let speed_level = self
            .rate_adapter
            .as_ref()
            .map(|a| a.tx_level())
            .unwrap_or(openpulse_core::rate::SpeedLevel::Sl2);
        let mode = self
            .current_adaptive_mode()
            .unwrap_or("unknown")
            .to_string();
        if self.rate_adapter.is_some() {
            let _ = self.event_tx.send(EngineEvent::RateChange {
                event: rate_event,
                speed_level,
                mode,
                direction,
                trigger: None,
            });
        }
        rate_event
    }

    /// Return the mode string for the current TX speed level of the active adaptive session.
    ///
    /// Returns `None` when no profile is active or the current speed level has no
    /// mode assigned (e.g. SL1 chirp fallback, reserved levels).
    pub fn current_adaptive_mode(&self) -> Option<&str> {
        let profile = self.session_profile.as_ref()?;
        let adapter = self.rate_adapter.as_ref()?;
        profile.mode_for(adapter.tx_level())
    }

    /// Return the mode string for the current RX speed level.
    pub fn current_rx_mode(&self) -> Option<&str> {
        let profile = self.session_profile.as_ref()?;
        let adapter = self.rate_adapter.as_ref()?;
        profile.mode_for(adapter.rx_level())
    }

    /// Return the current TX [`SpeedLevel`](openpulse_core::rate::SpeedLevel).
    pub fn current_tx_level(&self) -> Option<openpulse_core::rate::SpeedLevel> {
        self.rate_adapter.as_ref().map(|a| a.tx_level())
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
        let Some(adapter) = self.rate_adapter.as_mut() else {
            return;
        };
        let Some(profile) = self.session_profile.as_ref() else {
            return;
        };
        let tx_level = adapter.tx_level();
        let floor_db = profile
            .snr_floor_for_level(tx_level)
            .unwrap_or(f32::NEG_INFINITY);
        let ceiling_db = profile
            .snr_ceiling_for_level(tx_level)
            .unwrap_or(f32::INFINITY);
        if let Some(rate_event) = adapter.tx.apply_snr_hint(snr_db, floor_db, ceiling_db) {
            let new_level = adapter.tx_level();
            let mode = profile.mode_for(new_level).unwrap_or("unknown").to_string();
            let _ = self.event_tx.send(EngineEvent::RateChange {
                event: rate_event,
                speed_level: new_level,
                mode,
                direction: Some(RateDirection::Tx),
                trigger: Some(RateTrigger::SnrFloor),
            });
        }
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

        let outbound = self.stage_encode_frame(data);
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

        let outbound = self.stage_encode_frame(data);
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

        let wire = {
            let plugin = self
                .plugins
                .get(mode)
                .ok_or_else(|| ModemError::PluginNotFound(mode.to_string()))?;
            self.stage_demodulate_payload(plugin, mode, &samples)?
        };
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

    /// Like [`transmit`](Self::transmit) but wraps the encoded frame bytes
    /// with Reed-Solomon FEC before modulation.
    ///
    /// Set the station callsign used in broadcast frame headers.
    pub fn set_callsign(&mut self, callsign: impl Into<String>) {
        self.callsign = callsign.into();
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

        let outbound = self.stage_encode_frame(&wire_bytes);
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

        let frame_wire = self.stage_encode_frame(data);
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

        let frame_wire = self.stage_encode_frame(data);
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

        let frame_wire = self.stage_encode_frame(data);
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

        let frame_wire = self.stage_encode_frame(data);
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
        let wire = self.stage_encode_frame(data);
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

        let frame_wire = self.stage_encode_frame(data);
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
    /// Soft-Viterbi + RS decode.
    ///
    /// Each attempt yields a LLR vector from `plugin.demodulate_soft`.  The
    /// per-frame noise-variance proxy is `1 / mean(|LLR|)` — frames with higher
    /// confidence (larger magnitude LLRs) receive proportionally more weight.
    ///
    /// This provides ~2–4 dB improvement over equal-weight sample combining when
    /// different attempts experience different SNR (e.g., Watterson fading).
    ///
    /// TX chain: `transmit_with_fec` or `transmit_with_fec_mode(Concatenated)`.
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
        let mut first_audio: Vec<f32> = Vec::new();

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
                first_audio = samples.samples.clone();
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

        self.update_afc_estimate(mode, &first_audio);
        if let Some(hz) = self.last_afc_offset_hz {
            let _ = self.event_tx.send(EngineEvent::AfcUpdate {
                offset_hz: hz,
                correction_hz: self.afc_correction_hz,
                mode: mode.to_string(),
            });
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
    /// `FecMode::ShortRs` is reserved for ACK frames; this method returns
    /// `Err(ModemError::Frame)` when called with it — use
    /// [`transmit_ack_with_short_fec`](Self::transmit_ack_with_short_fec) instead.
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
            FecMode::ShortRs => Err(ModemError::Frame(
                "FecMode::ShortRs is for ACK frames only; use transmit_ack_with_short_fec".into(),
            )),
            FecMode::RsStrong => self.transmit_with_strong_fec(data, mode, device),
            FecMode::SoftConcatenated => self.transmit_with_soft_viterbi_fec(data, mode, device),
            FecMode::Ldpc => self.transmit_with_ldpc(data, mode, device),
        }
    }

    /// Receive with the codec selected by `fec`.
    ///
    /// Mirror of [`transmit_with_fec_mode`](Self::transmit_with_fec_mode).
    /// `FecMode::ShortRs` returns `Err` — use
    /// [`receive_ack_with_short_fec`](Self::receive_ack_with_short_fec) instead.
    pub fn receive_with_fec_mode(
        &mut self,
        mode: &str,
        fec: FecMode,
        device: Option<&str>,
    ) -> Result<Vec<u8>, ModemError> {
        match fec {
            FecMode::None => self.receive(mode, device),
            FecMode::Rs => self.receive_with_fec(mode, device),
            FecMode::RsInterleaved => {
                self.receive_with_fec_interleaved(mode, device, DEFAULT_INTERLEAVER_DEPTH)
            }
            FecMode::Concatenated => self.receive_with_concatenated_fec(mode, device),
            FecMode::ShortRs => Err(ModemError::Frame(
                "FecMode::ShortRs is for ACK frames only; use receive_ack_with_short_fec".into(),
            )),
            FecMode::RsStrong => self.receive_with_strong_fec(mode, device),
            FecMode::SoftConcatenated => self.receive_with_soft_viterbi_fec(mode, device),
            FecMode::Ldpc => self.receive_with_ldpc(mode, device),
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

    fn stage_encode_frame(&mut self, data: &[u8]) -> WirePayload {
        let _stage = PipelineStage::EncodeModulate;
        let frame = Frame::new(self.sequence, data.to_vec());
        self.sequence = self.sequence.wrapping_add(1);
        WirePayload {
            bytes: frame.encode(),
        }
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
