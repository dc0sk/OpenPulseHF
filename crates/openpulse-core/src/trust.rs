use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PublicKeyTrustLevel {
    Full,
    Marginal,
    Unknown,
    Untrusted,
    Revoked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CertificateSource {
    OutOfBand,
    OverAir,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConnectionTrustLevel {
    Rejected,
    Low,
    Unverified,
    Reduced,
    PskVerified,
    Verified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SigningMode {
    Normal,
    Psk,
    Relaxed,
    Paranoid,
    /// ML-DSA-44 post-quantum signature only.
    Pq,
    /// Ed25519 + ML-DSA-44 dual signature (highest strength).
    Hybrid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PolicyProfile {
    Strict,
    Balanced,
    Permissive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustDecision {
    pub decision: ConnectionTrustLevel,
    pub reason_code: String,
    pub certificate_source: CertificateSource,
    pub public_key_trust: PublicKeyTrustLevel,
    pub psk_validated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandshakeDecision {
    pub selected_mode: SigningMode,
    pub trust: TrustDecision,
    pub policy_profile: PolicyProfile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustError {
    NoMutualSigningMode,
    WeakSigningModeRejected,
    RejectedTrustLevel,
    KeyDerivationFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionKeys {
    pub hmac_key: [u8; 32],
    pub key_confirmation_tag: [u8; 16],
    pub transcript_hash: [u8; 32],
}

pub fn classify_connection_trust(
    key_trust: PublicKeyTrustLevel,
    certificate_source: CertificateSource,
    psk_validated: bool,
) -> TrustDecision {
    let decision = match key_trust {
        PublicKeyTrustLevel::Untrusted | PublicKeyTrustLevel::Revoked => {
            ConnectionTrustLevel::Rejected
        }
        PublicKeyTrustLevel::Full => match certificate_source {
            CertificateSource::OutOfBand => ConnectionTrustLevel::Verified,
            CertificateSource::OverAir if psk_validated => ConnectionTrustLevel::PskVerified,
            CertificateSource::OverAir => ConnectionTrustLevel::Reduced,
        },
        PublicKeyTrustLevel::Marginal => match certificate_source {
            CertificateSource::OverAir if psk_validated => ConnectionTrustLevel::PskVerified,
            _ => ConnectionTrustLevel::Reduced,
        },
        PublicKeyTrustLevel::Unknown => match certificate_source {
            CertificateSource::OutOfBand => ConnectionTrustLevel::Unverified,
            CertificateSource::OverAir if psk_validated => ConnectionTrustLevel::PskVerified,
            CertificateSource::OverAir => ConnectionTrustLevel::Low,
        },
    };

    let reason_code = match decision {
        ConnectionTrustLevel::Verified => "verified_out_of_band",
        ConnectionTrustLevel::PskVerified => "psk_validated_over_air_certificate",
        ConnectionTrustLevel::Reduced => "over_air_certificate_without_psk",
        ConnectionTrustLevel::Unverified => "key_trust_unknown",
        ConnectionTrustLevel::Low => "unknown_key_over_air_certificate_without_psk",
        ConnectionTrustLevel::Rejected => "policy_rejected",
    };

    TrustDecision {
        decision,
        reason_code: reason_code.to_string(),
        certificate_source,
        public_key_trust: key_trust,
        psk_validated,
    }
}

pub fn allowed_signing_modes(profile: PolicyProfile) -> &'static [SigningMode] {
    match profile {
        PolicyProfile::Strict => &[
            SigningMode::Normal,
            SigningMode::Paranoid,
            SigningMode::Pq,
            SigningMode::Hybrid,
        ],
        PolicyProfile::Balanced => &[
            SigningMode::Normal,
            SigningMode::Psk,
            SigningMode::Relaxed,
            SigningMode::Pq,
            SigningMode::Hybrid,
        ],
        PolicyProfile::Permissive => &[
            SigningMode::Normal,
            SigningMode::Psk,
            SigningMode::Relaxed,
            SigningMode::Pq,
            SigningMode::Hybrid,
        ],
    }
}

fn mode_strength(mode: SigningMode) -> u8 {
    match mode {
        SigningMode::Relaxed => 1,
        SigningMode::Psk | SigningMode::Normal => 2,
        SigningMode::Paranoid => 3,
        SigningMode::Pq => 4,
        SigningMode::Hybrid => 5,
    }
}

pub fn select_signing_mode(
    profile: PolicyProfile,
    local_minimum: SigningMode,
    peer_supported: &[SigningMode],
) -> Result<SigningMode, TrustError> {
    let mut candidates: Vec<SigningMode> = allowed_signing_modes(profile)
        .iter()
        .copied()
        .filter(|mode| peer_supported.contains(mode))
        .collect();

    if candidates.is_empty() {
        return Err(TrustError::NoMutualSigningMode);
    }

    candidates.sort_by_key(|mode| std::cmp::Reverse(mode_strength(*mode)));
    let selected = candidates[0];
    if mode_strength(selected) < mode_strength(local_minimum) {
        return Err(TrustError::WeakSigningModeRejected);
    }

    Ok(selected)
}

pub fn evaluate_handshake(
    profile: PolicyProfile,
    local_minimum_mode: SigningMode,
    peer_supported_modes: &[SigningMode],
    key_trust: PublicKeyTrustLevel,
    certificate_source: CertificateSource,
    psk_validated: bool,
) -> Result<HandshakeDecision, TrustError> {
    let selected_mode = select_signing_mode(profile, local_minimum_mode, peer_supported_modes)?;
    let trust = classify_connection_trust(key_trust, certificate_source, psk_validated);

    if trust.decision == ConnectionTrustLevel::Rejected {
        return Err(TrustError::RejectedTrustLevel);
    }

    Ok(HandshakeDecision {
        selected_mode,
        trust,
        policy_profile: profile,
    })
}

pub fn derive_session_keys(
    local_private_key: [u8; 32],
    remote_public_key: [u8; 32],
    session_id: &str,
    local_peer_id: &str,
    remote_peer_id: &str,
    selected_mode: SigningMode,
) -> Result<SessionKeys, TrustError> {
    let private = StaticSecret::from(local_private_key);
    let remote = PublicKey::from(remote_public_key);
    let shared_secret = private.diffie_hellman(&remote);

    let transcript_hash = {
        let mut hasher = Sha256::new();
        hasher.update(session_id.as_bytes());
        hasher.update(local_peer_id.as_bytes());
        hasher.update(remote_peer_id.as_bytes());
        hasher.update(format!("{:?}", selected_mode).as_bytes());
        hasher.finalize()
    };

    let hk = Hkdf::<Sha256>::new(Some(transcript_hash.as_slice()), shared_secret.as_bytes());
    let mut okm = [0u8; 48];
    hk.expand(b"openpulsehf/session-v1", &mut okm)
        .map_err(|_| TrustError::KeyDerivationFailed)?;

    let mut hmac_key = [0u8; 32];
    hmac_key.copy_from_slice(&okm[..32]);
    let mut key_confirmation_tag = [0u8; 16];
    key_confirmation_tag.copy_from_slice(&okm[32..48]);

    let mut transcript = [0u8; 32];
    transcript.copy_from_slice(transcript_hash.as_slice());

    Ok(SessionKeys {
        hmac_key,
        key_confirmation_tag,
        transcript_hash: transcript,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trust_classification_matches_policy_table() {
        let verified = classify_connection_trust(
            PublicKeyTrustLevel::Full,
            CertificateSource::OutOfBand,
            false,
        );
        assert_eq!(verified.decision, ConnectionTrustLevel::Verified);

        let reduced = classify_connection_trust(
            PublicKeyTrustLevel::Marginal,
            CertificateSource::OverAir,
            false,
        );
        assert_eq!(reduced.decision, ConnectionTrustLevel::Reduced);

        let low = classify_connection_trust(
            PublicKeyTrustLevel::Unknown,
            CertificateSource::OverAir,
            false,
        );
        assert_eq!(low.decision, ConnectionTrustLevel::Low);
    }

    #[test]
    fn strict_profile_rejects_psk_only_peer() {
        let mode = select_signing_mode(
            PolicyProfile::Strict,
            SigningMode::Normal,
            &[SigningMode::Psk],
        );
        assert!(matches!(mode, Err(TrustError::NoMutualSigningMode)));
    }

    #[test]
    fn balanced_profile_can_choose_psk() {
        let mode = select_signing_mode(
            PolicyProfile::Balanced,
            SigningMode::Relaxed,
            &[SigningMode::Psk, SigningMode::Relaxed],
        )
        .unwrap();
        assert_eq!(mode, SigningMode::Psk);
    }

    #[test]
    fn strict_profile_prefers_paranoid_when_available() {
        let mode = select_signing_mode(
            PolicyProfile::Strict,
            SigningMode::Normal,
            &[SigningMode::Paranoid, SigningMode::Normal],
        )
        .unwrap();
        assert_eq!(mode, SigningMode::Paranoid);
    }

    #[test]
    fn evaluate_handshake_rejects_untrusted_keys() {
        let result = evaluate_handshake(
            PolicyProfile::Balanced,
            SigningMode::Normal,
            &[SigningMode::Normal],
            PublicKeyTrustLevel::Untrusted,
            CertificateSource::OutOfBand,
            false,
        );
        assert!(matches!(result, Err(TrustError::RejectedTrustLevel)));
    }

    #[test]
    fn x25519_hkdf_derivation_is_deterministic() {
        let local_private = [7u8; 32];
        let remote_private = [9u8; 32];
        let remote_public = PublicKey::from(&StaticSecret::from(remote_private));

        let keys_a = derive_session_keys(
            local_private,
            remote_public.to_bytes(),
            "session-1",
            "A",
            "B",
            SigningMode::Normal,
        )
        .unwrap();

        let keys_b = derive_session_keys(
            local_private,
            remote_public.to_bytes(),
            "session-1",
            "A",
            "B",
            SigningMode::Normal,
        )
        .unwrap();

        assert_eq!(keys_a, keys_b);
    }
}
