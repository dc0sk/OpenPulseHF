use crate::error::ModemError;
use crate::trust::SigningMode;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const ENVELOPE_MAGIC: &[u8; 4] = b"OPSE";
const ENVELOPE_VERSION: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvelopeHeader {
    pub session_id: String,
    pub sequence: u64,
    pub signing_mode: SigningMode,
    pub payload_hash_algorithm: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignatureBlock {
    pub signer_id: String,
    pub key_id: String,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedEnvelope {
    pub header: EnvelopeHeader,
    pub payload: Vec<u8>,
    pub payload_hash: [u8; 32],
    pub signature: SignatureBlock,
}

impl SignedEnvelope {
    pub fn new(
        session_id: impl Into<String>,
        sequence: u64,
        signing_mode: SigningMode,
        payload: Vec<u8>,
        signer_id: impl Into<String>,
        key_id: impl Into<String>,
        signature: Vec<u8>,
    ) -> Self {
        let payload_hash = sha256(&payload);
        Self {
            header: EnvelopeHeader {
                session_id: session_id.into(),
                sequence,
                signing_mode,
                payload_hash_algorithm: "sha256".to_string(),
            },
            payload,
            payload_hash,
            signature: SignatureBlock {
                signer_id: signer_id.into(),
                key_id: key_id.into(),
                signature,
            },
        }
    }

    pub fn verify_payload_hash(&self) -> bool {
        self.payload_hash == sha256(&self.payload)
    }

    pub fn encode(&self) -> Result<Vec<u8>, ModemError> {
        let body = serde_json::to_vec(self)
            .map_err(|e| ModemError::Frame(format!("signed envelope encode failed: {e}")))?;
        let len = u32::try_from(body.len())
            .map_err(|_| ModemError::Frame("signed envelope too large".to_string()))?;

        let mut out = Vec::with_capacity(4 + 1 + 4 + body.len());
        out.extend_from_slice(ENVELOPE_MAGIC);
        out.push(ENVELOPE_VERSION);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&body);
        Ok(out)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ModemError> {
        let min = 4 + 1 + 4;
        if bytes.len() < min {
            return Err(ModemError::Frame("signed envelope too short".to_string()));
        }
        if &bytes[..4] != ENVELOPE_MAGIC {
            return Err(ModemError::Frame(
                "invalid signed envelope magic".to_string(),
            ));
        }
        if bytes[4] != ENVELOPE_VERSION {
            return Err(ModemError::Frame(format!(
                "unsupported signed envelope version {}",
                bytes[4]
            )));
        }

        let body_len = u32::from_be_bytes([bytes[5], bytes[6], bytes[7], bytes[8]]) as usize;
        if bytes.len() != min + body_len {
            return Err(ModemError::Frame(
                "signed envelope length mismatch".to_string(),
            ));
        }

        let env: SignedEnvelope = serde_json::from_slice(&bytes[min..])
            .map_err(|e| ModemError::Frame(format!("signed envelope decode failed: {e}")))?;

        if env.header.payload_hash_algorithm != "sha256" {
            return Err(ModemError::Frame(format!(
                "unsupported payload hash algorithm '{}'",
                env.header.payload_hash_algorithm
            )));
        }

        if !env.verify_payload_hash() {
            return Err(ModemError::Frame(
                "signed envelope payload hash mismatch".to_string(),
            ));
        }

        Ok(env)
    }
}

fn sha256(payload: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(payload);
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_round_trip() {
        let env = SignedEnvelope::new(
            "session-1",
            7,
            SigningMode::Normal,
            b"hello".to_vec(),
            "peer-a",
            "key-1",
            vec![1, 2, 3, 4],
        );

        let encoded = env.encode().expect("encode envelope");
        let decoded = SignedEnvelope::decode(&encoded).expect("decode envelope");
        assert_eq!(decoded, env);
    }

    #[test]
    fn tampered_payload_is_rejected() {
        let env = SignedEnvelope::new(
            "session-1",
            3,
            SigningMode::Psk,
            b"hello".to_vec(),
            "peer-a",
            "key-1",
            vec![9, 9, 9],
        );

        let mut encoded = env.encode().expect("encode envelope");
        let last = encoded.len() - 1;
        encoded[last] ^= 0x01;

        assert!(SignedEnvelope::decode(&encoded).is_err());
    }
}
