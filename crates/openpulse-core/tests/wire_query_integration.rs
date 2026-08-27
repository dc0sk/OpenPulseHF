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
        signature: Some([0xcc; 64]),
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
    assert_eq!(decoded.signature, Some([0xcc; 64]));

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
fn envelope_rejects_missing_signature() {
    let env = make_envelope(WireMsgType::PeerQueryRequest, vec![0xAB; 4]);
    let bytes = env.encode().unwrap();
    // Strip the last 16 bytes of the 64-byte signature
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
fn peer_query_response_rejects_oversized_result_count_without_over_allocating() {
    // Audit F-4: a payload claiming 65535 results but carrying none must fail fast (the loop bails on
    // the first short record) rather than pre-allocating a multi-MB Vec from the attacker-controlled
    // count. query_id (8 bytes) + result_count = 0xFFFF, then no result bytes.
    let mut payload = vec![0u8; 8];
    payload.extend_from_slice(&0xFFFFu16.to_be_bytes());
    let err = PeerQueryResponse::decode(&payload);
    assert!(
        matches!(err, Err(WireQueryError::MalformedPayload)),
        "an over-claimed result count must be rejected, got {err:?}"
    );
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

/// #1164: the version byte is AUTHORITATIVE — a foreign version is rejected BY VERSION, not by
/// whatever its trailer happens to look like.
///
/// Both directions are exercised because the interesting failure is asymmetric: a HIGHER version
/// (a future build) and a LOWER one (a v1-era build reconstructed during a bisect) must both be
/// refused by number. Before this change the low case died in the trailer as a "corrupt frame",
/// which is the unattributable symptom the check exists to remove.
#[test]
fn an_envelope_of_a_foreign_version_is_rejected_by_version() {
    let env = WireEnvelope {
        msg_type: WireMsgType::PeerQueryRequest,
        flags: 0,
        session_id: 1,
        src_peer_id: [1u8; 32],
        dst_peer_id: [2u8; 32],
        nonce: [3u8; 12],
        timestamp_ms: 1_700_000_000_000,
        hop_limit: 3,
        hop_index: 0,
        payload: vec![0u8; 17],
        signature: None,
    };
    let good = env.encode().expect("encode");

    // Positive control: the CURRENT version decodes, or every assertion below passes vacuously.
    assert!(
        WireEnvelope::decode(&good).is_ok(),
        "control: a current-version envelope must decode"
    );

    for foreign in [1u8, 3u8, 255u8] {
        let mut bad = good.clone();
        bad[4] = foreign;
        match WireEnvelope::decode(&bad) {
            Err(WireQueryError::UnsupportedVersion { got, .. }) => assert_eq!(got, foreign),
            other => panic!("version {foreign} must be refused by VERSION, got {other:?}"),
        }
    }
}

/// A v1-era frame — foreign version AND the 16-byte `auth_tag` trailer v1 actually carried — is
/// refused by version rather than by trailer length. This is the bisect case: an old build on one
/// rig talking to a new build on the other.
#[test]
fn a_v1_era_frame_is_refused_by_version_not_by_its_trailer() {
    let env = WireEnvelope {
        msg_type: WireMsgType::PeerQueryRequest,
        flags: 0,
        session_id: 1,
        src_peer_id: [1u8; 32],
        dst_peer_id: [2u8; 32],
        nonce: [3u8; 12],
        timestamp_ms: 1_700_000_000_000,
        hop_limit: 3,
        hop_index: 0,
        payload: vec![0u8; 17],
        signature: None,
    };
    let mut v1 = env.encode().expect("encode");
    v1[4] = 1;
    v1.extend_from_slice(&[0xAAu8; 16]); // the v1 auth_tag
    assert!(
        matches!(
            WireEnvelope::decode(&v1),
            Err(WireQueryError::UnsupportedVersion { got: 1, .. })
        ),
        "a v1 frame must be refused by version, not diagnosed as a corrupt trailer"
    );
}
