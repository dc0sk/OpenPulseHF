//! Signed authentication beacon transmitted via the FreeDV data channel.

use openpulse_core::signing::{sign_bytes, verify_bytes};
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
        let signature = sign_bytes(signing_seed, &canonical);
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
        verify_bytes(&self.pubkey, &canonical, &self.signature)
    }

    /// Encode to length-prefixed JSON wire bytes: `[u16 BE len][JSON]`.
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
        let mut out = Vec::with_capacity(2 + json.len());
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&json);
        out
    }

    /// Decode from the length-prefixed wire format produced by [`encode`].
    pub fn decode(bytes: &[u8]) -> Result<Self, BeaconError> {
        if bytes.len() < 2 {
            return Err(BeaconError::InvalidLength);
        }
        let len = u16::from_be_bytes([bytes[0], bytes[1]]) as usize;
        if bytes.len() < 2 + len {
            return Err(BeaconError::InvalidLength);
        }
        let wire: BeaconWire = serde_json::from_slice(&bytes[2..2 + len])?;

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
        assert!(AuthBeacon::decode(&[0x01]).is_err());
        assert!(AuthBeacon::decode(&[0x00, 0x10, 0xFF]).is_err());
    }
}
