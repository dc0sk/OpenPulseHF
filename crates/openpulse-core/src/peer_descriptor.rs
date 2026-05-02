use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::handshake::sha256_bytes;

// ------------------------------------------------------------------
// Errors
// ------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum PeerDescriptorError {
    #[error("invalid descriptor signature")]
    InvalidSignature,
    #[error("invalid peer_id: {0}")]
    InvalidKey(String),
    #[error("encoding error: {0}")]
    Encoding(String),
}

// ------------------------------------------------------------------
// PeerDescriptor
// ------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PeerDescriptorBody {
    peer_id: Vec<u8>,
    callsign: String,
    capability_mask: u32,
    timestamp_ms: u64,
}

/// Signed identity descriptor that a peer broadcasts during discovery.
///
/// The `peer_id` doubles as the Ed25519 verifying-key bytes so no
/// external key lookup is required to verify the signature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerDescriptor {
    /// Ed25519 verifying-key bytes (32 bytes) — the peer's stable ID.
    pub peer_id: [u8; 32],
    /// Callsign / station ID of the peer.
    pub callsign: String,
    /// Bitfield of declared peer capabilities (application-defined).
    pub capability_mask: u32,
    /// Unix epoch milliseconds when the descriptor was generated.
    pub timestamp_ms: u64,
    /// Ed25519 signature (64 bytes) over canonical JSON of the body fields.
    pub signature: Vec<u8>,
}

impl PeerDescriptor {
    /// Create and sign a new descriptor with the given seed key.
    pub fn sign(
        callsign: &str,
        capability_mask: u32,
        timestamp_ms: u64,
        signing_key_seed: &[u8; 32],
    ) -> Result<Self, PeerDescriptorError> {
        let signing_key = SigningKey::from_bytes(signing_key_seed);
        let peer_id = signing_key.verifying_key().to_bytes();

        let body = PeerDescriptorBody {
            peer_id: peer_id.to_vec(),
            callsign: callsign.to_string(),
            capability_mask,
            timestamp_ms,
        };
        let canonical =
            serde_json::to_vec(&body).map_err(|e| PeerDescriptorError::Encoding(e.to_string()))?;
        let sig: Signature = signing_key.sign(&canonical);

        Ok(Self {
            peer_id,
            callsign: callsign.to_string(),
            capability_mask,
            timestamp_ms,
            signature: sig.to_bytes().to_vec(),
        })
    }

    /// SHA-256 of the callsign, for use in query response result entries.
    pub fn callsign_hash(&self) -> [u8; 32] {
        sha256_bytes(self.callsign.as_bytes())
    }

    fn canonical_bytes(&self) -> Result<Vec<u8>, PeerDescriptorError> {
        let body = PeerDescriptorBody {
            peer_id: self.peer_id.to_vec(),
            callsign: self.callsign.clone(),
            capability_mask: self.capability_mask,
            timestamp_ms: self.timestamp_ms,
        };
        serde_json::to_vec(&body).map_err(|e| PeerDescriptorError::Encoding(e.to_string()))
    }
}

/// Verify a peer descriptor against its embedded `peer_id` (the verifying key).
///
/// Returns `Ok(())` if the signature is valid.
pub fn verify_peer_descriptor(desc: &PeerDescriptor) -> Result<(), PeerDescriptorError> {
    let key = VerifyingKey::from_bytes(&desc.peer_id)
        .map_err(|e| PeerDescriptorError::InvalidKey(e.to_string()))?;

    let Ok(sig_arr): Result<[u8; 64], _> = desc.signature.as_slice().try_into() else {
        return Err(PeerDescriptorError::InvalidSignature);
    };
    let sig = Signature::from_bytes(&sig_arr);

    let canonical = desc.canonical_bytes()?;
    key.verify(&canonical, &sig)
        .map_err(|_| PeerDescriptorError::InvalidSignature)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(b: u8) -> [u8; 32] {
        [b; 32]
    }

    #[test]
    fn sign_and_verify_round_trip() {
        let desc = PeerDescriptor::sign("W1AW", 0x0001, 1_000, &seed(1)).unwrap();
        verify_peer_descriptor(&desc).expect("valid descriptor must verify");
    }

    #[test]
    fn tampered_callsign_rejected() {
        let mut desc = PeerDescriptor::sign("W1AW", 0x0001, 1_000, &seed(1)).unwrap();
        desc.callsign = "EVIL".to_string();
        assert!(matches!(
            verify_peer_descriptor(&desc),
            Err(PeerDescriptorError::InvalidSignature)
        ));
    }

    #[test]
    fn tampered_capability_mask_rejected() {
        let mut desc = PeerDescriptor::sign("W1AW", 0x0001, 1_000, &seed(1)).unwrap();
        desc.capability_mask = 0xFFFF;
        assert!(matches!(
            verify_peer_descriptor(&desc),
            Err(PeerDescriptorError::InvalidSignature)
        ));
    }

    #[test]
    fn callsign_hash_is_deterministic() {
        let desc = PeerDescriptor::sign("W1AW", 0x0001, 1_000, &seed(1)).unwrap();
        assert_eq!(desc.callsign_hash(), desc.callsign_hash());
    }
}
