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
    #[error("hop count exceeds maximum")]
    HopCountExceeded,
    #[error("signature too large to encode")]
    SignatureTooLarge,
}

/// Message type codes for the OPHF wire envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WireMsgType {
    PeerQueryRequest = 0x01,
    PeerQueryResponse = 0x02,
    RouteDiscoveryRequest = 0x03,
    RouteDiscoveryResponse = 0x04,
    RelayDataChunk = 0x05,
    RelayHopAck = 0x06,
    RelayRouteUpdate = 0x07,
    RelayRouteReject = 0x08,
}

impl WireMsgType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x01 => Some(Self::PeerQueryRequest),
            0x02 => Some(Self::PeerQueryResponse),
            0x03 => Some(Self::RouteDiscoveryRequest),
            0x04 => Some(Self::RouteDiscoveryResponse),
            0x05 => Some(Self::RelayDataChunk),
            0x06 => Some(Self::RelayHopAck),
            0x07 => Some(Self::RelayRouteUpdate),
            0x08 => Some(Self::RelayRouteReject),
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

// ------------------------------------------------------------------
// relay_data_chunk payload (msg_type 0x05)
// ------------------------------------------------------------------

/// Payload for msg_type 0x05 — relay_data_chunk.
///
/// Fixed header: transfer_id(8) | chunk_seq(4) | total_chunks(4) |
///               chunk_len(2) | chunk_hash(32) | e2e_manifest_hash(32) = 82 bytes,
/// then variable: chunk_signature (u16-prefixed) + chunk_data (remainder).
///
/// Relays must not mutate `chunk_hash`, `e2e_manifest_hash`, or `chunk_signature`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayDataChunk {
    pub transfer_id: u64,
    pub chunk_seq: u32,
    pub total_chunks: u32,
    pub chunk_hash: [u8; 32],
    pub e2e_manifest_hash: [u8; 32],
    pub chunk_signature: Vec<u8>,
    pub chunk_data: Vec<u8>,
}

impl RelayDataChunk {
    pub fn encode(&self) -> Result<Vec<u8>, WireQueryError> {
        let sig_len = self.chunk_signature.len();
        let data_len = self.chunk_data.len();
        if sig_len > u16::MAX as usize || data_len > u16::MAX as usize {
            return Err(WireQueryError::PayloadTooLong);
        }
        let mut buf = Vec::with_capacity(82 + 2 + sig_len + 2 + data_len);
        buf.extend_from_slice(&self.transfer_id.to_be_bytes());
        buf.extend_from_slice(&self.chunk_seq.to_be_bytes());
        buf.extend_from_slice(&self.total_chunks.to_be_bytes());
        buf.extend_from_slice(&(data_len as u16).to_be_bytes());
        buf.extend_from_slice(&self.chunk_hash);
        buf.extend_from_slice(&self.e2e_manifest_hash);
        buf.extend_from_slice(&(sig_len as u16).to_be_bytes());
        buf.extend_from_slice(&self.chunk_signature);
        buf.extend_from_slice(&self.chunk_data);
        Ok(buf)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, WireQueryError> {
        // Fixed: 8+4+4+2+32+32 = 82 bytes before sig_len
        if bytes.len() < 84 {
            return Err(WireQueryError::MalformedPayload);
        }
        let transfer_id = u64::from_be_bytes(bytes[0..8].try_into().unwrap());
        let chunk_seq = u32::from_be_bytes(bytes[8..12].try_into().unwrap());
        let total_chunks = u32::from_be_bytes(bytes[12..16].try_into().unwrap());
        let chunk_len = u16::from_be_bytes([bytes[16], bytes[17]]) as usize;
        let chunk_hash: [u8; 32] = bytes[18..50].try_into().unwrap();
        let e2e_manifest_hash: [u8; 32] = bytes[50..82].try_into().unwrap();
        let sig_len = u16::from_be_bytes([bytes[82], bytes[83]]) as usize;
        let sig_end = 84 + sig_len;
        if bytes.len() < sig_end + chunk_len {
            return Err(WireQueryError::MalformedPayload);
        }
        let chunk_signature = bytes[84..sig_end].to_vec();
        let chunk_data = bytes[sig_end..sig_end + chunk_len].to_vec();
        Ok(Self {
            transfer_id,
            chunk_seq,
            total_chunks,
            chunk_hash,
            e2e_manifest_hash,
            chunk_signature,
            chunk_data,
        })
    }
}

// ------------------------------------------------------------------
// relay_hop_ack payload (msg_type 0x06)
// ------------------------------------------------------------------

/// ACK status codes for `RelayHopAck`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AckStatus {
    Ok = 0x00,
    Retry = 0x01,
    Reject = 0x02,
}

impl AckStatus {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x00 => Some(Self::Ok),
            0x01 => Some(Self::Retry),
            0x02 => Some(Self::Reject),
            _ => None,
        }
    }
}

/// Payload for msg_type 0x06 — relay_hop_ack.
///
/// Fixed: transfer_id(8) | chunk_seq(4) | hop_peer_id(32) |
///        ack_status(1) | retry_after_ms(2) | reason_code(2) = 49 bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayHopAck {
    pub transfer_id: u64,
    pub chunk_seq: u32,
    pub hop_peer_id: [u8; 32],
    pub ack_status: AckStatus,
    pub retry_after_ms: u16,
    pub reason_code: u16,
}

impl RelayHopAck {
    pub const SIZE: usize = 49;

    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::SIZE);
        buf.extend_from_slice(&self.transfer_id.to_be_bytes());
        buf.extend_from_slice(&self.chunk_seq.to_be_bytes());
        buf.extend_from_slice(&self.hop_peer_id);
        buf.push(self.ack_status as u8);
        buf.extend_from_slice(&self.retry_after_ms.to_be_bytes());
        buf.extend_from_slice(&self.reason_code.to_be_bytes());
        buf
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, WireQueryError> {
        if bytes.len() < Self::SIZE {
            return Err(WireQueryError::MalformedPayload);
        }
        let transfer_id = u64::from_be_bytes(bytes[0..8].try_into().unwrap());
        let chunk_seq = u32::from_be_bytes(bytes[8..12].try_into().unwrap());
        let hop_peer_id: [u8; 32] = bytes[12..44].try_into().unwrap();
        let ack_status = AckStatus::from_u8(bytes[44]).ok_or(WireQueryError::MalformedPayload)?;
        let retry_after_ms = u16::from_be_bytes([bytes[45], bytes[46]]);
        let reason_code = u16::from_be_bytes([bytes[47], bytes[48]]);
        Ok(Self {
            transfer_id,
            chunk_seq,
            hop_peer_id,
            ack_status,
            retry_after_ms,
            reason_code,
        })
    }
}

// ------------------------------------------------------------------
// route_discovery_request payload (msg_type 0x03)
// ------------------------------------------------------------------

/// Trust state codes used in route discovery and route reject payloads.
/// Distinct from `trust::TrustDecision` which is the policy evaluation result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WireTrustState {
    Trusted = 0x00,
    Unknown = 0x01,
    Untrusted = 0x02,
    Revoked = 0x03,
}

impl WireTrustState {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x00 => Some(Self::Trusted),
            0x01 => Some(Self::Unknown),
            0x02 => Some(Self::Untrusted),
            0x03 => Some(Self::Revoked),
            _ => None,
        }
    }
}

/// Reason codes for route updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum RouteChangeReason {
    LinkQualityDegraded = 0x0001,
    HopUnreachable = 0x0002,
    TrustPolicyChange = 0x0003,
    OperatorOverride = 0x0004,
    RouteOptimization = 0x0005,
}

/// One hop entry in a route discovery response or route update payload.
///
/// Encoded: hop_peer_id(32) | hop_trust_state(1) | estimated_latency_ms(2) |
///          estimated_reliability_permille(2) = 37 bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteHop {
    pub hop_peer_id: [u8; 32],
    /// Wire code per `WireTrustState`.
    pub hop_trust_state: u8,
    pub estimated_latency_ms: u16,
    pub estimated_reliability_permille: u16,
}

impl RouteHop {
    pub const SIZE: usize = 37;

    fn encode(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        buf[0..32].copy_from_slice(&self.hop_peer_id);
        buf[32] = self.hop_trust_state;
        buf[33..35].copy_from_slice(&self.estimated_latency_ms.to_be_bytes());
        buf[35..37].copy_from_slice(&self.estimated_reliability_permille.to_be_bytes());
        buf
    }

    fn decode(bytes: &[u8]) -> Result<Self, WireQueryError> {
        if bytes.len() < Self::SIZE {
            return Err(WireQueryError::MalformedPayload);
        }
        Ok(Self {
            hop_peer_id: bytes[0..32].try_into().unwrap(),
            hop_trust_state: bytes[32],
            estimated_latency_ms: u16::from_be_bytes([bytes[33], bytes[34]]),
            estimated_reliability_permille: u16::from_be_bytes([bytes[35], bytes[36]]),
        })
    }
}

/// Payload for msg_type 0x03 — route_discovery_request.
///
/// Fixed: route_query_id(8) | destination_peer_id(32) | max_hops(1) |
///        required_capability_mask(4) | policy_flags(2) = 47 bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteDiscoveryRequest {
    pub route_query_id: u64,
    pub destination_peer_id: [u8; 32],
    pub max_hops: u8,
    pub required_capability_mask: u32,
    pub policy_flags: u16,
}

impl RouteDiscoveryRequest {
    pub const SIZE: usize = 47;

    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::SIZE);
        buf.extend_from_slice(&self.route_query_id.to_be_bytes());
        buf.extend_from_slice(&self.destination_peer_id);
        buf.push(self.max_hops);
        buf.extend_from_slice(&self.required_capability_mask.to_be_bytes());
        buf.extend_from_slice(&self.policy_flags.to_be_bytes());
        buf
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, WireQueryError> {
        if bytes.len() < Self::SIZE {
            return Err(WireQueryError::MalformedPayload);
        }
        Ok(Self {
            route_query_id: u64::from_be_bytes(bytes[0..8].try_into().unwrap()),
            destination_peer_id: bytes[8..40].try_into().unwrap(),
            max_hops: bytes[40],
            required_capability_mask: u32::from_be_bytes(bytes[41..45].try_into().unwrap()),
            policy_flags: u16::from_be_bytes([bytes[45], bytes[46]]),
        })
    }
}

// ------------------------------------------------------------------
// route_discovery_response payload (msg_type 0x04)
// ------------------------------------------------------------------

/// Payload for msg_type 0x04 — route_discovery_response.
///
/// Variable: route_query_id(8) | route_id(8) | hop_count(1) |
///           hops[hop_count × 37] | sig_len(2) | route_signature[sig_len].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteDiscoveryResponse {
    pub route_query_id: u64,
    pub route_id: u64,
    pub hops: Vec<RouteHop>,
    pub route_signature: Vec<u8>,
}

impl RouteDiscoveryResponse {
    pub fn encode(&self) -> Result<Vec<u8>, WireQueryError> {
        if self.hops.len() > 8 {
            return Err(WireQueryError::HopCountExceeded);
        }
        let sig_len = self.route_signature.len();
        if sig_len > u16::MAX as usize {
            return Err(WireQueryError::SignatureTooLarge);
        }
        let mut buf = Vec::with_capacity(17 + self.hops.len() * RouteHop::SIZE + 2 + sig_len);
        buf.extend_from_slice(&self.route_query_id.to_be_bytes());
        buf.extend_from_slice(&self.route_id.to_be_bytes());
        buf.push(self.hops.len() as u8);
        for hop in &self.hops {
            buf.extend_from_slice(&hop.encode());
        }
        buf.extend_from_slice(&(sig_len as u16).to_be_bytes());
        buf.extend_from_slice(&self.route_signature);
        Ok(buf)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, WireQueryError> {
        // Minimum: 8 + 8 + 1 + 0 hops + 2 sig_len = 19 bytes
        if bytes.len() < 19 {
            return Err(WireQueryError::MalformedPayload);
        }
        let route_query_id = u64::from_be_bytes(bytes[0..8].try_into().unwrap());
        let route_id = u64::from_be_bytes(bytes[8..16].try_into().unwrap());
        let hop_count = bytes[16] as usize;

        let hops_end = 17 + hop_count * RouteHop::SIZE;
        if bytes.len() < hops_end + 2 {
            return Err(WireQueryError::MalformedPayload);
        }
        let mut hops = Vec::with_capacity(hop_count);
        for i in 0..hop_count {
            let offset = 17 + i * RouteHop::SIZE;
            hops.push(RouteHop::decode(&bytes[offset..])?);
        }

        let sig_len = u16::from_be_bytes([bytes[hops_end], bytes[hops_end + 1]]) as usize;
        let sig_end = hops_end + 2 + sig_len;
        if bytes.len() < sig_end {
            return Err(WireQueryError::MalformedPayload);
        }
        let route_signature = bytes[hops_end + 2..sig_end].to_vec();
        Ok(Self {
            route_query_id,
            route_id,
            hops,
            route_signature,
        })
    }
}

// ------------------------------------------------------------------
// relay_route_update payload (msg_type 0x07)
// ------------------------------------------------------------------

/// Payload for msg_type 0x07 — relay_route_update.
///
/// Variable: route_id(8) | previous_hop_count(1) | new_hop_count(1) |
///           route_change_reason(2) | replacement_hops[new_hop_count × 37] |
///           sig_len(2) | route_update_signature[sig_len].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayRouteUpdate {
    pub route_id: u64,
    pub previous_hop_count: u8,
    pub route_change_reason: u16,
    pub replacement_hops: Vec<RouteHop>,
    pub route_update_signature: Vec<u8>,
}

impl RelayRouteUpdate {
    pub fn encode(&self) -> Result<Vec<u8>, WireQueryError> {
        if self.replacement_hops.len() > u8::MAX as usize {
            return Err(WireQueryError::HopCountExceeded);
        }
        let sig_len = self.route_update_signature.len();
        if sig_len > u16::MAX as usize {
            return Err(WireQueryError::SignatureTooLarge);
        }
        let new_hop_count = self.replacement_hops.len();
        let mut buf = Vec::with_capacity(12 + new_hop_count * RouteHop::SIZE + 2 + sig_len);
        buf.extend_from_slice(&self.route_id.to_be_bytes());
        buf.push(self.previous_hop_count);
        buf.push(new_hop_count as u8);
        buf.extend_from_slice(&self.route_change_reason.to_be_bytes());
        for hop in &self.replacement_hops {
            buf.extend_from_slice(&hop.encode());
        }
        buf.extend_from_slice(&(sig_len as u16).to_be_bytes());
        buf.extend_from_slice(&self.route_update_signature);
        Ok(buf)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, WireQueryError> {
        // Minimum: 8 + 1 + 1 + 2 + 0 hops + 2 sig_len = 14 bytes
        if bytes.len() < 14 {
            return Err(WireQueryError::MalformedPayload);
        }
        let route_id = u64::from_be_bytes(bytes[0..8].try_into().unwrap());
        let previous_hop_count = bytes[8];
        let new_hop_count = bytes[9] as usize;
        let route_change_reason = u16::from_be_bytes([bytes[10], bytes[11]]);

        let hops_end = 12 + new_hop_count * RouteHop::SIZE;
        if bytes.len() < hops_end + 2 {
            return Err(WireQueryError::MalformedPayload);
        }
        let mut replacement_hops = Vec::with_capacity(new_hop_count);
        for i in 0..new_hop_count {
            let offset = 12 + i * RouteHop::SIZE;
            replacement_hops.push(RouteHop::decode(&bytes[offset..])?);
        }

        let sig_len = u16::from_be_bytes([bytes[hops_end], bytes[hops_end + 1]]) as usize;
        let sig_end = hops_end + 2 + sig_len;
        if bytes.len() < sig_end {
            return Err(WireQueryError::MalformedPayload);
        }
        let route_update_signature = bytes[hops_end + 2..sig_end].to_vec();
        Ok(Self {
            route_id,
            previous_hop_count,
            route_change_reason,
            replacement_hops,
            route_update_signature,
        })
    }
}

// ------------------------------------------------------------------
// relay_route_reject payload (msg_type 0x08)
// ------------------------------------------------------------------

/// Payload for msg_type 0x08 — relay_route_reject.
///
/// Fixed: route_id(8) | reject_hop_peer_id(32) | reason_code(2) |
///        trust_decision(1) | policy_reference(2) = 45 bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayRouteReject {
    pub route_id: u64,
    pub reject_hop_peer_id: [u8; 32],
    pub reason_code: u16,
    /// Wire code per `WireTrustState`.
    pub trust_decision: u8,
    pub policy_reference: u16,
}

impl RelayRouteReject {
    pub const SIZE: usize = 45;

    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::SIZE);
        buf.extend_from_slice(&self.route_id.to_be_bytes());
        buf.extend_from_slice(&self.reject_hop_peer_id);
        buf.extend_from_slice(&self.reason_code.to_be_bytes());
        buf.push(self.trust_decision);
        buf.extend_from_slice(&self.policy_reference.to_be_bytes());
        buf
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, WireQueryError> {
        if bytes.len() < Self::SIZE {
            return Err(WireQueryError::MalformedPayload);
        }
        Ok(Self {
            route_id: u64::from_be_bytes(bytes[0..8].try_into().unwrap()),
            reject_hop_peer_id: bytes[8..40].try_into().unwrap(),
            reason_code: u16::from_be_bytes([bytes[40], bytes[41]]),
            trust_decision: bytes[42],
            policy_reference: u16::from_be_bytes([bytes[43], bytes[44]]),
        })
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
