//! Signed remote rig-control command — Phase 7.5.
//!
//! A trusted remote peer sends a [`RigCtrlCmd`] signed with their Ed25519 key.
//! [`RemoteControlHandler`] validates the signature, enforces the trust policy
//! (only `Full` trust level accepted), and checks a 30-second replay window
//! before returning a [`ValidatedRigCmd`] the caller can act on.

use std::collections::HashMap;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::handshake::TrustStore;
use crate::trust::PublicKeyTrustLevel;

// ── Errors ────────────────────────────────────────────────────────────────────

/// Errors returned by remote rig-control validation.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RemoteControlError {
    #[error("signature verification failed")]
    InvalidSignature,
    #[error("sender trust level insufficient (need Full, got {0:?})")]
    InsufficientTrust(PublicKeyTrustLevel),
    #[error("sender pubkey not found in trust store for station '{0}'")]
    UnknownSender(String),
    #[error("trust store pubkey mismatch for station '{0}'")]
    PubkeyMismatch(String),
    #[error("command timestamp outside replay window (skew {0} ms)")]
    ReplayWindowExpired(i64),
    #[error("replayed command (signature already seen)")]
    Replayed,
    #[error("encoding error: {0}")]
    Encoding(String),
}

// ── Command types ─────────────────────────────────────────────────────────────

/// Supported rig operation types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RigCtrlCmdType {
    SetFreq,
    SetMode,
    PttOn,
    PttOff,
}

/// The signable body of a rig-control command.
///
/// Serialised to canonical JSON and signed with the sender's Ed25519 key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RigCtrlCmdBody {
    pub cmd: RigCtrlCmdType,
    /// Target rig identifier: `"a"` or `"b"`.
    pub rig: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub freq_hz: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Unix timestamp in milliseconds when the command was generated.
    pub ts_ms: u64,
    /// Sender's station ID (callsign or peer_id hex string).
    pub sender_id: String,
}

/// Signed rig-control command frame.  Carried in a `WireEnvelope` with
/// `msg_type = WireMsgType::RigCtrlCmd` (0x09).
#[derive(Debug, Clone)]
pub struct RigCtrlCmd {
    pub body: RigCtrlCmdBody,
    /// Ed25519 verifying-key bytes of the sender (32 bytes).
    pub sender_pubkey: [u8; 32],
    /// Ed25519 signature over canonical JSON of `body` (64 bytes).
    pub signature: [u8; 64],
}

/// A rig-control command that has been validated by [`RemoteControlHandler`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedRigCmd {
    pub cmd: RigCtrlCmdType,
    pub rig: String,
    pub freq_hz: Option<u64>,
    pub mode: Option<String>,
    pub sender_id: String,
}

// ── Wire encode / decode ──────────────────────────────────────────────────────

impl RigCtrlCmd {
    /// Encode to bytes: 4-byte body-length (BE u32) + JSON body + 32-byte pubkey + 64-byte sig.
    pub fn encode(&self) -> Result<Vec<u8>, RemoteControlError> {
        let body_json = serde_json::to_vec(&self.body)
            .map_err(|e| RemoteControlError::Encoding(e.to_string()))?;
        let body_len = body_json.len() as u32;
        let mut out = Vec::with_capacity(4 + body_json.len() + 32 + 64);
        out.extend_from_slice(&body_len.to_be_bytes());
        out.extend_from_slice(&body_json);
        out.extend_from_slice(&self.sender_pubkey);
        out.extend_from_slice(&self.signature);
        Ok(out)
    }

    /// Decode from bytes produced by [`encode`].
    pub fn decode(bytes: &[u8]) -> Result<Self, RemoteControlError> {
        if bytes.len() < 4 + 32 + 64 {
            return Err(RemoteControlError::Encoding("buffer too short".into()));
        }
        let body_len = u32::from_be_bytes(
            bytes[..4]
                .try_into()
                .map_err(|_| RemoteControlError::Encoding("body-length slice error".into()))?,
        ) as usize;
        let body_end = 4 + body_len;
        if bytes.len() < body_end + 32 + 64 {
            return Err(RemoteControlError::Encoding(
                "body length overruns buffer".into(),
            ));
        }
        let body: RigCtrlCmdBody = serde_json::from_slice(&bytes[4..body_end])
            .map_err(|e| RemoteControlError::Encoding(e.to_string()))?;
        let sender_pubkey: [u8; 32] = bytes[body_end..body_end + 32]
            .try_into()
            .map_err(|_| RemoteControlError::Encoding("pubkey slice error".into()))?;
        let signature: [u8; 64] = bytes[body_end + 32..body_end + 96]
            .try_into()
            .map_err(|_| RemoteControlError::Encoding("signature slice error".into()))?;
        Ok(Self {
            body,
            sender_pubkey,
            signature,
        })
    }
}

// ── Constructor ───────────────────────────────────────────────────────────────

/// Build and sign a [`RigCtrlCmd`].
pub fn create_rig_ctrl_cmd(
    body: RigCtrlCmdBody,
    signing_key_seed: &[u8; 32],
) -> Result<RigCtrlCmd, RemoteControlError> {
    let body_json =
        serde_json::to_vec(&body).map_err(|e| RemoteControlError::Encoding(e.to_string()))?;
    let signing_key = SigningKey::from_bytes(signing_key_seed);
    let sig: Signature = signing_key.sign(&body_json);
    let sender_pubkey = signing_key.verifying_key().to_bytes();
    Ok(RigCtrlCmd {
        body,
        sender_pubkey,
        signature: sig.to_bytes(),
    })
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// Stateful handler that validates incoming [`RigCtrlCmd`] frames.
pub struct RemoteControlHandler {
    /// Replay cache: signature bytes → timestamp_ms.
    replay_cache: HashMap<[u8; 64], u64>,
    /// Replay window half-width in milliseconds (default 30 000).
    pub replay_window_ms: u64,
}

impl Default for RemoteControlHandler {
    fn default() -> Self {
        Self {
            replay_cache: HashMap::new(),
            replay_window_ms: 30_000,
        }
    }
}

impl RemoteControlHandler {
    /// Create a new handler with the default 30-second replay window.
    pub fn new() -> Self {
        Self::default()
    }

    /// Evict stale replay-cache entries older than `now_ms - replay_window_ms`.
    pub fn evict_expired(&mut self, now_ms: u64) {
        let cutoff = now_ms.saturating_sub(self.replay_window_ms);
        self.replay_cache.retain(|_, ts| *ts >= cutoff);
    }

    /// Validate `cmd` and return a [`ValidatedRigCmd`] on success.
    ///
    /// Checks (in order):
    /// 1. Ed25519 signature over canonical JSON of `cmd.body`.
    /// 2. Sender station ID is known in `trust_store`.
    /// 3. Trust store's pubkey for sender matches `cmd.sender_pubkey`.
    /// 4. Trust level is `Full` (operator has explicitly trusted this peer).
    /// 5. Timestamp within ±`replay_window_ms` of `now_ms`.
    /// 6. Signature not already seen (replay suppression).
    pub fn handle(
        &mut self,
        cmd: &RigCtrlCmd,
        trust_store: &dyn TrustStore,
        now_ms: u64,
    ) -> Result<ValidatedRigCmd, RemoteControlError> {
        // 1 — Verify Ed25519 signature.
        let body_json = serde_json::to_vec(&cmd.body)
            .map_err(|e| RemoteControlError::Encoding(e.to_string()))?;
        let vk = VerifyingKey::from_bytes(&cmd.sender_pubkey)
            .map_err(|_| RemoteControlError::InvalidSignature)?;
        let sig = Signature::from_bytes(&cmd.signature);
        vk.verify(&body_json, &sig)
            .map_err(|_| RemoteControlError::InvalidSignature)?;

        // 2 — Sender must be in the trust store.
        let sender_id = &cmd.body.sender_id;
        let stored_pubkey = trust_store
            .pubkey_for(sender_id)
            .ok_or_else(|| RemoteControlError::UnknownSender(sender_id.clone()))?;

        // 3 — Stored pubkey must match the frame's claimed pubkey.
        if stored_pubkey != cmd.sender_pubkey {
            return Err(RemoteControlError::PubkeyMismatch(sender_id.clone()));
        }

        // 4 — Trust level must be Full.
        let level = trust_store.trust_level(sender_id);
        if level != PublicKeyTrustLevel::Full {
            return Err(RemoteControlError::InsufficientTrust(level));
        }

        // 5 — Timestamp within replay window.
        let skew = cmd.body.ts_ms as i64 - now_ms as i64;
        if skew.unsigned_abs() > self.replay_window_ms {
            return Err(RemoteControlError::ReplayWindowExpired(skew));
        }

        // 6 — Replay suppression.
        self.evict_expired(now_ms);
        if self.replay_cache.contains_key(&cmd.signature) {
            return Err(RemoteControlError::Replayed);
        }
        self.replay_cache.insert(cmd.signature, cmd.body.ts_ms);

        Ok(ValidatedRigCmd {
            cmd: cmd.body.cmd.clone(),
            rig: cmd.body.rig.clone(),
            freq_hz: cmd.body.freq_hz,
            mode: cmd.body.mode.clone(),
            sender_id: sender_id.clone(),
        })
    }
}
