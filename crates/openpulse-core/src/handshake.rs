use std::collections::HashMap;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::compression::CompressionAlgorithm;
use crate::error::ModemError;
use crate::trust::{
    evaluate_handshake, CertificateSource, HandshakeDecision, PolicyProfile, PublicKeyTrustLevel,
    SigningMode, TrustError,
};

// ------------------------------------------------------------------
// Errors
// ------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum HandshakeError {
    #[error("invalid Ed25519 signature")]
    InvalidSignature,
    #[error("session ID mismatch: expected {expected}, got {got}")]
    SessionIdMismatch { expected: String, got: String },
    #[error("trust evaluation failed: {0:?}")]
    TrustFailure(TrustError),
    #[error("encoding error: {0}")]
    Encoding(String),
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
    /// Ed25519 signature over canonical JSON of the body fields (64 bytes).
    pub signature: Vec<u8>,
}

impl ConReq {
    /// Create and sign a new CONREQ.
    ///
    /// `signing_key_seed` is the 32-byte Ed25519 seed (private key scalar).
    pub fn create(
        station_id: &str,
        signing_key_seed: &[u8; 32],
        signing_modes: Vec<SigningMode>,
        session_id: &str,
        supported_compression: Vec<CompressionAlgorithm>,
    ) -> Result<Self, HandshakeError> {
        let signing_key = SigningKey::from_bytes(signing_key_seed);
        let verifying_key = signing_key.verifying_key();

        let body = ConReqBody {
            station_id: station_id.to_string(),
            pubkey: verifying_key.to_bytes().to_vec(),
            signing_modes: signing_modes.clone(),
            session_id: session_id.to_string(),
            supported_compression: supported_compression.clone(),
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
        };
        serde_json::to_vec(&body).map_err(|e| HandshakeError::Encoding(e.to_string()))
    }

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
    /// Ed25519 signature over canonical JSON of the body fields (64 bytes).
    pub signature: Vec<u8>,
}

impl ConAck {
    /// Create and sign a new CONACK in response to `req`.
    pub fn create(
        station_id: &str,
        signing_key_seed: &[u8; 32],
        selected_mode: SigningMode,
        session_id: &str,
        selected_compression: CompressionAlgorithm,
    ) -> Result<Self, HandshakeError> {
        let signing_key = SigningKey::from_bytes(signing_key_seed);
        let verifying_key = signing_key.verifying_key();

        let body = ConAckBody {
            station_id: station_id.to_string(),
            pubkey: verifying_key.to_bytes().to_vec(),
            selected_mode,
            session_id: session_id.to_string(),
            selected_compression,
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
        };
        serde_json::to_vec(&body).map_err(|e| HandshakeError::Encoding(e.to_string()))
    }

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
) -> Result<HandshakeDecision, HandshakeError> {
    let canonical = req.canonical_bytes()?;
    if !verify_ed25519(&req.pubkey, &canonical, &req.signature) {
        return Err(HandshakeError::InvalidSignature);
    }

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

/// Verify a received CONACK and evaluate trust.
///
/// `req_session_id` must match the session ID in the ConAck.  Fails if the
/// signature is invalid, the session ID does not match, or the trust policy
/// rejects the responder.
pub fn verify_conack(
    ack: &ConAck,
    req_session_id: &str,
    trust_store: &dyn TrustStore,
    policy: PolicyProfile,
    local_min_mode: SigningMode,
) -> Result<HandshakeDecision, HandshakeError> {
    if ack.session_id != req_session_id {
        return Err(HandshakeError::SessionIdMismatch {
            expected: req_session_id.to_string(),
            got: ack.session_id.clone(),
        });
    }

    let canonical = ack.canonical_bytes()?;
    if !verify_ed25519(&ack.pubkey, &canonical, &ack.signature) {
        return Err(HandshakeError::InvalidSignature);
    }

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
        )
        .unwrap();
        let mut tampered = req.clone();
        tampered.station_id = "EVIL".to_string();
        let canonical = tampered.canonical_bytes().unwrap();
        assert!(!verify_ed25519(&req.pubkey, &canonical, &req.signature));
    }

    #[test]
    fn conack_round_trip() {
        let ack = ConAck::create(
            "KD9XYZ",
            &make_key(2),
            SigningMode::Normal,
            "session-abc",
            CompressionAlgorithm::None,
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
        )
        .unwrap();
        req.pubkey = pubkey_for_seed(99); // wrong key

        let store = InMemoryTrustStore::new();
        let result = verify_conreq(&req, &store, PolicyProfile::Balanced, SigningMode::Normal);
        assert!(matches!(result, Err(HandshakeError::InvalidSignature)));
    }
}
