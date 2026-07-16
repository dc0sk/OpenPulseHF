use std::collections::HashMap;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::compression::CompressionAlgorithm;
use crate::error::ModemError;
use crate::fec::FecMode;
use crate::trust::{
    evaluate_handshake, CertificateSource, HandshakeDecision, PolicyProfile, PublicKeyTrustLevel,
    SigningMode, TrustError,
};

// ------------------------------------------------------------------
// Errors
// ------------------------------------------------------------------

/// Errors returned when creating or verifying a handshake frame.
#[derive(Debug, thiserror::Error)]
pub enum HandshakeError {
    #[error("invalid Ed25519 signature")]
    InvalidSignature,
    #[error("frame public key does not match the trusted key for this station")]
    PublicKeyMismatch,
    #[error("session ID mismatch: expected {expected}, got {got}")]
    SessionIdMismatch { expected: String, got: String },
    #[error("trust evaluation failed: {0:?}")]
    TrustFailure(TrustError),
    #[error("encoding error: {0}")]
    Encoding(String),
    #[error("responder selected a compression algorithm not offered by the initiator")]
    UnsupportedCompression,
    #[error("responder selected a FEC mode not offered by the initiator")]
    UnsupportedFecMode,
    #[error("handshake timestamp is stale: {skew_ms} ms skew exceeds {max_skew_ms} ms")]
    StaleTimestamp { skew_ms: u64, max_skew_ms: u64 },
    #[error("handshake carries no timestamp but freshness is required")]
    MissingTimestamp,
}

/// Freshness bound for verifying a handshake's signed timestamp, closing the
/// capture-replay window. The verifier rejects a frame whose `timestamp_ms`
/// differs from `now_ms` by more than `max_skew_ms` (in either direction), and
/// rejects a frame that carries no timestamp at all (`timestamp_ms == 0`).
#[derive(Debug, Clone, Copy)]
pub struct Freshness {
    /// The verifier's current wall-clock time in Unix milliseconds.
    pub now_ms: u64,
    /// Maximum tolerated clock skew between the two stations, in milliseconds.
    pub max_skew_ms: u64,
}

impl Freshness {
    /// Reject a stale/future-dated frame, or a frame with no timestamp when freshness is required.
    fn check(&self, timestamp_ms: u64) -> Result<(), HandshakeError> {
        if timestamp_ms == 0 {
            return Err(HandshakeError::MissingTimestamp);
        }
        let skew_ms = self.now_ms.abs_diff(timestamp_ms);
        if skew_ms > self.max_skew_ms {
            return Err(HandshakeError::StaleTimestamp {
                skew_ms,
                max_skew_ms: self.max_skew_ms,
            });
        }
        Ok(())
    }
}

impl From<TrustError> for HandshakeError {
    fn from(e: TrustError) -> Self {
        HandshakeError::TrustFailure(e)
    }
}

// ------------------------------------------------------------------
// TrustStore trait
// ------------------------------------------------------------------

/// Lookup table for peer public keys and their trust levels.
pub trait TrustStore {
    /// Returns the Ed25519 verifying-key bytes for the given station ID, if known.
    fn pubkey_for(&self, station_id: &str) -> Option<[u8; 32]>;

    /// Returns the trust level assigned to the given station ID.
    fn trust_level(&self, station_id: &str) -> PublicKeyTrustLevel;
}

/// In-memory trust store for testing and offline operation.
#[derive(Debug, Clone, Default)]
pub struct InMemoryTrustStore {
    entries: HashMap<String, ([u8; 32], PublicKeyTrustLevel)>,
}

impl InMemoryTrustStore {
    /// Create an empty trust store with no entries.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a station with a specific trust level.
    pub fn add_entry(
        &mut self,
        station_id: &str,
        pubkey: [u8; 32],
        trust_level: PublicKeyTrustLevel,
    ) {
        self.entries
            .insert(station_id.to_string(), (pubkey, trust_level));
    }

    /// Convenience: add a fully-trusted out-of-band entry.
    pub fn add_trusted(&mut self, station_id: &str, pubkey: [u8; 32]) {
        self.add_entry(station_id, pubkey, PublicKeyTrustLevel::Full);
    }

    /// Convenience: add a revoked entry.
    pub fn add_revoked(&mut self, station_id: &str, pubkey: [u8; 32]) {
        self.add_entry(station_id, pubkey, PublicKeyTrustLevel::Revoked);
    }
}

impl TrustStore for InMemoryTrustStore {
    fn pubkey_for(&self, station_id: &str) -> Option<[u8; 32]> {
        self.entries.get(station_id).map(|(k, _)| *k)
    }

    fn trust_level(&self, station_id: &str) -> PublicKeyTrustLevel {
        self.entries
            .get(station_id)
            .map(|(_, level)| *level)
            .unwrap_or(PublicKeyTrustLevel::Unknown)
    }
}

// ------------------------------------------------------------------
// ConReq — connection request frame
// ------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConReqBody {
    station_id: String,
    pubkey: Vec<u8>,
    signing_modes: Vec<SigningMode>,
    session_id: String,
    supported_compression: Vec<CompressionAlgorithm>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    supported_fec_modes: Vec<FecMode>,
    // Empty grid is skipped so legacy zero-grid frames (and their signatures) are byte-identical.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    station_grid: String,
    // Active OTA rate-ladder identity (name + fingerprint of the level→mode/FEC mapping). Skipped
    // when unset so legacy/no-OTA frames stay byte-identical for signature compatibility.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    profile_name: String,
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    profile_fingerprint: u64,
    // Unix-ms creation time, signed, for replay-freshness. Skipped when 0 so legacy no-timestamp
    // frames (and their signatures) stay byte-identical.
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    timestamp_ms: u64,
}

/// Connection request sent by the initiating station during Discovery.
///
/// The `signature` covers the canonical JSON of the body fields (excluding the
/// signature itself), signed with the initiator's Ed25519 private key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConReq {
    pub station_id: String,
    /// Ed25519 verifying-key bytes (32 bytes).
    pub pubkey: Vec<u8>,
    pub signing_modes: Vec<SigningMode>,
    pub session_id: String,
    /// Compression algorithms the initiator supports (empty = none).
    pub supported_compression: Vec<CompressionAlgorithm>,
    /// FEC modes the initiator supports (empty = none / raw).
    #[serde(default)]
    pub supported_fec_modes: Vec<FecMode>,
    /// Maidenhead grid locator the initiator announces (empty = not advertised).
    #[serde(default)]
    pub station_grid: String,
    /// Active OTA rate-ladder name (empty = no adaptive OTA advertised).
    #[serde(default)]
    pub profile_name: String,
    /// Fingerprint of the active OTA ladder mapping (0 = none). See `SessionProfile::fingerprint`.
    #[serde(default)]
    pub profile_fingerprint: u64,
    /// Unix-ms creation time, signed, for replay-freshness (0 = not advertised / legacy).
    #[serde(default)]
    pub timestamp_ms: u64,
    /// Ed25519 signature over canonical JSON of the body fields (64 bytes).
    pub signature: Vec<u8>,
}

impl ConReq {
    /// Create and sign a new CONREQ (no grid advertised).
    ///
    /// `signing_key_seed` is the 32-byte Ed25519 seed (private key scalar).
    pub fn create(
        station_id: &str,
        signing_key_seed: &[u8; 32],
        signing_modes: Vec<SigningMode>,
        session_id: &str,
        supported_compression: Vec<CompressionAlgorithm>,
        supported_fec_modes: Vec<FecMode>,
    ) -> Result<Self, HandshakeError> {
        Self::create_with_grid(
            station_id,
            signing_key_seed,
            signing_modes,
            session_id,
            supported_compression,
            supported_fec_modes,
            "",
        )
    }

    /// Create and sign a new CONREQ advertising a Maidenhead grid locator (no OTA profile).
    #[allow(clippy::too_many_arguments)]
    pub fn create_with_grid(
        station_id: &str,
        signing_key_seed: &[u8; 32],
        signing_modes: Vec<SigningMode>,
        session_id: &str,
        supported_compression: Vec<CompressionAlgorithm>,
        supported_fec_modes: Vec<FecMode>,
        station_grid: &str,
    ) -> Result<Self, HandshakeError> {
        Self::create_full(
            station_id,
            signing_key_seed,
            signing_modes,
            session_id,
            supported_compression,
            supported_fec_modes,
            station_grid,
            "",
            0,
            0,
        )
    }

    /// Create and sign a CONREQ advertising the grid AND the active OTA rate-ladder identity
    /// (`profile_name` + `profile_fingerprint`), so the peer can detect a diverged ladder.
    /// `timestamp_ms` is the signed Unix-ms creation time for replay-freshness (0 = not advertised).
    #[allow(clippy::too_many_arguments)]
    pub fn create_full(
        station_id: &str,
        signing_key_seed: &[u8; 32],
        signing_modes: Vec<SigningMode>,
        session_id: &str,
        supported_compression: Vec<CompressionAlgorithm>,
        supported_fec_modes: Vec<FecMode>,
        station_grid: &str,
        profile_name: &str,
        profile_fingerprint: u64,
        timestamp_ms: u64,
    ) -> Result<Self, HandshakeError> {
        let signing_key = SigningKey::from_bytes(signing_key_seed);
        let verifying_key = signing_key.verifying_key();

        let body = ConReqBody {
            station_id: station_id.to_string(),
            pubkey: verifying_key.to_bytes().to_vec(),
            signing_modes: signing_modes.clone(),
            session_id: session_id.to_string(),
            supported_compression: supported_compression.clone(),
            supported_fec_modes: supported_fec_modes.clone(),
            station_grid: station_grid.to_string(),
            profile_name: profile_name.to_string(),
            profile_fingerprint,
            timestamp_ms,
        };
        let canonical =
            serde_json::to_vec(&body).map_err(|e| HandshakeError::Encoding(e.to_string()))?;
        let sig: Signature = signing_key.sign(&canonical);

        Ok(Self {
            station_id: station_id.to_string(),
            pubkey: verifying_key.to_bytes().to_vec(),
            signing_modes,
            session_id: session_id.to_string(),
            supported_compression,
            supported_fec_modes,
            station_grid: station_grid.to_string(),
            profile_name: profile_name.to_string(),
            profile_fingerprint,
            timestamp_ms,
            signature: sig.to_bytes().to_vec(),
        })
    }

    fn canonical_bytes(&self) -> Result<Vec<u8>, HandshakeError> {
        let body = ConReqBody {
            station_id: self.station_id.clone(),
            pubkey: self.pubkey.clone(),
            signing_modes: self.signing_modes.clone(),
            session_id: self.session_id.clone(),
            supported_compression: self.supported_compression.clone(),
            supported_fec_modes: self.supported_fec_modes.clone(),
            station_grid: self.station_grid.clone(),
            profile_name: self.profile_name.clone(),
            profile_fingerprint: self.profile_fingerprint,
            timestamp_ms: self.timestamp_ms,
        };
        serde_json::to_vec(&body).map_err(|e| HandshakeError::Encoding(e.to_string()))
    }

    /// Serialize to HSCQ wire frame: magic(4) + version(1) + length(4) + JSON.
    pub fn encode(&self) -> Result<Vec<u8>, ModemError> {
        let body = serde_json::to_vec(self)
            .map_err(|e| ModemError::Frame(format!("CONREQ encode failed: {e}")))?;
        let len = u32::try_from(body.len())
            .map_err(|_| ModemError::Frame("CONREQ too large".to_string()))?;
        let mut out = Vec::with_capacity(4 + 1 + 4 + body.len());
        out.extend_from_slice(b"HSCQ");
        out.push(1u8);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&body);
        Ok(out)
    }

    /// Deserialize from a HSCQ wire frame.
    pub fn decode(bytes: &[u8]) -> Result<Self, ModemError> {
        let min = 9;
        if bytes.len() < min {
            return Err(ModemError::Frame("CONREQ too short".to_string()));
        }
        if &bytes[..4] != b"HSCQ" {
            return Err(ModemError::Frame("invalid CONREQ magic".to_string()));
        }
        if bytes[4] != 1 {
            return Err(ModemError::Frame(format!(
                "unsupported CONREQ version {}",
                bytes[4]
            )));
        }
        let body_len = u32::from_be_bytes([bytes[5], bytes[6], bytes[7], bytes[8]]) as usize;
        if bytes.len() != min + body_len {
            return Err(ModemError::Frame("CONREQ length mismatch".to_string()));
        }
        serde_json::from_slice(&bytes[min..])
            .map_err(|e| ModemError::Frame(format!("CONREQ decode failed: {e}")))
    }
}

// ------------------------------------------------------------------
// ConAck — connection acknowledgment frame
// ------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConAckBody {
    station_id: String,
    pubkey: Vec<u8>,
    selected_mode: SigningMode,
    session_id: String,
    selected_compression: CompressionAlgorithm,
    #[serde(default, skip_serializing_if = "fec_mode_is_none")]
    selected_fec_mode: FecMode,
    // Empty grid is skipped so legacy zero-grid frames (and their signatures) are byte-identical.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    station_grid: String,
    // Responder's active OTA rate-ladder identity; skipped when unset for signature compatibility.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    profile_name: String,
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    profile_fingerprint: u64,
    // Unix-ms creation time, signed, for replay-freshness. Skipped when 0 for signature compatibility.
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    timestamp_ms: u64,
}

fn fec_mode_is_none(m: &FecMode) -> bool {
    *m == FecMode::None
}

fn u64_is_zero(v: &u64) -> bool {
    *v == 0
}

/// Connection acknowledgment sent by the responder during Discovery.
///
/// The `signature` covers the canonical JSON of the body fields, signed with
/// the responder's Ed25519 private key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConAck {
    pub station_id: String,
    /// Ed25519 verifying-key bytes (32 bytes).
    pub pubkey: Vec<u8>,
    pub selected_mode: SigningMode,
    /// Must echo the session_id from the corresponding ConReq.
    pub session_id: String,
    /// Compression algorithm selected for this session.
    pub selected_compression: CompressionAlgorithm,
    /// FEC mode selected for this session.
    #[serde(default)]
    pub selected_fec_mode: FecMode,
    /// Maidenhead grid locator the responder announces (empty = not advertised).
    #[serde(default)]
    pub station_grid: String,
    /// Responder's active OTA rate-ladder name (empty = no adaptive OTA advertised).
    #[serde(default)]
    pub profile_name: String,
    /// Fingerprint of the responder's active OTA ladder mapping (0 = none).
    #[serde(default)]
    pub profile_fingerprint: u64,
    /// Unix-ms creation time, signed, for replay-freshness (0 = not advertised / legacy).
    #[serde(default)]
    pub timestamp_ms: u64,
    /// Ed25519 signature over canonical JSON of the body fields (64 bytes).
    pub signature: Vec<u8>,
}

impl ConAck {
    /// Create and sign a new CONACK in response to `req` (no grid advertised).
    pub fn create(
        station_id: &str,
        signing_key_seed: &[u8; 32],
        selected_mode: SigningMode,
        session_id: &str,
        selected_compression: CompressionAlgorithm,
        selected_fec_mode: FecMode,
    ) -> Result<Self, HandshakeError> {
        Self::create_with_grid(
            station_id,
            signing_key_seed,
            selected_mode,
            session_id,
            selected_compression,
            selected_fec_mode,
            "",
        )
    }

    /// Create and sign a new CONACK advertising a Maidenhead grid locator (no OTA profile).
    #[allow(clippy::too_many_arguments)]
    pub fn create_with_grid(
        station_id: &str,
        signing_key_seed: &[u8; 32],
        selected_mode: SigningMode,
        session_id: &str,
        selected_compression: CompressionAlgorithm,
        selected_fec_mode: FecMode,
        station_grid: &str,
    ) -> Result<Self, HandshakeError> {
        Self::create_full(
            station_id,
            signing_key_seed,
            selected_mode,
            session_id,
            selected_compression,
            selected_fec_mode,
            station_grid,
            "",
            0,
            0,
        )
    }

    /// Create and sign a CONACK advertising the grid AND the responder's active OTA rate-ladder
    /// identity (`profile_name` + `profile_fingerprint`). `timestamp_ms` is the signed Unix-ms
    /// creation time for replay-freshness (0 = not advertised).
    #[allow(clippy::too_many_arguments)]
    pub fn create_full(
        station_id: &str,
        signing_key_seed: &[u8; 32],
        selected_mode: SigningMode,
        session_id: &str,
        selected_compression: CompressionAlgorithm,
        selected_fec_mode: FecMode,
        station_grid: &str,
        profile_name: &str,
        profile_fingerprint: u64,
        timestamp_ms: u64,
    ) -> Result<Self, HandshakeError> {
        let signing_key = SigningKey::from_bytes(signing_key_seed);
        let verifying_key = signing_key.verifying_key();

        let body = ConAckBody {
            station_id: station_id.to_string(),
            pubkey: verifying_key.to_bytes().to_vec(),
            selected_mode,
            session_id: session_id.to_string(),
            selected_compression,
            selected_fec_mode,
            station_grid: station_grid.to_string(),
            profile_name: profile_name.to_string(),
            profile_fingerprint,
            timestamp_ms,
        };
        let canonical =
            serde_json::to_vec(&body).map_err(|e| HandshakeError::Encoding(e.to_string()))?;
        let sig: Signature = signing_key.sign(&canonical);

        Ok(Self {
            station_id: station_id.to_string(),
            pubkey: verifying_key.to_bytes().to_vec(),
            selected_mode,
            session_id: session_id.to_string(),
            selected_compression,
            selected_fec_mode,
            station_grid: station_grid.to_string(),
            profile_name: profile_name.to_string(),
            profile_fingerprint,
            timestamp_ms,
            signature: sig.to_bytes().to_vec(),
        })
    }

    fn canonical_bytes(&self) -> Result<Vec<u8>, HandshakeError> {
        let body = ConAckBody {
            station_id: self.station_id.clone(),
            pubkey: self.pubkey.clone(),
            selected_mode: self.selected_mode,
            session_id: self.session_id.clone(),
            selected_compression: self.selected_compression,
            selected_fec_mode: self.selected_fec_mode,
            station_grid: self.station_grid.clone(),
            profile_name: self.profile_name.clone(),
            profile_fingerprint: self.profile_fingerprint,
            timestamp_ms: self.timestamp_ms,
        };
        serde_json::to_vec(&body).map_err(|e| HandshakeError::Encoding(e.to_string()))
    }

    /// Serialize to HSAK wire frame: magic(4) + version(1) + length(4) + JSON.
    pub fn encode(&self) -> Result<Vec<u8>, ModemError> {
        let body = serde_json::to_vec(self)
            .map_err(|e| ModemError::Frame(format!("CONACK encode failed: {e}")))?;
        let len = u32::try_from(body.len())
            .map_err(|_| ModemError::Frame("CONACK too large".to_string()))?;
        let mut out = Vec::with_capacity(4 + 1 + 4 + body.len());
        out.extend_from_slice(b"HSAK");
        out.push(1u8);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&body);
        Ok(out)
    }

    /// Deserialize from a HSAK wire frame.
    pub fn decode(bytes: &[u8]) -> Result<Self, ModemError> {
        let min = 9;
        if bytes.len() < min {
            return Err(ModemError::Frame("CONACK too short".to_string()));
        }
        if &bytes[..4] != b"HSAK" {
            return Err(ModemError::Frame("invalid CONACK magic".to_string()));
        }
        if bytes[4] != 1 {
            return Err(ModemError::Frame(format!(
                "unsupported CONACK version {}",
                bytes[4]
            )));
        }
        let body_len = u32::from_be_bytes([bytes[5], bytes[6], bytes[7], bytes[8]]) as usize;
        if bytes.len() != min + body_len {
            return Err(ModemError::Frame("CONACK length mismatch".to_string()));
        }
        serde_json::from_slice(&bytes[min..])
            .map_err(|e| ModemError::Frame(format!("CONACK decode failed: {e}")))
    }
}

// ------------------------------------------------------------------
// Verification helpers
// ------------------------------------------------------------------

fn verify_ed25519(pubkey_bytes: &[u8], message: &[u8], sig_bytes: &[u8]) -> bool {
    let Ok(pubkey_arr): Result<[u8; 32], _> = pubkey_bytes.try_into() else {
        return false;
    };
    let Ok(sig_arr): Result<[u8; 64], _> = sig_bytes.try_into() else {
        return false;
    };
    let Ok(key) = VerifyingKey::from_bytes(&pubkey_arr) else {
        return false;
    };
    let sig = Signature::from_bytes(&sig_arr);
    key.verify(message, &sig).is_ok()
}

fn cert_source_for_trust(trust_level: PublicKeyTrustLevel) -> CertificateSource {
    match trust_level {
        PublicKeyTrustLevel::Full => CertificateSource::OutOfBand,
        _ => CertificateSource::OverAir,
    }
}

/// Verify a received CONREQ and evaluate trust.
///
/// Returns `HandshakeDecision` on success.  Fails if the Ed25519 signature is
/// invalid or if the trust policy rejects the peer.
pub fn verify_conreq(
    req: &ConReq,
    trust_store: &dyn TrustStore,
    policy: PolicyProfile,
    local_min_mode: SigningMode,
    freshness: Option<Freshness>,
) -> Result<HandshakeDecision, HandshakeError> {
    let canonical = req.canonical_bytes()?;
    if !verify_ed25519(&req.pubkey, &canonical, &req.signature) {
        return Err(HandshakeError::InvalidSignature);
    }

    // Replay-freshness: the timestamp is inside the signed body, so this check runs after signature
    // verification (an attacker cannot alter it without breaking the signature).
    if let Some(f) = freshness {
        f.check(req.timestamp_ms)?;
    }

    // Bind the in-frame key to the trusted key for this station (mirrors `verify_pq_conreq`). The signature
    // above only proves possession of the *frame's own* key; without this bind, an attacker self-signs a
    // CONREQ claiming a trusted callsign with their own key and is classified at that callsign's trust level.
    bind_frame_key(trust_store, &req.station_id, &req.pubkey)?;

    let key_trust = trust_store.trust_level(&req.station_id);
    let cert_source = cert_source_for_trust(key_trust);

    let decision = evaluate_handshake(
        policy,
        local_min_mode,
        &req.signing_modes,
        key_trust,
        cert_source,
        false,
    )?;

    Ok(decision)
}

/// Require the frame's public key to equal the trust-store key bound to `station_id`, if any. An unknown
/// station has no stored key, so it proceeds at `Unknown` trust (over-air TOFU) — the bind only rejects a
/// frame that *claims a trusted callsign* under a key the operator did not trust for it.
fn bind_frame_key(
    trust_store: &dyn TrustStore,
    station_id: &str,
    frame_pubkey: &[u8],
) -> Result<(), HandshakeError> {
    if let Some(stored) = trust_store.pubkey_for(station_id) {
        let frame_key: [u8; 32] = frame_pubkey
            .try_into()
            .map_err(|_| HandshakeError::InvalidSignature)?;
        if frame_key != stored {
            return Err(HandshakeError::PublicKeyMismatch);
        }
    }
    Ok(())
}

/// Verify a received CONACK and evaluate trust.
///
/// `req_session_id` must match the session ID in the ConAck. `req_supported_compression`
/// is the list advertised by the initiator; the responder's `selected_compression` must
/// appear in that list (or be `None`, which is always allowed). `req_supported_fec_modes`
/// is similarly checked for `selected_fec_mode`. Fails if the signature is invalid, the
/// session ID does not match, compression or FEC mode is not mutually supported, or the
/// trust policy rejects the responder.
#[allow(clippy::too_many_arguments)]
pub fn verify_conack(
    ack: &ConAck,
    req_session_id: &str,
    req_supported_compression: &[CompressionAlgorithm],
    req_supported_fec_modes: &[FecMode],
    trust_store: &dyn TrustStore,
    policy: PolicyProfile,
    local_min_mode: SigningMode,
    freshness: Option<Freshness>,
) -> Result<HandshakeDecision, HandshakeError> {
    if ack.session_id != req_session_id {
        return Err(HandshakeError::SessionIdMismatch {
            expected: req_session_id.to_string(),
            got: ack.session_id.clone(),
        });
    }

    if ack.selected_compression != CompressionAlgorithm::None
        && !req_supported_compression.contains(&ack.selected_compression)
    {
        return Err(HandshakeError::UnsupportedCompression);
    }

    if ack.selected_fec_mode != FecMode::None
        && !req_supported_fec_modes.contains(&ack.selected_fec_mode)
    {
        return Err(HandshakeError::UnsupportedFecMode);
    }

    let canonical = ack.canonical_bytes()?;
    if !verify_ed25519(&ack.pubkey, &canonical, &ack.signature) {
        return Err(HandshakeError::InvalidSignature);
    }

    // Replay-freshness (signed timestamp; checked after signature verification).
    if let Some(f) = freshness {
        f.check(ack.timestamp_ms)?;
    }

    // Bind the in-frame key to the trusted key for this station (see `verify_conreq`).
    bind_frame_key(trust_store, &ack.station_id, &ack.pubkey)?;

    let key_trust = trust_store.trust_level(&ack.station_id);
    let cert_source = cert_source_for_trust(key_trust);

    let decision = evaluate_handshake(
        policy,
        local_min_mode,
        &[ack.selected_mode],
        key_trust,
        cert_source,
        false,
    )?;

    Ok(decision)
}

// ------------------------------------------------------------------
// SHA-256 helper (shared with manifest.rs via pub(crate))
// ------------------------------------------------------------------

pub(crate) fn sha256_bytes(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compression::CompressionAlgorithm;
    use crate::fec::FecMode;

    fn make_key(seed: u8) -> [u8; 32] {
        [seed; 32]
    }

    fn pubkey_for_seed(seed: u8) -> Vec<u8> {
        let sk = SigningKey::from_bytes(&make_key(seed));
        sk.verifying_key().to_bytes().to_vec()
    }

    #[test]
    fn conreq_round_trip() {
        let req = ConReq::create(
            "W1AW",
            &make_key(1),
            vec![SigningMode::Normal],
            "session-abc",
            vec![],
            vec![],
        )
        .unwrap();
        let encoded = req.encode().expect("encode");
        let decoded = ConReq::decode(&encoded).expect("decode");
        assert_eq!(decoded.station_id, req.station_id);
        assert_eq!(decoded.pubkey, req.pubkey);
        assert_eq!(decoded.signature, req.signature);
    }

    #[test]
    fn conreq_signature_covers_content() {
        let req = ConReq::create(
            "W1AW",
            &make_key(1),
            vec![SigningMode::Normal],
            "s1",
            vec![],
            vec![],
        )
        .unwrap();
        let mut tampered = req.clone();
        tampered.station_id = "EVIL".to_string();
        let canonical = tampered.canonical_bytes().unwrap();
        assert!(!verify_ed25519(&req.pubkey, &canonical, &req.signature));
    }

    #[test]
    fn conreq_grid_round_trips_and_is_signature_covered() {
        let req = ConReq::create_with_grid(
            "W1AW",
            &make_key(1),
            vec![SigningMode::Normal],
            "s-grid",
            vec![],
            vec![],
            "FN31pr",
        )
        .unwrap();
        // Survives the wire round-trip.
        let decoded = ConReq::decode(&req.encode().unwrap()).unwrap();
        assert_eq!(decoded.station_grid, "FN31pr");
        // Self-verifies.
        assert!(verify_ed25519(
            &req.pubkey,
            &req.canonical_bytes().unwrap(),
            &req.signature
        ));
        // Tampering the grid breaks the signature (grid is covered).
        let mut tampered = req.clone();
        tampered.station_grid = "JO62".to_string();
        assert!(!verify_ed25519(
            &req.pubkey,
            &tampered.canonical_bytes().unwrap(),
            &req.signature
        ));
    }

    #[test]
    fn conack_grid_round_trips_and_is_signature_covered() {
        let ack = ConAck::create_with_grid(
            "KD9XYZ",
            &make_key(2),
            SigningMode::Normal,
            "s-grid",
            CompressionAlgorithm::None,
            FecMode::None,
            "EM69",
        )
        .unwrap();
        let decoded = ConAck::decode(&ack.encode().unwrap()).unwrap();
        assert_eq!(decoded.station_grid, "EM69");
        let mut tampered = ack.clone();
        tampered.station_grid = "AA00".to_string();
        assert!(!verify_ed25519(
            &ack.pubkey,
            &tampered.canonical_bytes().unwrap(),
            &ack.signature
        ));
    }

    #[test]
    fn empty_grid_conreq_is_byte_identical_to_legacy() {
        // A default-grid frame must serialize identically to the pre-grid wire format
        // so existing signatures and decoders are unaffected.
        let with_helper = ConReq::create(
            "W1AW",
            &make_key(1),
            vec![SigningMode::Normal],
            "s1",
            vec![],
            vec![],
        )
        .unwrap();
        let with_grid_empty = ConReq::create_with_grid(
            "W1AW",
            &make_key(1),
            vec![SigningMode::Normal],
            "s1",
            vec![],
            vec![],
            "",
        )
        .unwrap();
        assert_eq!(
            with_helper.encode().unwrap(),
            with_grid_empty.encode().unwrap()
        );
        assert_eq!(with_helper.signature, with_grid_empty.signature);
    }

    #[test]
    fn conack_round_trip() {
        let ack = ConAck::create(
            "KD9XYZ",
            &make_key(2),
            SigningMode::Normal,
            "session-abc",
            CompressionAlgorithm::None,
            FecMode::None,
        )
        .unwrap();
        let encoded = ack.encode().expect("encode");
        let decoded = ConAck::decode(&encoded).expect("decode");
        assert_eq!(decoded.station_id, ack.station_id);
        assert_eq!(decoded.pubkey, pubkey_for_seed(2));
    }

    #[test]
    fn verify_conreq_rejects_wrong_pubkey() {
        let mut req = ConReq::create(
            "W1AW",
            &make_key(1),
            vec![SigningMode::Normal],
            "s1",
            vec![],
            vec![],
        )
        .unwrap();
        req.pubkey = pubkey_for_seed(99); // wrong key

        let store = InMemoryTrustStore::new();
        let result = verify_conreq(
            &req,
            &store,
            PolicyProfile::Balanced,
            SigningMode::Normal,
            None,
        );
        assert!(matches!(result, Err(HandshakeError::InvalidSignature)));
    }

    fn conreq_at(timestamp_ms: u64) -> ConReq {
        ConReq::create_full(
            "W1AW",
            &make_key(1),
            vec![SigningMode::Normal],
            "s-fresh",
            vec![],
            vec![],
            "",
            "",
            0,
            timestamp_ms,
        )
        .unwrap()
    }

    #[test]
    fn fresh_conreq_within_window_is_accepted() {
        let now = 1_700_000_000_000;
        let req = conreq_at(now - 5_000); // 5 s old
        let store = InMemoryTrustStore::new();
        let f = Freshness {
            now_ms: now,
            max_skew_ms: 120_000,
        };
        assert!(verify_conreq(
            &req,
            &store,
            PolicyProfile::Balanced,
            SigningMode::Normal,
            Some(f)
        )
        .is_ok());
    }

    #[test]
    fn stale_conreq_is_rejected() {
        let now = 1_700_000_000_000;
        let req = conreq_at(now - 10 * 60_000); // 10 min old
        let store = InMemoryTrustStore::new();
        let f = Freshness {
            now_ms: now,
            max_skew_ms: 120_000,
        };
        assert!(matches!(
            verify_conreq(
                &req,
                &store,
                PolicyProfile::Balanced,
                SigningMode::Normal,
                Some(f)
            ),
            Err(HandshakeError::StaleTimestamp { .. })
        ));
    }

    #[test]
    fn future_dated_conreq_is_rejected() {
        let now = 1_700_000_000_000;
        let req = conreq_at(now + 10 * 60_000); // 10 min in the future
        let store = InMemoryTrustStore::new();
        let f = Freshness {
            now_ms: now,
            max_skew_ms: 120_000,
        };
        assert!(matches!(
            verify_conreq(
                &req,
                &store,
                PolicyProfile::Balanced,
                SigningMode::Normal,
                Some(f)
            ),
            Err(HandshakeError::StaleTimestamp { .. })
        ));
    }

    #[test]
    fn timestampless_conreq_rejected_when_freshness_required() {
        let req = conreq_at(0); // legacy / no timestamp
        let store = InMemoryTrustStore::new();
        let f = Freshness {
            now_ms: 1_700_000_000_000,
            max_skew_ms: 120_000,
        };
        assert!(matches!(
            verify_conreq(
                &req,
                &store,
                PolicyProfile::Balanced,
                SigningMode::Normal,
                Some(f)
            ),
            Err(HandshakeError::MissingTimestamp)
        ));
    }

    #[test]
    fn none_freshness_skips_the_check() {
        let req = conreq_at(0); // no timestamp — accepted when freshness is not enforced
        let store = InMemoryTrustStore::new();
        assert!(verify_conreq(
            &req,
            &store,
            PolicyProfile::Balanced,
            SigningMode::Normal,
            None
        )
        .is_ok());
    }

    #[test]
    fn stale_conack_is_rejected() {
        let now = 1_700_000_000_000;
        let ack = ConAck::create_full(
            "W1AW",
            &make_key(1),
            SigningMode::Normal,
            "s-fresh",
            CompressionAlgorithm::None,
            FecMode::None,
            "",
            "",
            0,
            now - 10 * 60_000,
        )
        .unwrap();
        let store = InMemoryTrustStore::new();
        let f = Freshness {
            now_ms: now,
            max_skew_ms: 120_000,
        };
        assert!(matches!(
            verify_conack(
                &ack,
                "s-fresh",
                &[],
                &[],
                &store,
                PolicyProfile::Balanced,
                SigningMode::Normal,
                Some(f)
            ),
            Err(HandshakeError::StaleTimestamp { .. })
        ));
    }
}
