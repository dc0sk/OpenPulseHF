use openpulse_core::{
    PeerQueryRequest, PeerQueryResponse, PeerQueryResult, WireEnvelope, WireMsgType, WireQueryError,
};

fn make_envelope(msg_type: WireMsgType, payload: Vec<u8>) -> WireEnvelope {
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

// ------------------------------------------------------------------
// Envelope tests
// ------------------------------------------------------------------

#[test]
fn envelope_query_request_round_trip() {
    let req = PeerQueryRequest {
        query_id: 0x22,
        capability_mask: 0x05,
        min_link_quality: 300,
        trust_filter: 0x01,
        max_results: 32,
    };
    let env = make_envelope(WireMsgType::PeerQueryRequest, req.encode());
    let bytes = env.encode().unwrap();
    let decoded = WireEnvelope::decode(&bytes).unwrap();

    assert_eq!(decoded.msg_type, WireMsgType::PeerQueryRequest);
    assert_eq!(decoded.flags, 0x0001);
    assert_eq!(decoded.session_id, 0x1001);
    assert_eq!(decoded.src_peer_id, [0xaa; 32]);
    assert_eq!(decoded.dst_peer_id, [0xbb; 32]);
    assert_eq!(decoded.nonce, [0x11; 12]);
    assert_eq!(decoded.hop_limit, 3);
    assert_eq!(decoded.hop_index, 0);
    assert_eq!(decoded.auth_tag, [0xcc; 16]);

    let decoded_req = PeerQueryRequest::decode(&decoded.payload).unwrap();
    assert_eq!(decoded_req.query_id, 0x22);
    assert_eq!(decoded_req.capability_mask, 0x05);
    assert_eq!(decoded_req.min_link_quality, 300);
    assert_eq!(decoded_req.trust_filter, 0x01);
    assert_eq!(decoded_req.max_results, 32);
}

#[test]
fn envelope_query_response_round_trip() {
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
        results: vec![result.clone()],
    };
    let env = make_envelope(WireMsgType::PeerQueryResponse, resp.encode().unwrap());
    let bytes = env.encode().unwrap();

    let decoded_env = WireEnvelope::decode(&bytes).unwrap();
    assert_eq!(decoded_env.msg_type, WireMsgType::PeerQueryResponse);

    let decoded_resp = PeerQueryResponse::decode(&decoded_env.payload).unwrap();
    assert_eq!(decoded_resp.query_id, 0x42);
    assert_eq!(decoded_resp.results.len(), 1);
    assert_eq!(decoded_resp.results[0], result);
}

#[test]
fn envelope_rejects_invalid_magic() {
    let env = make_envelope(WireMsgType::PeerQueryRequest, vec![]);
    let mut bytes = env.encode().unwrap();
    bytes[0] = 0xFF;
    assert!(matches!(
        WireEnvelope::decode(&bytes),
        Err(WireQueryError::InvalidMagic)
    ));
}

#[test]
fn envelope_rejects_unknown_msg_type() {
    let env = make_envelope(WireMsgType::PeerQueryRequest, vec![]);
    let mut bytes = env.encode().unwrap();
    bytes[5] = 0x99;
    assert!(matches!(
        WireEnvelope::decode(&bytes),
        Err(WireQueryError::UnknownMsgType(0x99))
    ));
}

#[test]
fn envelope_rejects_truncated_header() {
    let env = make_envelope(WireMsgType::PeerQueryRequest, vec![0xAB; 17]);
    let bytes = env.encode().unwrap();
    assert!(matches!(
        WireEnvelope::decode(&bytes[..40]),
        Err(WireQueryError::BufferTooShort)
    ));
}

#[test]
fn envelope_rejects_missing_auth_tag() {
    let env = make_envelope(WireMsgType::PeerQueryRequest, vec![0xAB; 4]);
    let bytes = env.encode().unwrap();
    // Strip the last 16 bytes (auth_tag)
    assert!(matches!(
        WireEnvelope::decode(&bytes[..bytes.len() - 16]),
        Err(WireQueryError::BufferTooShort)
    ));
}

// ------------------------------------------------------------------
// Payload size spec tests
// ------------------------------------------------------------------

#[test]
fn peer_query_request_encoded_size_is_17() {
    let req = PeerQueryRequest {
        query_id: 0,
        capability_mask: 0,
        min_link_quality: 0,
        trust_filter: 0,
        max_results: 0,
    };
    assert_eq!(req.encode().len(), PeerQueryRequest::SIZE);
    assert_eq!(PeerQueryRequest::SIZE, 17);
}

#[test]
fn response_with_multiple_results_round_trips() {
    let make_result = |b: u8| PeerQueryResult {
        peer_id: [b; 32],
        callsign_hash: [b + 1; 32],
        capability_mask: b as u32,
        last_seen_ms: b as u64 * 1_000,
        trust_state: b % 4,
        descriptor_signature: vec![b; 64],
    };

    let resp = PeerQueryResponse {
        query_id: 0xDEAD,
        results: (1u8..=3).map(make_result).collect(),
    };
    let payload = resp.encode().unwrap();
    let decoded = PeerQueryResponse::decode(&payload).unwrap();
    assert_eq!(decoded.query_id, 0xDEAD);
    assert_eq!(decoded.results.len(), 3);
    for (i, r) in decoded.results.iter().enumerate() {
        let b = (i + 1) as u8;
        assert_eq!(r.peer_id, [b; 32]);
        assert_eq!(r.capability_mask, b as u32);
    }
}

#[test]
fn hop_limit_and_index_preserved() {
    let mut env = make_envelope(WireMsgType::PeerQueryRequest, vec![]);
    env.hop_limit = 5;
    env.hop_index = 2;
    let bytes = env.encode().unwrap();
    let decoded = WireEnvelope::decode(&bytes).unwrap();
    assert_eq!(decoded.hop_limit, 5);
    assert_eq!(decoded.hop_index, 2);
}
