//! The core [`ModemEngine`] struct.

use tracing::{debug, info};

use openpulse_core::audio::{AudioBackend, AudioConfig};
use openpulse_core::error::{ModemError, PluginError};
use openpulse_core::fec::FecCodec;
use openpulse_core::frame::Frame;
use openpulse_core::hpx::{HpxEvent, HpxSession, HpxState, HpxTransition};
use openpulse_core::plugin::{ModulationConfig, PluginRegistry};
use openpulse_core::signed_envelope::SignedEnvelope;
use openpulse_core::trust::{
    evaluate_handshake, CertificateSource, ConnectionTrustLevel, HandshakeDecision, PolicyProfile,
    PublicKeyTrustLevel, SigningMode,
};

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
}

impl ModemEngine {
    /// Create a new engine backed by the given audio backend.
    pub fn new(audio: Box<dyn AudioBackend>) -> Self {
        Self {
            audio,
            plugins: PluginRegistry::new(),
            sequence: 0,
            hpx: HpxSession::new(),
            scheduler: PipelineScheduler::new(8, BackpressurePolicy::Block),
            trust_policy_profile: PolicyProfile::Balanced,
            active_handshake: None,
        }
    }

    /// Returns the active trust policy profile used as session default.
    pub fn trust_policy_profile(&self) -> PolicyProfile {
        self.trust_policy_profile
    }

    /// Sets the active trust policy profile used as session default.
    pub fn set_trust_policy_profile(&mut self, profile: PolicyProfile) {
        self.trust_policy_profile = profile;
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
        Ok(handshake)
    }

    /// Gracefully closes an active secure HPX session.
    pub fn end_secure_session(&mut self, timestamp_ms: u64) -> Result<(), ModemError> {
        if self.hpx_state() == HpxState::Idle {
            self.active_handshake = None;
            return Ok(());
        }

        self.hpx_apply_event(HpxEvent::LocalCancel, timestamp_ms)?;
        self.hpx_apply_event(HpxEvent::TransferComplete, timestamp_ms.saturating_add(1))?;
        self.active_handshake = None;
        Ok(())
    }

    /// Apply an HPX state-machine event and return the emitted transition event.
    pub fn hpx_apply_event(
        &mut self,
        event: HpxEvent,
        timestamp_ms: u64,
    ) -> Result<HpxTransition, ModemError> {
        self.hpx
            .apply_event(event, timestamp_ms)
            .map_err(|e| ModemError::Configuration(e.to_string()))
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

        Ok(())
    }

    /// Read audio from the input device, demodulate with the plugin for
    /// `mode`, and return the decoded frame payload.
    ///
    /// Pass `device = None` to use the backend's default input device.
    pub fn receive(&mut self, mode: &str, device: Option<&str>) -> Result<Vec<u8>, ModemError> {
        let samples = self.stage_capture_input(device)?;
        let samples = self.route_audio_stage(PipelineStage::InputCapture, samples)?;

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

        let frame = self.stage_decode_frame(&wire)?;
        let frame = self.route_decoded_stage(PipelineStage::HpxStateUpdate, frame)?;
        info!("received frame seq={}", frame.sequence);

        Ok(frame.payload)
    }

    /// Like [`transmit`](Self::transmit) but wraps the encoded frame bytes
    /// with Reed-Solomon FEC before modulation.
    ///
    /// On a noisy channel the receiver can use [`receive_with_fec`](Self::receive_with_fec)
    /// to correct up to **16 byte errors per 255-byte RS block** after
    /// demodulation.
    pub fn transmit_with_fec(
        &mut self,
        data: &[u8],
        mode: &str,
        device: Option<&str>,
    ) -> Result<(), ModemError> {
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
        self.stage_emit_output(device, &samples)
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

        let raw_wire = {
            let plugin = self
                .plugins
                .get(mode)
                .ok_or_else(|| ModemError::PluginNotFound(mode.to_string()))?;
            self.stage_demodulate_payload(plugin, mode, &samples)?
        };
        let raw_wire = self.route_wire_stage(PipelineStage::DemodulateDecode, raw_wire)?;

        let corrected_bytes = FecCodec::new().decode(&raw_wire.bytes)?;
        let corrected_wire = WirePayload {
            bytes: corrected_bytes,
        };

        let frame = self.stage_decode_frame(&corrected_wire)?;
        let frame = self.route_decoded_stage(PipelineStage::HpxStateUpdate, frame)?;
        info!("FEC receive: frame seq={}", frame.sequence);

        Ok(frame.payload)
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

        stream
            .write(&samples.samples)
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

    fn stage_demodulate_payload(
        &self,
        plugin: &dyn openpulse_core::plugin::ModulationPlugin,
        mode: &str,
        samples: &AudioSamples,
    ) -> Result<WirePayload, ModemError> {
        let _stage = PipelineStage::DemodulateDecode;
        let mod_cfg = ModulationConfig {
            mode: mode.to_string(),
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
