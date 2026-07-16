use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
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
    #[error("src_peer_id is not a valid ed25519 verifying key")]
    InvalidSrcKey,
    #[error("envelope origin signature is invalid")]
    InvalidSignature,
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
    /// Signed remote rig-control command (Phase 7.5).
    RigCtrlCmd = 0x09,
    /// One-to-many unacknowledged broadcast frame (Phase 9.5).
    BroadcastFrame = 0x0A,
}

impl WireMsgType {
    /// Map a wire byte to the corresponding variant; returns `None` for unknown values.
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
            0x09 => Some(Self::RigCtrlCmd),
            0x0A => Some(Self::BroadcastFrame),
            _ => None,
        }
    }
}

const MAGIC: &[u8; 4] = b"OPHF";
/// Wire schema version. v2 replaced the unauthenticated fixed 16-byte `auth_tag` with an *optional*
/// 64-byte Ed25519 origin `signature` verifiable against `src_peer_id` (E3): signed frames append 64
/// bytes, unsigned frames append none.
const VERSION: u8 = 2;
/// Byte count of fixed envelope fields excluding payload and the optional signature.
const HEADER_SIZE: usize = 104;
/// Ed25519 signature length; the envelope's origin authenticator when present.
const SIGNATURE_SIZE: usize = 64;
/// Offset of the relay-mutated `hop_index` byte, excluded from the signed region.
const HOP_INDEX_OFFSET: usize = 101;

fn read_u64(bytes: &[u8], off: usize) -> Result<u64, WireQueryError> {
    Ok(u64::from_be_bytes(
        bytes[off..off + 8]
            .try_into()
            .map_err(|_| WireQueryError::MalformedPayload)?,
    ))
}

fn read_u32(bytes: &[u8], off: usize) -> Result<u32, WireQueryError> {
    Ok(u32::from_be_bytes(
        bytes[off..off + 4]
            .try_into()
            .map_err(|_| WireQueryError::MalformedPayload)?,
    ))
}

fn read_u16(bytes: &[u8], off: usize) -> Result<u16, WireQueryError> {
    if bytes.len() < off + 2 {
        return Err(WireQueryError::MalformedPayload);
    }
    Ok(u16::from_be_bytes([bytes[off], bytes[off + 1]]))
}

fn read_arr32(bytes: &[u8], off: usize) -> Result<[u8; 32], WireQueryError> {
    bytes[off..off + 32]
        .try_into()
        .map_err(|_| WireQueryError::MalformedPayload)
}

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
    /// Optional Ed25519 origin signature over every field except `hop_index` (relay-mutated)
    /// and this field, verifiable against `src_peer_id`. `Some` appends 64 bytes to the wire form;
    /// `None` (unsigned) appends nothing, keeping control frames that carry their own payload-level
    /// signatures compact. The relay requires `Some` on frames it forwards (E3).
    pub signature: Option<[u8; 64]>,
}

impl WireEnvelope {
    /// Header (104 bytes) + payload, without the trailing signature.
    fn header_and_payload(&self) -> Result<Vec<u8>, WireQueryError> {
        let payload_len = self.payload.len();
        if payload_len > u16::MAX as usize {
            return Err(WireQueryError::PayloadTooLong);
        }
        let mut buf = Vec::with_capacity(HEADER_SIZE + payload_len + SIGNATURE_SIZE);
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
        Ok(buf)
    }

    /// Canonical byte sequence covered by the origin signature: the full header
    /// and payload with `hop_index` zeroed, so a relay incrementing `hop_index`
    /// does not invalidate the originator's signature.
    fn signing_bytes(&self) -> Result<Vec<u8>, WireQueryError> {
        let mut buf = self.header_and_payload()?;
        buf[HOP_INDEX_OFFSET] = 0;
        Ok(buf)
    }

    /// Serialize to `OPHF` wire format: header (104 bytes) + payload, plus a 64-byte signature when
    /// the envelope is signed. Unsigned envelopes carry no trailing signature.
    pub fn encode(&self) -> Result<Vec<u8>, WireQueryError> {
        let mut buf = self.header_and_payload()?;
        if let Some(sig) = &self.signature {
            buf.extend_from_slice(sig);
        }
        Ok(buf)
    }

    /// Sign the envelope's canonical region with `signing_key_seed`, storing a 64-byte Ed25519
    /// signature. The seed's verifying key must equal `src_peer_id` for the signature to verify.
    pub fn sign(&mut self, signing_key_seed: &[u8; 32]) -> Result<(), WireQueryError> {
        let msg = self.signing_bytes()?;
        let key = SigningKey::from_bytes(signing_key_seed);
        let sig: Signature = key.sign(&msg);
        self.signature = Some(sig.to_bytes());
        Ok(())
    }

    /// Verify the origin signature against `src_peer_id` (which is the originator's Ed25519 verifying
    /// key). Self-authenticating: no external key store is needed. An unsigned envelope fails.
    pub fn verify_origin(&self) -> Result<(), WireQueryError> {
        let sig_bytes = self.signature.ok_or(WireQueryError::InvalidSignature)?;
        let key = VerifyingKey::from_bytes(&self.src_peer_id)
            .map_err(|_| WireQueryError::InvalidSrcKey)?;
        let sig = Signature::from_bytes(&sig_bytes);
        let msg = self.signing_bytes()?;
        key.verify_strict(&msg, &sig)
            .map_err(|_| WireQueryError::InvalidSignature)
    }

    /// Deserialize from `OPHF` wire format; checks magic, msg_type, and payload length. The signature
    /// is present iff exactly 64 bytes trail the payload (the envelope is length-delimited by its
    /// carrying `Frame`); no trailing bytes means unsigned.
    pub fn decode(bytes: &[u8]) -> Result<Self, WireQueryError> {
        if bytes.len() < HEADER_SIZE {
            return Err(WireQueryError::BufferTooShort);
        }
        if &bytes[0..4] != MAGIC {
            return Err(WireQueryError::InvalidMagic);
        }
        // bytes[4] = version; forward-compatible: parse but don't reject unknown versions here
        let msg_type =
            WireMsgType::from_u8(bytes[5]).ok_or(WireQueryError::UnknownMsgType(bytes[5]))?;
        let flags = read_u16(bytes, 6)?;
        let session_id = read_u64(bytes, 8)?;
        let src_peer_id = read_arr32(bytes, 16)?;
        let dst_peer_id = read_arr32(bytes, 48)?;
        let nonce: [u8; 12] = bytes[80..92]
            .try_into()
            .map_err(|_| WireQueryError::MalformedPayload)?;
        let timestamp_ms = read_u64(bytes, 92)?;
        let hop_limit = bytes[100];
        let hop_index = bytes[101];
        let payload_len = read_u16(bytes, 102)? as usize;

        let payload_start = 104;
        let payload_end = payload_start + payload_len;
        if bytes.len() < payload_end {
            return Err(WireQueryError::BufferTooShort);
        }
        let payload = bytes[payload_start..payload_end].to_vec();

        // The signature is present iff exactly 64 bytes trail the payload; any other trailer length
        // is a truncated or corrupt frame.
        let signature = match bytes.len() - payload_end {
            0 => None,
            SIGNATURE_SIZE => Some(
                bytes[payload_end..payload_end + SIGNATURE_SIZE]
                    .try_into()
                    .map_err(|_| WireQueryError::MalformedPayload)?,
            ),
            _ => return Err(WireQueryError::BufferTooShort),
        };

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
            signature,
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

    /// Serialize to the 17-byte wire layout.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::SIZE);
        buf.extend_from_slice(&self.query_id.to_be_bytes());
        buf.extend_from_slice(&self.capability_mask.to_be_bytes());
        buf.extend_from_slice(&self.min_link_quality.to_be_bytes());
        buf.push(self.trust_filter);
        buf.extend_from_slice(&self.max_results.to_be_bytes());
        buf
    }

    /// Deserialize from the 17-byte wire layout.
    pub fn decode(bytes: &[u8]) -> Result<Self, WireQueryError> {
        if bytes.len() < Self::SIZE {
            return Err(WireQueryError::MalformedPayload);
        }
        Ok(Self {
            query_id: read_u64(bytes, 0)?,
            capability_mask: read_u32(bytes, 8)?,
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

/// Minimum encoded length of a [`PeerQueryResult`]: the 77-byte fixed part + a 2-byte signature length.
const PEER_QUERY_RESULT_MIN_SIZE: usize = 79;

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
        if bytes.len() < PEER_QUERY_RESULT_MIN_SIZE {
            return Err(WireQueryError::MalformedPayload);
        }
        let peer_id = read_arr32(bytes, 0)?;
        let callsign_hash = read_arr32(bytes, 32)?;
        let capability_mask = read_u32(bytes, 64)?;
        let last_seen_ms = read_u64(bytes, 68)?;
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
    /// Serialize to the variable-length wire layout.
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

    /// Deserialize from the variable-length wire layout.
    pub fn decode(bytes: &[u8]) -> Result<Self, WireQueryError> {
        if bytes.len() < 10 {
            return Err(WireQueryError::MalformedPayload);
        }
        let query_id = read_u64(bytes, 0)?;
        let result_count = u16::from_be_bytes([bytes[8], bytes[9]]) as usize;

        let mut offset = 10;
        // Bound the pre-allocation by the bytes actually present (each result is ≥ 79 B) so an
        // attacker-controlled count can't drive a multi-MB allocation from a tiny frame (audit F-4);
        // the loop still bails on the first short record. Mirrors the sibling variable-length decoders.
        let capacity =
            result_count.min(bytes.len().saturating_sub(10) / PEER_QUERY_RESULT_MIN_SIZE);
        let mut results = Vec::with_capacity(capacity);
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
    /// Serialize to the variable-length wire layout.
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

    /// Deserialize from the variable-length wire layout.
    pub fn decode(bytes: &[u8]) -> Result<Self, WireQueryError> {
        // Fixed: 8+4+4+2+32+32 = 82 bytes before sig_len
        if bytes.len() < 84 {
            return Err(WireQueryError::MalformedPayload);
        }
        let transfer_id = read_u64(bytes, 0)?;
        let chunk_seq = read_u32(bytes, 8)?;
        let total_chunks = read_u32(bytes, 12)?;
        let chunk_len = u16::from_be_bytes([bytes[16], bytes[17]]) as usize;
        let chunk_hash = read_arr32(bytes, 18)?;
        let e2e_manifest_hash = read_arr32(bytes, 50)?;
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
    /// Map a wire byte to the corresponding variant; returns `None` for unknown values.
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
    /// Encoded size in bytes.
    pub const SIZE: usize = 49;

    /// Serialize to the 49-byte wire layout.
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

    /// Deserialize from the 49-byte wire layout.
    pub fn decode(bytes: &[u8]) -> Result<Self, WireQueryError> {
        if bytes.len() < Self::SIZE {
            return Err(WireQueryError::MalformedPayload);
        }
        let transfer_id = read_u64(bytes, 0)?;
        let chunk_seq = read_u32(bytes, 8)?;
        let hop_peer_id = read_arr32(bytes, 12)?;
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
    /// Map a wire byte to the corresponding variant; returns `None` for unknown values.
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
    /// Encoded size in bytes.
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
            hop_peer_id: read_arr32(bytes, 0)?,
            hop_trust_state: bytes[32],
            estimated_latency_ms: u16::from_be_bytes([bytes[33], bytes[34]]),
            estimated_reliability_permille: u16::from_be_bytes([bytes[35], bytes[36]]),
        })
    }
}

/// Payload for msg_type 0x03 — route_discovery_request.
///
/// Header (47 B): route_query_id(8) | destination_peer_id(32) | max_hops(1) |
///        required_capability_mask(4) | policy_flags(2), then the source-accumulated path:
///        path_count(1) | path[path_count × 32]. A header-only (47 B) frame decodes to an empty path
///        (backward compatible with pre-path encoders).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteDiscoveryRequest {
    pub route_query_id: u64,
    pub destination_peer_id: [u8; 32],
    pub max_hops: u8,
    pub required_capability_mask: u32,
    pub policy_flags: u16,
    /// Peer ids this request has been forwarded through, in order (originator → …). Empty when
    /// originated; each forwarding node appends its own id before re-flooding, so the answerer can reply
    /// with the true multi-hop route instead of only what it can locally vouch for.
    pub accumulated_path: Vec<[u8; 32]>,
}

impl RouteDiscoveryRequest {
    /// Fixed-header size in bytes (before the accumulated path).
    pub const HEADER_SIZE: usize = 47;

    /// Serialize to the wire layout (header + accumulated path).
    pub fn encode(&self) -> Vec<u8> {
        let n = self.accumulated_path.len().min(u8::MAX as usize);
        let mut buf = Vec::with_capacity(Self::HEADER_SIZE + 1 + n * 32);
        buf.extend_from_slice(&self.route_query_id.to_be_bytes());
        buf.extend_from_slice(&self.destination_peer_id);
        buf.push(self.max_hops);
        buf.extend_from_slice(&self.required_capability_mask.to_be_bytes());
        buf.extend_from_slice(&self.policy_flags.to_be_bytes());
        buf.push(n as u8);
        for id in self.accumulated_path.iter().take(n) {
            buf.extend_from_slice(id);
        }
        buf
    }

    /// Deserialize. A 47-byte (header-only) frame yields an empty accumulated path.
    pub fn decode(bytes: &[u8]) -> Result<Self, WireQueryError> {
        if bytes.len() < Self::HEADER_SIZE {
            return Err(WireQueryError::MalformedPayload);
        }
        let mut accumulated_path = Vec::new();
        if bytes.len() > Self::HEADER_SIZE {
            let count = bytes[Self::HEADER_SIZE] as usize;
            let start = Self::HEADER_SIZE + 1;
            if bytes.len() < start + count * 32 {
                return Err(WireQueryError::MalformedPayload);
            }
            for i in 0..count {
                accumulated_path.push(read_arr32(bytes, start + i * 32)?);
            }
        }
        Ok(Self {
            route_query_id: read_u64(bytes, 0)?,
            destination_peer_id: read_arr32(bytes, 8)?,
            max_hops: bytes[40],
            required_capability_mask: read_u32(bytes, 41)?,
            policy_flags: u16::from_be_bytes([bytes[45], bytes[46]]),
            accumulated_path,
        })
    }
}

// ------------------------------------------------------------------
// route_discovery_response payload (msg_type 0x04)
// ------------------------------------------------------------------

/// Payload for msg_type 0x04 — route_discovery_response.
///
/// Variable layout: `route_query_id(8) | route_id(8) | hop_count(1) |`
/// `hops[hop_count × 37] | sig_len(2) | route_signature[sig_len]`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteDiscoveryResponse {
    pub route_query_id: u64,
    pub route_id: u64,
    pub hops: Vec<RouteHop>,
    pub route_signature: Vec<u8>,
}

impl RouteDiscoveryResponse {
    /// Serialize to the variable-length wire layout.
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

    /// Deserialize from the variable-length wire layout.
    pub fn decode(bytes: &[u8]) -> Result<Self, WireQueryError> {
        // Minimum: 8 + 8 + 1 + 0 hops + 2 sig_len = 19 bytes
        if bytes.len() < 19 {
            return Err(WireQueryError::MalformedPayload);
        }
        let route_query_id = read_u64(bytes, 0)?;
        let route_id = read_u64(bytes, 8)?;
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
/// Variable layout: `route_id(8) | previous_hop_count(1) | new_hop_count(1) |`
/// `route_change_reason(2) | replacement_hops[new_hop_count × 37] |`
/// `sig_len(2) | route_update_signature[sig_len]`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayRouteUpdate {
    pub route_id: u64,
    pub previous_hop_count: u8,
    pub route_change_reason: u16,
    pub replacement_hops: Vec<RouteHop>,
    pub route_update_signature: Vec<u8>,
}

impl RelayRouteUpdate {
    /// Serialize to the variable-length wire layout.
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

    /// Deserialize from the variable-length wire layout.
    pub fn decode(bytes: &[u8]) -> Result<Self, WireQueryError> {
        // Minimum: 8 + 1 + 1 + 2 + 0 hops + 2 sig_len = 14 bytes
        if bytes.len() < 14 {
            return Err(WireQueryError::MalformedPayload);
        }
        let route_id = read_u64(bytes, 0)?;
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
    /// Encoded size in bytes.
    pub const SIZE: usize = 45;

    /// Serialize to the 45-byte wire layout.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::SIZE);
        buf.extend_from_slice(&self.route_id.to_be_bytes());
        buf.extend_from_slice(&self.reject_hop_peer_id);
        buf.extend_from_slice(&self.reason_code.to_be_bytes());
        buf.push(self.trust_decision);
        buf.extend_from_slice(&self.policy_reference.to_be_bytes());
        buf
    }

    /// Deserialize from the 45-byte wire layout.
    pub fn decode(bytes: &[u8]) -> Result<Self, WireQueryError> {
        if bytes.len() < Self::SIZE {
            return Err(WireQueryError::MalformedPayload);
        }
        Ok(Self {
            route_id: read_u64(bytes, 0)?,
            reject_hop_peer_id: read_arr32(bytes, 8)?,
            reason_code: u16::from_be_bytes([bytes[40], bytes[41]]),
            trust_decision: bytes[42],
            policy_reference: u16::from_be_bytes([bytes[43], bytes[44]]),
        })
    }
}

// ------------------------------------------------------------------
// broadcast_frame payload (msg_type 0x0A)
// ------------------------------------------------------------------

/// Payload for msg_type 0x0A — broadcast_frame.
///
/// Header: callsign_hash(4) | seq(2) | ttl(1) | flags(1) = 8 bytes,
/// followed by variable-length payload bytes.
/// No ACK is expected; no session state is required.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BroadcastFrame {
    /// FNV-1a hash of the sender's callsign (for display / dedup without full identity).
    pub callsign_hash: u32,
    /// Sender-local sequence number for duplicate suppression.
    pub seq: u16,
    /// Remaining TTL hops; relay nodes decrement before re-transmitting.
    pub ttl: u8,
    /// Reserved flags byte (0 for now).
    pub flags: u8,
    /// Arbitrary payload bytes (station ID text, position, network announcement, …).
    pub payload: Vec<u8>,
}

impl BroadcastFrame {
    /// Byte count of the fixed header before the payload bytes.
    pub const HEADER_SIZE: usize = 8;

    /// Serialize to the variable-length wire layout.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::HEADER_SIZE + self.payload.len());
        buf.extend_from_slice(&self.callsign_hash.to_be_bytes());
        buf.extend_from_slice(&self.seq.to_be_bytes());
        buf.push(self.ttl);
        buf.push(self.flags);
        buf.extend_from_slice(&self.payload);
        buf
    }

    /// Deserialize from the variable-length wire layout.
    pub fn decode(bytes: &[u8]) -> Result<Self, WireQueryError> {
        if bytes.len() < Self::HEADER_SIZE {
            return Err(WireQueryError::MalformedPayload);
        }
        Ok(Self {
            callsign_hash: read_u32(bytes, 0)?,
            seq: u16::from_be_bytes([bytes[4], bytes[5]]),
            ttl: bytes[6],
            flags: bytes[7],
            payload: bytes[8..].to_vec(),
        })
    }
}

/// Compute an FNV-1a 32-bit hash of a callsign string (uppercase, trimmed).
pub fn callsign_hash(callsign: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in callsign.trim().to_uppercase().bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
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
            signature: Some([0xcc; 64]),
        }
    }

    /// Build a signed relay envelope whose `src_peer_id` is the verifying key of `seed`.
    fn signed_envelope(seed: &[u8; 32], payload: Vec<u8>) -> WireEnvelope {
        let src = SigningKey::from_bytes(seed).verifying_key().to_bytes();
        let mut env = WireEnvelope {
            msg_type: WireMsgType::RelayDataChunk,
            flags: 0,
            session_id: 7,
            src_peer_id: src,
            dst_peer_id: [0xbb; 32],
            nonce: [0x22; 12],
            timestamp_ms: 1_700_000_000_000,
            hop_limit: 4,
            hop_index: 0,
            payload,
            signature: None,
        };
        env.sign(seed).unwrap();
        env
    }

    #[test]
    fn unsigned_envelope_omits_signature_bytes_on_wire() {
        let env = test_envelope(WireMsgType::PeerQueryRequest, vec![0xAB; 4]);
        let unsigned = WireEnvelope {
            signature: None,
            ..env
        };
        let bytes = unsigned.encode().unwrap();
        // 104 header + 4 payload + 0 signature.
        assert_eq!(bytes.len(), 104 + 4);
        let decoded = WireEnvelope::decode(&bytes).unwrap();
        assert_eq!(decoded.signature, None);
    }

    #[test]
    fn signed_envelope_verifies_against_src_peer_id() {
        let env = signed_envelope(&[7u8; 32], b"hello".to_vec());
        assert!(env.verify_origin().is_ok());
        // Survives an encode/decode round-trip.
        let decoded = WireEnvelope::decode(&env.encode().unwrap()).unwrap();
        assert!(decoded.verify_origin().is_ok());
    }

    #[test]
    fn signature_survives_hop_index_increment() {
        let mut env = signed_envelope(&[9u8; 32], b"relayed".to_vec());
        // A relay bumps hop_index; the origin signature must still verify.
        env.hop_index += 1;
        assert!(env.verify_origin().is_ok());
    }

    #[test]
    fn tampered_payload_fails_verification() {
        let mut env = signed_envelope(&[3u8; 32], b"payload".to_vec());
        env.payload[0] ^= 0xff;
        assert_eq!(env.verify_origin(), Err(WireQueryError::InvalidSignature));
    }

    #[test]
    fn spoofed_src_peer_id_fails_verification() {
        let mut env = signed_envelope(&[5u8; 32], b"payload".to_vec());
        // Claim to be a different (valid) key we do not hold.
        env.src_peer_id = SigningKey::from_bytes(&[6u8; 32])
            .verifying_key()
            .to_bytes();
        assert_eq!(env.verify_origin(), Err(WireQueryError::InvalidSignature));
    }

    #[test]
    fn unsigned_envelope_fails_verification() {
        let mut env = signed_envelope(&[1u8; 32], b"x".to_vec());
        env.signature = None;
        assert_eq!(env.verify_origin(), Err(WireQueryError::InvalidSignature));
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
        assert_eq!(decoded_env.signature, Some([0xcc; 64]));

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

    #[test]
    fn broadcast_frame_round_trip() {
        let frame = BroadcastFrame {
            callsign_hash: callsign_hash("KX0ABC"),
            seq: 42,
            ttl: 3,
            flags: 0,
            payload: b"hello mesh".to_vec(),
        };
        let enc = frame.encode();
        assert_eq!(enc.len(), BroadcastFrame::HEADER_SIZE + 10);
        let dec = BroadcastFrame::decode(&enc).unwrap();
        assert_eq!(dec, frame);
    }

    #[test]
    fn broadcast_frame_ttl_decrement_in_place() {
        let mut frame = BroadcastFrame {
            callsign_hash: callsign_hash("W1AW"),
            seq: 1,
            ttl: 2,
            flags: 0,
            payload: vec![0xDE, 0xAD],
        };
        frame.ttl -= 1;
        assert_eq!(frame.ttl, 1);
        let dec = BroadcastFrame::decode(&frame.encode()).unwrap();
        assert_eq!(dec.ttl, 1);
    }

    #[test]
    fn callsign_hash_case_insensitive() {
        assert_eq!(callsign_hash("kx0abc"), callsign_hash("KX0ABC"));
        assert_eq!(callsign_hash(" W1AW "), callsign_hash("W1AW"));
    }
}
