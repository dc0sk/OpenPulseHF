use openpulse_core::error::ModemError;
use openpulse_core::signed_envelope::SignedEnvelope;
use openpulse_core::trust::SigningMode;

/// Encode a payload and signature metadata into a signed envelope wire blob.
pub fn encode_signed_payload(
    session_id: &str,
    sequence: u64,
    signing_mode: SigningMode,
    payload: &[u8],
    signer_id: &str,
    key_id: &str,
    signature: &[u8],
) -> Result<Vec<u8>, ModemError> {
    SignedEnvelope::new(
        session_id,
        sequence,
        signing_mode,
        payload.to_vec(),
        signer_id,
        key_id,
        signature.to_vec(),
    )
    .encode()
}

/// Decode a signed envelope wire blob and verify payload hash integrity.
pub fn decode_signed_payload(envelope_bytes: &[u8]) -> Result<SignedEnvelope, ModemError> {
    SignedEnvelope::decode(envelope_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codec_round_trip() {
        let encoded = encode_signed_payload(
            "sess-1",
            9,
            SigningMode::Normal,
            b"hello",
            "peer-a",
            "key-a",
            &[1, 2, 3],
        )
        .expect("encode signed payload");

        let decoded = decode_signed_payload(&encoded).expect("decode signed payload");
        assert_eq!(decoded.header.session_id, "sess-1");
        assert_eq!(decoded.header.sequence, 9);
        assert_eq!(decoded.payload, b"hello");
    }
}
