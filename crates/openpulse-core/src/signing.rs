//! Thin public wrappers around Ed25519 sign / verify primitives.
//!
//! These exist so consumer crates can authenticate arbitrary byte slices
//! without taking a direct dependency on `ed25519-dalek`.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

/// Sign `message` with the Ed25519 key derived from `seed` and return the
/// 64-byte signature.
pub fn sign_bytes(seed: &[u8; 32], message: &[u8]) -> [u8; 64] {
    let key = SigningKey::from_bytes(seed);
    let sig: Signature = key.sign(message);
    sig.to_bytes()
}

/// Verify an Ed25519 `signature` over `message` against `pubkey`.
///
/// Returns `true` when the signature is valid.
pub fn verify_bytes(pubkey: &[u8; 32], message: &[u8], signature: &[u8; 64]) -> bool {
    let Ok(vk) = VerifyingKey::from_bytes(pubkey) else {
        return false;
    };
    let sig = Signature::from_bytes(signature);
    vk.verify(message, &sig).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let seed = [0x42u8; 32];
        let key = SigningKey::from_bytes(&seed);
        let pubkey: [u8; 32] = key.verifying_key().to_bytes();
        let msg = b"hello openPulseHF";
        let sig = sign_bytes(&seed, msg);
        assert!(verify_bytes(&pubkey, msg, &sig));
    }

    #[test]
    fn tampered_message_fails() {
        let seed = [0x07u8; 32];
        let key = SigningKey::from_bytes(&seed);
        let pubkey: [u8; 32] = key.verifying_key().to_bytes();
        let sig = sign_bytes(&seed, b"original");
        assert!(!verify_bytes(&pubkey, b"tampered", &sig));
    }

    #[test]
    fn wrong_key_fails() {
        let seed_a = [0x01u8; 32];
        let seed_b = [0x02u8; 32];
        let pubkey_b: [u8; 32] = SigningKey::from_bytes(&seed_b).verifying_key().to_bytes();
        let sig = sign_bytes(&seed_a, b"message");
        assert!(!verify_bytes(&pubkey_b, b"message", &sig));
    }
}
