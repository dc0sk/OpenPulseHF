//! Signed authentication beacon transmitted via the FreeDV data channel.

use openpulse_core::signing::{sign_in_band, verify_in_band};
use openpulse_core::signing_domain::SigningDomain;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BeaconError {
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("hex: {0}")]
    Hex(#[from] hex::FromHexError),
    #[error("invalid field length")]
    InvalidLength,
    #[error("bad magic: expected {expected:?}")]
    BadMagic { expected: &'static [u8; 4] },
    #[error("unsupported beacon version {got:#04x} (expected {expected:#04x})")]
    UnsupportedVersion { got: u8, expected: u8 },
}

/// Wire magic. Equals `SigningDomain::AuthBeacon.tag()`, which is what the signature covers.
pub const BEACON_MAGIC: &[u8; 4] = SigningDomain::AuthBeacon.tag();

/// Wire version, taken from the registry so the transmitted byte and the signed byte are one value.
pub const BEACON_VERSION: u8 = SigningDomain::AuthBeacon.version();

/// `OPAB` + version + `u16` length.
const HEADER_LEN: usize = 7;

/// The signed message: `OPAB || version || canonical body`.
///
/// It begins with the domain tag, which is what makes this an in-band domain — the bytes the
/// signature covers are bytes the receiver actually has, not a prefix it must reconstruct.
fn signed_message(canonical_body: &[u8]) -> Vec<u8> {
    let mut msg = Vec::with_capacity(5 + canonical_body.len());
    msg.extend_from_slice(BEACON_MAGIC);
    msg.push(BEACON_VERSION);
    msg.extend_from_slice(canonical_body);
    msg
}

/// Canonical body covered by the Ed25519 signature (no signature field).
#[derive(Serialize, Deserialize)]
struct BeaconBody {
    callsign: String,
    timestamp_utc: u64,
    session_nonce: String, // hex 16 bytes
    freq_hz: u64,
    mode: String,
    pubkey: String, // hex 32 bytes
}

/// Wire-format struct with all binary fields hex-encoded for clean JSON.
#[derive(Serialize, Deserialize)]
struct BeaconWire {
    callsign: String,
    timestamp_utc: u64,
    session_nonce: String, // hex 16 bytes
    freq_hz: u64,
    mode: String,
    pubkey: String,    // hex 32 bytes
    signature: String, // hex 64 bytes
}

/// Ed25519-signed authentication beacon.
///
/// The `signature` covers the canonical JSON of all fields except `signature`
/// itself (via [`BeaconBody`]).  Recipients verify against the embedded
/// `pubkey` then optionally look it up in their trust store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthBeacon {
    pub callsign: String,
    pub timestamp_utc: u64,
    pub session_nonce: [u8; 16],
    pub freq_hz: u64,
    pub mode: String,
    pub pubkey: [u8; 32],
    pub signature: [u8; 64],
}

impl AuthBeacon {
    /// Build and sign a beacon with the operator's Ed25519 key.
    pub fn sign(
        callsign: impl Into<String>,
        timestamp_utc: u64,
        session_nonce: [u8; 16],
        freq_hz: u64,
        mode: impl Into<String>,
        signing_seed: &[u8; 32],
        pubkey: [u8; 32],
    ) -> Self {
        let callsign = callsign.into();
        let mode = mode.into();
        let body = BeaconBody {
            callsign: callsign.clone(),
            timestamp_utc,
            session_nonce: hex::encode(session_nonce),
            freq_hz,
            mode: mode.clone(),
            pubkey: hex::encode(pubkey),
        };
        let canonical = serde_json::to_vec(&body).expect("beacon body serialisation");
        let signature = sign_in_band(
            SigningDomain::AuthBeacon,
            signing_seed,
            &signed_message(&canonical),
        )
        .unwrap_or([0u8; 64]);
        Self {
            callsign,
            timestamp_utc,
            session_nonce,
            freq_hz,
            mode,
            pubkey,
            signature,
        }
    }

    /// Verify the beacon's signature against its embedded public key.
    pub fn verify(&self) -> bool {
        let body = BeaconBody {
            callsign: self.callsign.clone(),
            timestamp_utc: self.timestamp_utc,
            session_nonce: hex::encode(self.session_nonce),
            freq_hz: self.freq_hz,
            mode: self.mode.clone(),
            pubkey: hex::encode(self.pubkey),
        };
        let Ok(canonical) = serde_json::to_vec(&body) else {
            return false;
        };
        verify_in_band(
            SigningDomain::AuthBeacon,
            &self.pubkey,
            &signed_message(&canonical),
            &self.signature,
        )
    }

    /// Encode to wire bytes: `[OPAB][version: u8][u16 BE len][JSON]`.
    ///
    /// The magic and version are what a receiver branches on. Before #1206 there was neither, so a
    /// build could not tell which format it was looking at — on the one message whose entire
    /// purpose is "you can verify who sent this".
    pub fn encode(&self) -> Vec<u8> {
        let wire = BeaconWire {
            callsign: self.callsign.clone(),
            timestamp_utc: self.timestamp_utc,
            session_nonce: hex::encode(self.session_nonce),
            freq_hz: self.freq_hz,
            mode: self.mode.clone(),
            pubkey: hex::encode(self.pubkey),
            signature: hex::encode(self.signature),
        };
        let json = serde_json::to_vec(&wire).expect("beacon wire serialisation");
        let len = json.len() as u16;
        let mut out = Vec::with_capacity(HEADER_LEN + json.len());
        out.extend_from_slice(BEACON_MAGIC);
        out.push(BEACON_VERSION);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&json);
        out
    }

    /// Decode from the wire format produced by [`encode`].
    ///
    /// Rejects an unknown magic or version rather than attempting the parse: a beacon from a
    /// format this build does not know is not a beacon it can make an identity claim about.
    pub fn decode(bytes: &[u8]) -> Result<Self, BeaconError> {
        if bytes.len() < HEADER_LEN {
            return Err(BeaconError::InvalidLength);
        }
        if &bytes[0..4] != BEACON_MAGIC.as_slice() {
            return Err(BeaconError::BadMagic {
                expected: BEACON_MAGIC,
            });
        }
        if bytes[4] != BEACON_VERSION {
            return Err(BeaconError::UnsupportedVersion {
                got: bytes[4],
                expected: BEACON_VERSION,
            });
        }
        let len = u16::from_be_bytes([bytes[5], bytes[6]]) as usize;
        if bytes.len() < HEADER_LEN + len {
            return Err(BeaconError::InvalidLength);
        }
        let wire: BeaconWire = serde_json::from_slice(&bytes[HEADER_LEN..HEADER_LEN + len])?;

        let session_nonce: [u8; 16] = hex::decode(&wire.session_nonce)?
            .try_into()
            .map_err(|_| BeaconError::InvalidLength)?;
        let pubkey: [u8; 32] = hex::decode(&wire.pubkey)?
            .try_into()
            .map_err(|_| BeaconError::InvalidLength)?;
        let signature: [u8; 64] = hex::decode(&wire.signature)?
            .try_into()
            .map_err(|_| BeaconError::InvalidLength)?;

        Ok(Self {
            callsign: wire.callsign,
            timestamp_utc: wire.timestamp_utc,
            session_nonce,
            freq_hz: wire.freq_hz,
            mode: wire.mode,
            pubkey,
            signature,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    fn make_key() -> ([u8; 32], [u8; 32]) {
        let seed = [0xABu8; 32];
        let sk = SigningKey::from_bytes(&seed);
        (seed, sk.verifying_key().to_bytes())
    }

    #[test]
    fn sign_verify_round_trip() {
        let (seed, pubkey) = make_key();
        let beacon = AuthBeacon::sign(
            "W1AW",
            1_746_800_000,
            [0x01u8; 16],
            14_236_000,
            "FreeDV-1600",
            &seed,
            pubkey,
        );
        assert!(beacon.verify());
        assert_eq!(beacon.callsign, "W1AW");
    }

    #[test]
    fn tampered_callsign_fails_verify() {
        let (seed, pubkey) = make_key();
        let mut beacon = AuthBeacon::sign(
            "W1AW",
            1_746_800_000,
            [0u8; 16],
            14_236_000,
            "FreeDV-1600",
            &seed,
            pubkey,
        );
        beacon.callsign = "W9ZZZ".into();
        assert!(!beacon.verify());
    }

    #[test]
    fn encode_decode_round_trip() {
        let (seed, pubkey) = make_key();
        let beacon = AuthBeacon::sign(
            "K0ABC",
            1_746_900_000,
            [0x55u8; 16],
            7_074_000,
            "FreeDV-700D",
            &seed,
            pubkey,
        );
        let decoded = AuthBeacon::decode(&beacon.encode()).unwrap();
        assert_eq!(beacon, decoded);
        assert!(decoded.verify());
    }

    #[test]
    fn decode_truncated_returns_error() {
        assert!(matches!(
            AuthBeacon::decode(&[0x01]),
            Err(BeaconError::InvalidLength)
        ));
        // A well-formed header whose length field promises more body than is present.
        let mut short = Vec::new();
        short.extend_from_slice(BEACON_MAGIC);
        short.push(BEACON_VERSION);
        short.extend_from_slice(&999u16.to_be_bytes());
        short.extend_from_slice(b"{}");
        assert!(matches!(
            AuthBeacon::decode(&short),
            Err(BeaconError::InvalidLength)
        ));
    }

    fn sample() -> AuthBeacon {
        let (seed, pubkey) = make_key();
        AuthBeacon::sign(
            "W1AW",
            1_746_800_000,
            [0x07u8; 16],
            14_236_000,
            "FreeDV-1600",
            &seed,
            pubkey,
        )
    }

    /// The transmitted header is the registry's, not a local literal — that is what makes the
    /// signed byte and the wire byte one value rather than two that can drift.
    #[test]
    fn the_wire_header_comes_from_the_signing_registry() {
        let bytes = sample().encode();
        assert_eq!(&bytes[0..4], SigningDomain::AuthBeacon.tag().as_slice());
        assert_eq!(bytes[4], SigningDomain::AuthBeacon.version());
    }

    #[test]
    fn decode_rejects_a_foreign_magic() {
        let mut bytes = sample().encode();
        bytes[0..4].copy_from_slice(b"OPQS");
        assert!(matches!(
            AuthBeacon::decode(&bytes),
            Err(BeaconError::BadMagic { .. })
        ));
    }

    /// The wrong version is DERIVED from the constant, so this keeps straddling the boundary when
    /// the version is bumped instead of silently testing the shipping value.
    #[test]
    fn decode_rejects_an_unknown_version() {
        let mut bytes = sample().encode();
        bytes[4] = BEACON_VERSION.wrapping_add(1);
        match AuthBeacon::decode(&bytes) {
            Err(BeaconError::UnsupportedVersion { got, expected }) => {
                assert_eq!(got, BEACON_VERSION.wrapping_add(1));
                assert_eq!(expected, BEACON_VERSION);
            }
            other => panic!("expected UnsupportedVersion, got {other:?}"),
        }
    }

    /// The magic and version are inside the signature, not merely beside it — so a peer cannot
    /// relabel a beacon's format and keep the identity claim attached to it.
    #[test]
    fn the_signature_covers_the_magic_and_version() {
        let beacon = sample();
        let body = BeaconBody {
            callsign: beacon.callsign.clone(),
            timestamp_utc: beacon.timestamp_utc,
            session_nonce: hex::encode(beacon.session_nonce),
            freq_hz: beacon.freq_hz,
            mode: beacon.mode.clone(),
            pubkey: hex::encode(beacon.pubkey),
        };
        let canonical = serde_json::to_vec(&body).unwrap();

        // Positive control: the real signed message verifies.
        assert!(verify_in_band(
            SigningDomain::AuthBeacon,
            &beacon.pubkey,
            &signed_message(&canonical),
            &beacon.signature,
        ));

        for (label, mut msg) in [
            ("version", signed_message(&canonical)),
            ("magic", signed_message(&canonical)),
        ] {
            if label == "version" {
                msg[4] = msg[4].wrapping_add(1);
            } else {
                msg[0..4].copy_from_slice(b"OPQS");
            }
            assert!(
                !verify_in_band(
                    SigningDomain::AuthBeacon,
                    &beacon.pubkey,
                    &msg,
                    &beacon.signature,
                ),
                "a tampered {label} still verified"
            );
        }
    }
}
