//! The single choke point through which the station identity key signs and verifies.
//!
//! Every signature the station makes goes through one of the four entry points below, each of which
//! binds the message to a [`SigningDomain`]. A `clippy.toml` `disallowed-methods` wall refuses raw
//! `ed25519_dalek` sign/verify calls everywhere else in the workspace, so an unregistered signing
//! site fails the lint gate rather than silently voiding domain separation for every other context.
//!
//! That wall is the load-bearing part. A registry plus a test governs only the sites that volunteer
//! for it; the wall governs the ones that do not.

use crate::signing_domain::{SigningDomain, TagPlacement};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use thiserror::Error;

/// Why a message could not be bound to its signing domain.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SigningError {
    /// An in-band domain's message did not begin with that domain's transmitted magic.
    #[error("message for signing domain {domain} does not begin with its in-band tag")]
    MissingInBandTag {
        /// The domain whose tag was expected.
        domain: SigningDomain,
    },
    /// An in-band entry point was used for a prepended domain, or the reverse.
    #[error("signing domain {domain} uses {expected:?} placement; wrong entry point called")]
    WrongPlacement {
        /// The domain that was asked for.
        domain: SigningDomain,
        /// The placement that domain actually uses.
        expected: TagPlacement,
    },
}

/// Build the signed message for a **prepended** domain: `tag || version || payload`.
///
/// The prefix is never transmitted, so this costs no airtime — only the signature value changes.
fn prepend_domain(domain: SigningDomain, payload: &[u8]) -> Result<Vec<u8>, SigningError> {
    if domain.placement() != TagPlacement::Prepended {
        return Err(SigningError::WrongPlacement {
            domain,
            expected: domain.placement(),
        });
    }
    let mut out = Vec::with_capacity(5 + payload.len());
    out.extend_from_slice(domain.tag());
    out.push(domain.version());
    out.extend_from_slice(payload);
    Ok(out)
}

/// Confirm an **in-band** domain's message already begins with that domain's tag.
fn check_in_band(domain: SigningDomain, message: &[u8]) -> Result<(), SigningError> {
    if domain.placement() != TagPlacement::InBand {
        return Err(SigningError::WrongPlacement {
            domain,
            expected: domain.placement(),
        });
    }
    if !message.starts_with(domain.tag()) {
        return Err(SigningError::MissingInBandTag { domain });
    }
    Ok(())
}

/// Sign `payload` in a **prepended** domain.
pub fn sign_in_domain(
    domain: SigningDomain,
    seed: &[u8; 32],
    payload: &[u8],
) -> Result<[u8; 64], SigningError> {
    let msg = prepend_domain(domain, payload)?;
    Ok(raw_sign(seed, &msg))
}

/// Verify a signature made by [`sign_in_domain`]. Any domain or key error verifies as `false`.
pub fn verify_in_domain(
    domain: SigningDomain,
    pubkey: &[u8; 32],
    payload: &[u8],
    signature: &[u8; 64],
) -> bool {
    match prepend_domain(domain, payload) {
        Ok(msg) => raw_verify(pubkey, &msg, signature),
        Err(_) => false,
    }
}

/// Sign a message that already carries its domain tag in-band.
pub fn sign_in_band(
    domain: SigningDomain,
    seed: &[u8; 32],
    message: &[u8],
) -> Result<[u8; 64], SigningError> {
    check_in_band(domain, message)?;
    Ok(raw_sign(seed, message))
}

/// Verify a message that carries its domain tag in-band. A missing tag verifies as `false`.
pub fn verify_in_band(
    domain: SigningDomain,
    pubkey: &[u8; 32],
    message: &[u8],
    signature: &[u8; 64],
) -> bool {
    check_in_band(domain, message).is_ok() && raw_verify(pubkey, message, signature)
}

// The only two places in the workspace that touch the Ed25519 primitives directly. Everything else
// reaches them through a domain-bound entry point above; `clippy.toml` enforces that.
#[allow(clippy::disallowed_methods)]
fn raw_sign(seed: &[u8; 32], message: &[u8]) -> [u8; 64] {
    let key = SigningKey::from_bytes(seed);
    let sig: Signature = key.sign(message);
    sig.to_bytes()
}

#[allow(clippy::disallowed_methods)]
fn raw_verify(pubkey: &[u8; 32], message: &[u8], signature: &[u8; 64]) -> bool {
    let Ok(vk) = VerifyingKey::from_bytes(pubkey) else {
        return false;
    };
    // `verify_strict` rather than `verify`: routing every context through one choke point forces
    // a single semantics, and this is the stricter of the two that existed before. It additionally
    // rejects small-order and torsion-component public keys, which no honest station produces.
    // `wire_query` already required it; the other contexts are strengthened to match.
    vk.verify_strict(message, &Signature::from_bytes(signature))
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: SigningDomain = SigningDomain::Manifest;
    const CONREQ: SigningDomain = SigningDomain::ConReq;

    fn pubkey_of(seed: &[u8; 32]) -> [u8; 32] {
        SigningKey::from_bytes(seed).verifying_key().to_bytes()
    }

    #[test]
    fn prepended_round_trip() {
        let seed = [0x42u8; 32];
        let sig = sign_in_domain(MANIFEST, &seed, b"payload").expect("sign");
        assert!(verify_in_domain(
            MANIFEST,
            &pubkey_of(&seed),
            b"payload",
            &sig
        ));
    }

    #[test]
    fn in_band_round_trip() {
        let seed = [0x42u8; 32];
        let msg = b"HSCQ\x02\x00\x04body";
        let sig = sign_in_band(CONREQ, &seed, msg).expect("sign");
        assert!(verify_in_band(CONREQ, &pubkey_of(&seed), msg, &sig));
    }

    /// The property the whole registry exists for: the same payload under two domains yields
    /// signatures that do not cross-verify.
    #[test]
    fn a_signature_does_not_cross_verify_into_another_domain() {
        // VERIFIES: REQ-SEC-13
        let seed = [0x11u8; 32];
        let pk = pubkey_of(&seed);
        let payload = b"identical bytes in both contexts";

        let manifest_sig = sign_in_domain(MANIFEST, &seed, payload).expect("sign");
        assert!(verify_in_domain(MANIFEST, &pk, payload, &manifest_sig));
        assert!(
            !verify_in_domain(SigningDomain::FileOffer, &pk, payload, &manifest_sig),
            "a manifest signature verified as a file offer over identical payload bytes"
        );
        assert!(
            !verify_in_domain(SigningDomain::PeerDescriptor, &pk, payload, &manifest_sig),
            "a manifest signature verified as a peer descriptor over identical payload bytes"
        );
    }

    #[test]
    fn tampered_payload_fails() {
        let seed = [0x07u8; 32];
        let sig = sign_in_domain(MANIFEST, &seed, b"original").expect("sign");
        assert!(!verify_in_domain(
            MANIFEST,
            &pubkey_of(&seed),
            b"tampered",
            &sig
        ));
    }

    #[test]
    fn wrong_key_fails() {
        let sig = sign_in_domain(MANIFEST, &[0x01u8; 32], b"message").expect("sign");
        assert!(!verify_in_domain(
            MANIFEST,
            &pubkey_of(&[0x02u8; 32]),
            b"message",
            &sig
        ));
    }

    #[test]
    fn an_in_band_message_without_its_tag_is_refused() {
        let err = sign_in_band(CONREQ, &[0x42u8; 32], b"XXXX\x02body").unwrap_err();
        assert_eq!(err, SigningError::MissingInBandTag { domain: CONREQ });
    }

    #[test]
    fn the_entry_points_refuse_the_wrong_placement() {
        assert!(matches!(
            sign_in_band(MANIFEST, &[0x42u8; 32], b"OPMFbody"),
            Err(SigningError::WrongPlacement { .. })
        ));
        assert!(matches!(
            sign_in_domain(CONREQ, &[0x42u8; 32], b"body"),
            Err(SigningError::WrongPlacement { .. })
        ));
    }
}

#[cfg(test)]
mod family_tests {
    use super::*;
    use crate::signing_domain::SigningDomain;

    /// The two families behave differently BY DESIGN, and this pins which is which.
    ///
    /// In-band domains sign the transmitted bytes unchanged, so this work does not alter their
    /// signatures at all — the CONREQ known-answer vector still matches, which is the evidence.
    /// Prepended domains gain five bytes ahead of the payload, so their signatures necessarily
    /// change. A future edit that "unified" the two would break the KAT and fail here.
    #[test]
    fn only_the_prepended_family_changes_the_signed_bytes() {
        let seed = [0x01u8; 32];
        let payload = b"identical payload bytes";

        let in_band_msg = b"HSCQ\x02\x00\x17identical payload bytes";
        let in_band = sign_in_band(SigningDomain::ConReq, &seed, in_band_msg).expect("sign");
        assert_eq!(
            in_band,
            raw_sign(&seed, in_band_msg),
            "an in-band domain must sign the transmitted bytes verbatim"
        );

        let prepended = sign_in_domain(SigningDomain::Manifest, &seed, payload).expect("sign");
        assert_ne!(
            prepended,
            raw_sign(&seed, payload),
            "a prepended domain must NOT sign the bare payload"
        );

        let msg = prepend_domain(SigningDomain::Manifest, payload).expect("prepend");
        assert_eq!(&msg[..4], b"OPMF");
        assert_eq!(msg[4], SigningDomain::Manifest.version());
        assert_eq!(&msg[5..], payload);
    }
}
