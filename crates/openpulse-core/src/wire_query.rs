use thiserror::Error;

/// Errors produced by wire envelope and payload codec.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum WireQueryError {
    #[error("buffer too short")]
    BufferTooShort,
    #[error("invalid magic bytes")]
    InvalidMagic,
    #[error("unknown message type {0:#04x}")]
    UnknownMsgType(u8),
    #[error("payload_len in envelope does not match actual payload bytes")]
    PayloadLenMismatch,
    #[error("payload too long to encode")]
    PayloadTooLong,
    #[error("malformed payload")]
    MalformedPayload,
}

/// Message type codes for the OPHF wire envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WireMsgType {
    PeerQueryRequest = 0x01,
    PeerQueryResponse = 0x02,
}

impl WireMsgType {
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x01 => Some(Self::PeerQueryRequest),
            0x02 => Some(Self::PeerQueryResponse),
            _ => None,
        }
    }
}

const MAGIC: &[u8; 4] = b"OPHF";
const VERSION: u8 = 1;
/// Byte count of fixed envelope fields excluding payload and auth_tag.
const HEADER_SIZE: usize = 104;
const AUTH_TAG_SIZE: usize = 16;

/// Outer envelope wrapping all OPHF control-plane messages.
///
/// All integer fields use big-endian (network) byte order.
#[derive(Debug, Clone)]
pub struct WireEnvelope {
    pub msg_type: WireMsgType,
    pub flags: u16,
    pub session_id: u64,
    pub src_peer_id: [u8; 32],
    pub dst_peer_id: [u8; 32],
    pub nonce: [u8; 12],
    pub timestamp_ms: u64,
    pub hop_limit: u8,
    pub hop_index: u8,
    pub payload: Vec<u8>,
    pub auth_tag: [u8; 16],
}

impl WireEnvelope {
    pub fn encode(&self) -> Result<Vec<u8>, WireQueryError> {
        let payload_len = self.payload.len();
        if payload_len > u16::MAX as usize {
            return Err(WireQueryError::PayloadTooLong);
        }
        let mut buf = Vec::with_capacity(HEADER_SIZE + payload_len + AUTH_TAG_SIZE);
        buf.extend_from_slice(MAGIC);
        buf.push(VERSION);
        buf.push(self.msg_type as u8);
        buf.extend_from_slice(&self.flags.to_be_bytes());
        buf.extend_from_slice(&self.session_id.to_be_bytes());
        buf.extend_from_slice(&self.src_peer_id);
        buf.extend_from_slice(&self.dst_peer_id);
        buf.extend_from_slice(&self.nonce);
        buf.extend_from_slice(&self.timestamp_ms.to_be_bytes());
        buf.push(self.hop_limit);
        buf.push(self.hop_index);
        buf.extend_from_slice(&(payload_len as u16).to_be_bytes());
        buf.extend_from_slice(&self.payload);
        buf.extend_from_slice(&self.auth_tag);
        Ok(buf)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, WireQueryError> {
        if bytes.len() < HEADER_SIZE + AUTH_TAG_SIZE {
            return Err(WireQueryError::BufferTooShort);
        }
        if &bytes[0..4] != MAGIC {
            return Err(WireQueryError::InvalidMagic);
        }
        // bytes[4] = version; forward-compatible: parse but don't reject unknown versions here
        let msg_type =
            WireMsgType::from_u8(bytes[5]).ok_or(WireQueryError::UnknownMsgType(bytes[5]))?;
        let flags = u16::from_be_bytes([bytes[6], bytes[7]]);
        let session_id = u64::from_be_bytes(bytes[8..16].try_into().unwrap());
        let src_peer_id: [u8; 32] = bytes[16..48].try_into().unwrap();
        let dst_peer_id: [u8; 32] = bytes[48..80].try_into().unwrap();
        let nonce: [u8; 12] = bytes[80..92].try_into().unwrap();
        let timestamp_ms = u64::from_be_bytes(bytes[92..100].try_into().unwrap());
        let hop_limit = bytes[100];
        let hop_index = bytes[101];
        let payload_len = u16::from_be_bytes([bytes[102], bytes[103]]) as usize;

        let payload_start = 104;
        let payload_end = payload_start + payload_len;
        let auth_tag_end = payload_end + AUTH_TAG_SIZE;

        if bytes.len() < auth_tag_end {
            return Err(WireQueryError::BufferTooShort);
        }

        let payload = bytes[payload_start..payload_end].to_vec();
        let auth_tag: [u8; 16] = bytes[payload_end..auth_tag_end].try_into().unwrap();

        Ok(Self {
            msg_type,
            flags,
            session_id,
            src_peer_id,
            dst_peer_id,
            nonce,
            timestamp_ms,
            hop_limit,
            hop_index,
            payload,
            auth_tag,
        })
    }
}

// ------------------------------------------------------------------
// peer_query_request payload
// ------------------------------------------------------------------

/// Payload for msg_type 0x01 — peer_query_request.
///
/// Encoded: query_id(8) | capability_mask(4) | min_link_quality(2) |
///          trust_filter(1) | max_results(2) = 17 bytes fixed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerQueryRequest {
    pub query_id: u64,
    /// Peer must have all bits set; 0 = any capability.
    pub capability_mask: u32,
    pub min_link_quality: u16,
    /// Wire code per peer-query-relay-wire.md: 0x00 trusted_only, 0x01 trusted_or_unknown, 0x02 any.
    pub trust_filter: u8,
    pub max_results: u16,
}

impl PeerQueryRequest {
    /// Encoded size in bytes.
    pub const SIZE: usize = 17;

    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::SIZE);
        buf.extend_from_slice(&self.query_id.to_be_bytes());
        buf.extend_from_slice(&self.capability_mask.to_be_bytes());
        buf.extend_from_slice(&self.min_link_quality.to_be_bytes());
        buf.push(self.trust_filter);
        buf.extend_from_slice(&self.max_results.to_be_bytes());
        buf
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, WireQueryError> {
        if bytes.len() < Self::SIZE {
            return Err(WireQueryError::MalformedPayload);
        }
        Ok(Self {
            query_id: u64::from_be_bytes(bytes[0..8].try_into().unwrap()),
            capability_mask: u32::from_be_bytes(bytes[8..12].try_into().unwrap()),
            min_link_quality: u16::from_be_bytes([bytes[12], bytes[13]]),
            trust_filter: bytes[14],
            max_results: u16::from_be_bytes([bytes[15], bytes[16]]),
        })
    }
}

// ------------------------------------------------------------------
// peer_query_response payload
// ------------------------------------------------------------------

/// One entry in a peer_query_response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerQueryResult {
    pub peer_id: [u8; 32],
    /// SHA-256 of the peer's callsign (from `PeerDescriptor::callsign_hash`).
    pub callsign_hash: [u8; 32],
    pub capability_mask: u32,
    pub last_seen_ms: u64,
    /// Trust state wire code: 0x00 trusted, 0x01 unknown, 0x02 untrusted, 0x03 revoked.
    pub trust_state: u8,
    /// Ed25519 signature bytes from the peer's `PeerDescriptor`.
    pub descriptor_signature: Vec<u8>,
}

impl PeerQueryResult {
    fn encode(&self) -> Vec<u8> {
        let sig_len = self.descriptor_signature.len().min(u16::MAX as usize);
        let mut buf = Vec::with_capacity(77 + 2 + sig_len);
        buf.extend_from_slice(&self.peer_id);
        buf.extend_from_slice(&self.callsign_hash);
        buf.extend_from_slice(&self.capability_mask.to_be_bytes());
        buf.extend_from_slice(&self.last_seen_ms.to_be_bytes());
        buf.push(self.trust_state);
        buf.extend_from_slice(&(sig_len as u16).to_be_bytes());
        buf.extend_from_slice(&self.descriptor_signature[..sig_len]);
        buf
    }

    fn decode(bytes: &[u8]) -> Result<(Self, usize), WireQueryError> {
        // fixed part: 32+32+4+8+1 = 77 bytes, then 2 bytes sig_len
        if bytes.len() < 79 {
            return Err(WireQueryError::MalformedPayload);
        }
        let peer_id: [u8; 32] = bytes[0..32].try_into().unwrap();
        let callsign_hash: [u8; 32] = bytes[32..64].try_into().unwrap();
        let capability_mask = u32::from_be_bytes(bytes[64..68].try_into().unwrap());
        let last_seen_ms = u64::from_be_bytes(bytes[68..76].try_into().unwrap());
        let trust_state = bytes[76];
        let sig_len = u16::from_be_bytes([bytes[77], bytes[78]]) as usize;
        let sig_end = 79 + sig_len;
        if bytes.len() < sig_end {
            return Err(WireQueryError::MalformedPayload);
        }
        let descriptor_signature = bytes[79..sig_end].to_vec();
        Ok((
            Self {
                peer_id,
                callsign_hash,
                capability_mask,
                last_seen_ms,
                trust_state,
                descriptor_signature,
            },
            sig_end,
        ))
    }
}

/// Payload for msg_type 0x02 — peer_query_response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerQueryResponse {
    pub query_id: u64,
    pub results: Vec<PeerQueryResult>,
}

impl PeerQueryResponse {
    pub fn encode(&self) -> Result<Vec<u8>, WireQueryError> {
        let result_count = self.results.len();
        if result_count > u16::MAX as usize {
            return Err(WireQueryError::PayloadTooLong);
        }
        let mut buf = Vec::new();
        buf.extend_from_slice(&self.query_id.to_be_bytes());
        buf.extend_from_slice(&(result_count as u16).to_be_bytes());
        for r in &self.results {
            buf.extend_from_slice(&r.encode());
        }
        Ok(buf)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, WireQueryError> {
        if bytes.len() < 10 {
            return Err(WireQueryError::MalformedPayload);
        }
        let query_id = u64::from_be_bytes(bytes[0..8].try_into().unwrap());
        let result_count = u16::from_be_bytes([bytes[8], bytes[9]]) as usize;

        let mut offset = 10;
        let mut results = Vec::with_capacity(result_count);
        for _ in 0..result_count {
            let (result, consumed) = PeerQueryResult::decode(&bytes[offset..])?;
            offset += consumed;
            results.push(result);
        }
        Ok(Self { query_id, results })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_envelope(msg_type: WireMsgType, payload: Vec<u8>) -> WireEnvelope {
        WireEnvelope {
            msg_type,
            flags: 0x0001,
            session_id: 0x1001,
            src_peer_id: [0xaa; 32],
            dst_peer_id: [0xbb; 32],
            nonce: [0x11; 12],
            timestamp_ms: 1_700_000_000_000,
            hop_limit: 3,
            hop_index: 0,
            payload,
            auth_tag: [0xcc; 16],
        }
    }

    #[test]
    fn envelope_round_trip_peer_query_request() {
        let req = PeerQueryRequest {
            query_id: 0x22,
            capability_mask: 0x05,
            min_link_quality: 300,
            trust_filter: 0x01,
            max_results: 32,
        };
        let env = test_envelope(WireMsgType::PeerQueryRequest, req.encode());
        let bytes = env.encode().unwrap();

        // Verify the payload_len from Example A = 17
        assert_eq!(req.encode().len(), PeerQueryRequest::SIZE);

        let decoded_env = WireEnvelope::decode(&bytes).unwrap();
        assert_eq!(decoded_env.msg_type, WireMsgType::PeerQueryRequest);
        assert_eq!(decoded_env.session_id, 0x1001);
        assert_eq!(decoded_env.hop_limit, 3);
        assert_eq!(decoded_env.hop_index, 0);
        assert_eq!(decoded_env.auth_tag, [0xcc; 16]);

        let decoded_req = PeerQueryRequest::decode(&decoded_env.payload).unwrap();
        assert_eq!(decoded_req, req);
    }

    #[test]
    fn envelope_round_trip_peer_query_response() {
        let result = PeerQueryResult {
            peer_id: [0xdd; 32],
            callsign_hash: [0xee; 32],
            capability_mask: 0x0003,
            last_seen_ms: 1_700_000_000_001,
            trust_state: 0x00,
            descriptor_signature: vec![0xf0; 64],
        };
        let resp = PeerQueryResponse {
            query_id: 0x42,
            results: vec![result],
        };
        let env = test_envelope(WireMsgType::PeerQueryResponse, resp.encode().unwrap());
        let bytes = env.encode().unwrap();

        let decoded_env = WireEnvelope::decode(&bytes).unwrap();
        assert_eq!(decoded_env.msg_type, WireMsgType::PeerQueryResponse);

        let decoded_resp = PeerQueryResponse::decode(&decoded_env.payload).unwrap();
        assert_eq!(decoded_resp, resp);
    }

    #[test]
    fn envelope_rejects_invalid_magic() {
        let mut bytes = test_envelope(WireMsgType::PeerQueryRequest, vec![])
            .encode()
            .unwrap();
        bytes[0] = 0xFF;
        assert!(matches!(
            WireEnvelope::decode(&bytes),
            Err(WireQueryError::InvalidMagic)
        ));
    }

    #[test]
    fn envelope_rejects_unknown_msg_type() {
        let mut bytes = test_envelope(WireMsgType::PeerQueryRequest, vec![])
            .encode()
            .unwrap();
        bytes[5] = 0xFF;
        assert!(matches!(
            WireEnvelope::decode(&bytes),
            Err(WireQueryError::UnknownMsgType(0xFF))
        ));
    }

    #[test]
    fn envelope_rejects_truncated_buffer() {
        let bytes = test_envelope(WireMsgType::PeerQueryRequest, vec![0xAB; 17])
            .encode()
            .unwrap();
        assert!(matches!(
            WireEnvelope::decode(&bytes[..50]),
            Err(WireQueryError::BufferTooShort)
        ));
    }

    #[test]
    fn peer_query_request_example_a_layout() {
        // Verifies the exact layout from docs/peer-query-relay-wire.md Example A
        let req = PeerQueryRequest {
            query_id: 0x22,
            capability_mask: 0x05,
            min_link_quality: 300,
            trust_filter: 0x01,
            max_results: 32,
        };
        let payload = req.encode();
        assert_eq!(payload.len(), 17);
        assert_eq!(&payload[0..8], &0x22u64.to_be_bytes());
        assert_eq!(&payload[8..12], &0x05u32.to_be_bytes());
        assert_eq!(&payload[12..14], &300u16.to_be_bytes());
        assert_eq!(payload[14], 0x01);
        assert_eq!(&payload[15..17], &32u16.to_be_bytes());
    }

    #[test]
    fn response_with_empty_results() {
        let resp = PeerQueryResponse {
            query_id: 99,
            results: vec![],
        };
        let payload = resp.encode().unwrap();
        let decoded = PeerQueryResponse::decode(&payload).unwrap();
        assert_eq!(decoded.query_id, 99);
        assert!(decoded.results.is_empty());
    }
}
