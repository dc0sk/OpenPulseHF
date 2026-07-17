//! Session-key agreement for authenticating the OTA rate-control ACK (E7).
//!
//! The signed handshake proves *who* the peer is, but the tiny FSK4 rate ACK carried no
//! authentication — so any listener who read the cleartext `session_id` from the CONREQ could forge
//! ACKs and drive a link's rate ladder. This module adds an ephemeral **X25519** key agreement whose
//! public keys ride inside the Ed25519-signed CONREQ/CONACK bodies (so a MITM cannot substitute them
//! without breaking the identity signature). Both stations derive the same 32-byte ACK MAC key via
//! ECDH → HKDF-SHA256; the ACK then carries a keyed MAC (see [`crate::ack`]).
//!
//! This is **authentication, not encryption**: the shared secret keys a MAC over ACK content that
//! stays in the clear, so it is compatible with amateur-radio rules that forbid obscuring meaning.

use hkdf::Hkdf;
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};

/// HKDF info string binding the derived key to this purpose and version.
const ACK_KEY_INFO: &[u8] = b"openpulse-ota-ack-key-v1";

/// Generate an ephemeral X25519 keypair. Returns `(secret_bytes, public_bytes)`; the caller advertises
/// the public bytes in its signed handshake frame and keeps the secret to derive the shared key when the
/// peer's public key arrives.
pub fn generate_kex_ephemeral() -> ([u8; 32], [u8; 32]) {
    let secret = StaticSecret::random_from_rng(rand::rngs::OsRng);
    let public = PublicKey::from(&secret);
    (secret.to_bytes(), public.to_bytes())
}

/// Derive the 32-byte OTA-ACK MAC key from this station's ephemeral secret and the peer's ephemeral
/// public key (X25519 ECDH → HKDF-SHA256). Both ends compute the identical key.
pub fn derive_ack_key(my_secret: &[u8; 32], peer_public: &[u8; 32]) -> [u8; 32] {
    let secret = StaticSecret::from(*my_secret);
    let peer = PublicKey::from(*peer_public);
    let shared = secret.diffie_hellman(&peer);
    let hk = Hkdf::<Sha256>::new(None, shared.as_bytes());
    let mut okm = [0u8; 32];
    // expand into a fixed 32-byte buffer never fails for this length.
    hk.expand(ACK_KEY_INFO, &mut okm)
        .expect("HKDF expand of 32 bytes is infallible");
    okm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_sides_derive_the_same_key() {
        let (sk_a, pk_a) = generate_kex_ephemeral();
        let (sk_b, pk_b) = generate_kex_ephemeral();
        let key_a = derive_ack_key(&sk_a, &pk_b);
        let key_b = derive_ack_key(&sk_b, &pk_a);
        assert_eq!(key_a, key_b, "ECDH must agree on both ends");
        assert_ne!(key_a, [0u8; 32], "derived key must not be all-zero");
    }

    #[test]
    fn distinct_pairs_derive_distinct_keys() {
        let (sk_a, pk_a) = generate_kex_ephemeral();
        let (sk_b, pk_b) = generate_kex_ephemeral();
        let (_sk_c, pk_c) = generate_kex_ephemeral();
        let key_ab = derive_ack_key(&sk_a, &pk_b);
        let key_ac = derive_ack_key(&sk_a, &pk_c);
        assert_ne!(key_ab, key_ac, "A↔B and A↔C must not share a key");
        // B's own view of the A↔B pair is the same key, and is still distinct from A↔C — so the
        // separation above is a property of the *pair*, not an artefact of deriving from A's side.
        assert_eq!(derive_ack_key(&sk_b, &pk_a), key_ab);
        assert_ne!(derive_ack_key(&sk_b, &pk_a), key_ac);
        // A third party's key with B does not match A↔B.
        let (sk_x, _pk_x) = generate_kex_ephemeral();
        assert_ne!(derive_ack_key(&sk_x, &pk_b), key_ab);
    }
}
