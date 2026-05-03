use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};

#[derive(Debug, thiserror::Error)]
pub enum VerificationError {
    #[error("payload must contain a 'pubkey' field (base64-encoded Ed25519 verifying key)")]
    MissingPublicKey,
    #[error("pubkey field is not valid base64")]
    InvalidPublicKeyEncoding,
    #[error("pubkey is not a valid Ed25519 verifying key")]
    InvalidPublicKey,
    #[error("detached_signature is not valid base64")]
    InvalidSignatureEncoding,
    #[error("detached_signature must be exactly 64 bytes when decoded")]
    InvalidSignatureLength,
    #[error("signature verification failed")]
    InvalidSignature,
}

/// Verify an Ed25519 detached signature over the canonical JSON bytes of `payload`.
///
/// The `pubkey` field inside `payload` must be a base64-encoded 32-byte Ed25519 verifying key.
/// `detached_signature_b64` must be a base64-encoded 64-byte Ed25519 signature over
/// `serde_json::to_vec(payload)`.
///
/// Returns the verified 32-byte public key on success.
pub fn verify_submission_signature(
    payload: &serde_json::Value,
    detached_signature_b64: &str,
) -> Result<[u8; 32], VerificationError> {
    let pubkey_b64 = payload
        .get("pubkey")
        .and_then(|v| v.as_str())
        .ok_or(VerificationError::MissingPublicKey)?;

    let pubkey_bytes = STANDARD
        .decode(pubkey_b64)
        .map_err(|_| VerificationError::InvalidPublicKeyEncoding)?;
    let pubkey_arr: [u8; 32] = pubkey_bytes
        .as_slice()
        .try_into()
        .map_err(|_| VerificationError::InvalidPublicKey)?;
    let verifying_key =
        VerifyingKey::from_bytes(&pubkey_arr).map_err(|_| VerificationError::InvalidPublicKey)?;

    let sig_bytes = STANDARD
        .decode(detached_signature_b64)
        .map_err(|_| VerificationError::InvalidSignatureEncoding)?;
    let sig_arr: [u8; 64] = sig_bytes
        .as_slice()
        .try_into()
        .map_err(|_| VerificationError::InvalidSignatureLength)?;
    let signature = Signature::from_bytes(&sig_arr);

    let canonical = serde_json::to_vec(payload).expect("serde_json::Value is always serializable");

    verifying_key
        .verify(&canonical, &signature)
        .map_err(|_| VerificationError::InvalidSignature)?;

    Ok(pubkey_arr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::json;

    fn make_payload_and_sig(seed: u8) -> (serde_json::Value, String) {
        let sk = SigningKey::from_bytes(&[seed; 32]);
        let vk = sk.verifying_key();
        let pubkey_b64 = STANDARD.encode(vk.to_bytes());

        let payload = json!({
            "pubkey": pubkey_b64,
            "session_id": "test-session",
            "signed_at": "2026-01-01T00:00:00Z",
        });

        let canonical = serde_json::to_vec(&payload).unwrap();
        let sig: Signature = sk.sign(&canonical);
        let sig_b64 = STANDARD.encode(sig.to_bytes());

        (payload, sig_b64)
    }

    #[test]
    fn valid_signature_verifies() {
        let (payload, sig_b64) = make_payload_and_sig(1);
        assert!(verify_submission_signature(&payload, &sig_b64).is_ok());
    }

    #[test]
    fn corrupted_signature_rejected() {
        let (payload, mut sig_b64) = make_payload_and_sig(2);
        // Flip last char to corrupt the base64
        let last = sig_b64.pop().unwrap();
        sig_b64.push(if last == 'A' { 'B' } else { 'A' });
        assert!(matches!(
            verify_submission_signature(&payload, &sig_b64),
            Err(
                VerificationError::InvalidSignature
                    | VerificationError::InvalidSignatureEncoding
                    | VerificationError::InvalidSignatureLength
            )
        ));
    }

    #[test]
    fn wrong_length_signature_rejected() {
        let (payload, _) = make_payload_and_sig(5);
        // 48 bytes encoded as base64 is valid base64 but not 64 bytes when decoded
        let short_sig_b64 = STANDARD.encode([0u8; 48]);
        assert!(matches!(
            verify_submission_signature(&payload, &short_sig_b64),
            Err(VerificationError::InvalidSignatureLength)
        ));
    }

    #[test]
    fn wrong_pubkey_rejected() {
        let (mut payload, sig_b64) = make_payload_and_sig(3);
        // Replace pubkey with a different key
        let other_vk = SigningKey::from_bytes(&[99u8; 32]).verifying_key();
        payload["pubkey"] = json!(STANDARD.encode(other_vk.to_bytes()));
        assert!(matches!(
            verify_submission_signature(&payload, &sig_b64),
            Err(VerificationError::InvalidSignature)
        ));
    }

    #[test]
    fn missing_pubkey_field_rejected() {
        let payload = json!({ "session_id": "s1" });
        assert!(matches!(
            verify_submission_signature(&payload, "AAAA"),
            Err(VerificationError::MissingPublicKey)
        ));
    }
}
