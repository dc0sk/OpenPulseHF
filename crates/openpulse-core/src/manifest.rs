use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::handshake::sha256_bytes;

// ------------------------------------------------------------------
// Errors
// ------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("invalid manifest signature")]
    InvalidSignature,
    #[error("invalid signing key")]
    InvalidKey,
    #[error("encoding error: {0}")]
    Encoding(String),
}

// ------------------------------------------------------------------
// TransferManifest
// ------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestBody {
    payload_hash: Vec<u8>,
    payload_size: u64,
    sender_id: String,
}

/// Signed summary of a completed transfer, exchanged during Teardown.
///
/// Both peers compute and sign a manifest over the transferred payload.  The
/// receiver verifies the sender's signature before issuing a final delivery
/// acknowledgement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferManifest {
    /// SHA-256 of the complete transferred payload (all reassembled bytes).
    pub payload_hash: Vec<u8>,
    /// Total transferred payload size in bytes.
    pub payload_size: u64,
    /// Callsign / station ID of the sender.
    pub sender_id: String,
    /// Ed25519 signature over canonical JSON of the body fields (64 bytes).
    pub signature: Vec<u8>,
}

impl TransferManifest {
    /// Compute the SHA-256 of `payload` and sign it with `signing_key_seed`.
    pub fn sign(
        payload: &[u8],
        sender_id: &str,
        signing_key_seed: &[u8; 32],
    ) -> Result<Self, ManifestError> {
        let payload_hash = sha256_bytes(payload).to_vec();
        let payload_size = payload.len() as u64;

        let body = ManifestBody {
            payload_hash: payload_hash.clone(),
            payload_size,
            sender_id: sender_id.to_string(),
        };
        let canonical =
            serde_json::to_vec(&body).map_err(|e| ManifestError::Encoding(e.to_string()))?;

        let signing_key = SigningKey::from_bytes(signing_key_seed);
        let sig: Signature = signing_key.sign(&canonical);

        Ok(Self {
            payload_hash,
            payload_size,
            sender_id: sender_id.to_string(),
            signature: sig.to_bytes().to_vec(),
        })
    }

    fn canonical_bytes(&self) -> Result<Vec<u8>, ManifestError> {
        let body = ManifestBody {
            payload_hash: self.payload_hash.clone(),
            payload_size: self.payload_size,
            sender_id: self.sender_id.clone(),
        };
        serde_json::to_vec(&body).map_err(|e| ManifestError::Encoding(e.to_string()))
    }
}

/// Verify a received manifest against the sender's Ed25519 public key.
///
/// Returns `Ok(())` if the signature is valid.  The caller is responsible for
/// also verifying that `manifest.payload_hash` matches the locally computed
/// hash of the received data.
pub fn verify_manifest(
    manifest: &TransferManifest,
    pubkey_bytes: &[u8; 32],
) -> Result<(), ManifestError> {
    let key = VerifyingKey::from_bytes(pubkey_bytes).map_err(|_| ManifestError::InvalidKey)?;

    let Ok(sig_arr): Result<[u8; 64], _> = manifest.signature.as_slice().try_into() else {
        return Err(ManifestError::InvalidSignature);
    };
    let sig = Signature::from_bytes(&sig_arr);

    let canonical = manifest.canonical_bytes()?;
    key.verify(&canonical, &sig)
        .map_err(|_| ManifestError::InvalidSignature)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_seed(b: u8) -> [u8; 32] {
        [b; 32]
    }

    fn pubkey_for(seed: u8) -> [u8; 32] {
        SigningKey::from_bytes(&make_seed(seed))
            .verifying_key()
            .to_bytes()
    }

    #[test]
    fn manifest_sign_and_verify() {
        let payload = b"hello from W1AW";
        let m = TransferManifest::sign(payload, "W1AW", &make_seed(3)).unwrap();
        assert_eq!(m.payload_size, payload.len() as u64);
        verify_manifest(&m, &pubkey_for(3)).expect("valid manifest");
    }

    #[test]
    fn manifest_rejects_wrong_pubkey() {
        let m = TransferManifest::sign(b"data", "W1AW", &make_seed(3)).unwrap();
        let result = verify_manifest(&m, &pubkey_for(4)); // wrong key
        assert!(matches!(result, Err(ManifestError::InvalidSignature)));
    }

    #[test]
    fn manifest_rejects_tampered_hash() {
        let mut m = TransferManifest::sign(b"data", "W1AW", &make_seed(3)).unwrap();
        m.payload_hash[0] ^= 0xff; // corrupt the hash
        let result = verify_manifest(&m, &pubkey_for(3));
        assert!(matches!(result, Err(ManifestError::InvalidSignature)));
    }
}
