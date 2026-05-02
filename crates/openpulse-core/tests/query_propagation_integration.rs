use openpulse_core::{
    PeerQueryRequest, QueryEvent, QueryForwardError, QueryForwarder, RelayRouteReject,
    RelayRouteUpdate, RelayTrustPolicy, RouteChangeReason, RouteDiscoveryRequest,
    RouteDiscoveryResponse, RouteHop, WireEnvelope, WireMsgType, WireTrustState,
};

// ------------------------------------------------------------------
// Helpers
// ------------------------------------------------------------------

fn query_envelope(
    query_id: u64,
    src_peer_id: [u8; 32],
    hop_limit: u8,
    hop_index: u8,
) -> WireEnvelope {
    let payload = PeerQueryRequest {
        query_id,
        capability_mask: 0,
        min_link_quality: 0,
        trust_filter: 0x02,
        max_results: 10,
    }
    .encode();
    WireEnvelope {
        msg_type: WireMsgType::PeerQueryRequest,
        flags: 0,
        session_id: query_id,
        src_peer_id,
        dst_peer_id: [0xff; 32],
        nonce: [0x11; 12],
        timestamp_ms: 1_000,
        hop_limit,
        hop_index,
        payload,
        auth_tag: [0; 16],
    }
}

fn hop(id: u8) -> RouteHop {
    RouteHop {
        hop_peer_id: [id; 32],
        hop_trust_state: WireTrustState::Trusted as u8,
        estimated_latency_ms: 50,
        estimated_reliability_permille: 900,
    }
}

// ------------------------------------------------------------------
// Wire codec round-trip tests
// ------------------------------------------------------------------

#[test]
fn route_discovery_request_round_trip() {
    let req = RouteDiscoveryRequest {
        route_query_id: 0xDEAD_BEEF_0000_0001,
        destination_peer_id: [0x99; 32],
        max_hops: 5,
        required_capability_mask: 0x0000_0009,
        policy_flags: 0x0003,
    };
    assert_eq!(req.encode().len(), RouteDiscoveryRequest::SIZE);
    let decoded = RouteDiscoveryRequest::decode(&req.encode()).unwrap();
    assert_eq!(decoded.route_query_id, req.route_query_id);
    assert_eq!(decoded.destination_peer_id, req.destination_peer_id);
    assert_eq!(decoded.max_hops, req.max_hops);
    assert_eq!(
        decoded.required_capability_mask,
        req.required_capability_mask
    );
    assert_eq!(decoded.policy_flags, req.policy_flags);
}

#[test]
fn route_discovery_response_no_hops_round_trip() {
    let resp = RouteDiscoveryResponse {
        route_query_id: 0x1234,
        route_id: 0xABCD,
        hops: vec![],
        route_signature: vec![],
    };
    let encoded = resp.encode().unwrap();
    let decoded = RouteDiscoveryResponse::decode(&encoded).unwrap();
    assert_eq!(decoded.route_query_id, resp.route_query_id);
    assert_eq!(decoded.route_id, resp.route_id);
    assert!(decoded.hops.is_empty());
    assert!(decoded.route_signature.is_empty());
}

#[test]
fn route_discovery_response_multi_hop_round_trip() {
    let resp = RouteDiscoveryResponse {
        route_query_id: 0x5678,
        route_id: 0xEF01,
        hops: vec![hop(0xAA), hop(0xBB), hop(0xCC)],
        route_signature: vec![0x55; 64],
    };
    let encoded = resp.encode().unwrap();
    let decoded = RouteDiscoveryResponse::decode(&encoded).unwrap();
    assert_eq!(decoded.hops.len(), 3);
    assert_eq!(decoded.hops[1].hop_peer_id, [0xBB; 32]);
    assert_eq!(decoded.route_signature, vec![0x55; 64]);
}

#[test]
fn relay_route_update_round_trip() {
    let update = RelayRouteUpdate {
        route_id: 0x0000_1111_2222_3333,
        previous_hop_count: 2,
        route_change_reason: RouteChangeReason::HopUnreachable as u16,
        replacement_hops: vec![hop(0x10), hop(0x20)],
        route_update_signature: vec![0xAA; 32],
    };
    let encoded = update.encode().unwrap();
    let decoded = RelayRouteUpdate::decode(&encoded).unwrap();
    assert_eq!(decoded.route_id, update.route_id);
    assert_eq!(decoded.previous_hop_count, 2);
    assert_eq!(
        decoded.route_change_reason,
        RouteChangeReason::HopUnreachable as u16
    );
    assert_eq!(decoded.replacement_hops.len(), 2);
    assert_eq!(decoded.route_update_signature, vec![0xAA; 32]);
}

#[test]
fn relay_route_reject_round_trip() {
    let reject = RelayRouteReject {
        route_id: 0xFFFF_FFFF_FFFF_FFFF,
        reject_hop_peer_id: [0xDD; 32],
        reason_code: 0x0007, // trust_policy_reject
        trust_decision: WireTrustState::Revoked as u8,
        policy_reference: 0x0042,
    };
    assert_eq!(reject.encode().len(), RelayRouteReject::SIZE);
    let decoded = RelayRouteReject::decode(&reject.encode()).unwrap();
    assert_eq!(decoded.route_id, reject.route_id);
    assert_eq!(decoded.reject_hop_peer_id, [0xDD; 32]);
    assert_eq!(decoded.reason_code, 0x0007);
    assert_eq!(decoded.trust_decision, WireTrustState::Revoked as u8);
    assert_eq!(decoded.policy_reference, 0x0042);
}

// ------------------------------------------------------------------
// QueryForwarder behaviour tests
// ------------------------------------------------------------------

#[test]
fn query_propagates_with_incremented_hop_index() {
    let mut fwd = QueryForwarder::new(60_000, 64, RelayTrustPolicy::default());
    let env = query_envelope(1, [0xAA; 32], 4, 0);
    let out = fwd.propagate(&env, 1_000).unwrap();
    assert_eq!(out.hop_index, 1);
}

#[test]
fn query_not_forwarded_at_hop_limit() {
    let mut fwd = QueryForwarder::new(60_000, 64, RelayTrustPolicy::default());
    let env = query_envelope(2, [0xAA; 32], 3, 3);
    assert!(matches!(
        fwd.propagate(&env, 1_000),
        Err(QueryForwardError::HopLimitExceeded {
            hop_index: 3,
            hop_limit: 3
        })
    ));
}

#[test]
fn duplicate_query_suppressed_within_ttl() {
    let mut fwd = QueryForwarder::new(60_000, 64, RelayTrustPolicy::default());
    let env = query_envelope(3, [0xAA; 32], 4, 0);
    fwd.propagate(&env, 1_000).unwrap();
    assert!(matches!(
        fwd.propagate(&env, 1_001),
        Err(QueryForwardError::DuplicateDetected)
    ));
}

#[test]
fn trust_policy_rejects_query_propagation() {
    let src_hex: String = [0xABu8; 32].iter().fold(String::new(), |mut s, b| {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
        s
    });
    let policy = RelayTrustPolicy::deny_relays([src_hex]);
    let mut fwd = QueryForwarder::new(60_000, 64, policy);
    let env = query_envelope(4, [0xAB; 32], 4, 0);
    assert!(matches!(
        fwd.propagate(&env, 1_000),
        Err(QueryForwardError::PolicyRejected { .. })
    ));
}

#[test]
fn query_events_emitted_for_each_outcome() {
    let src_hex: String = [0xCDu8; 32].iter().fold(String::new(), |mut s, b| {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
        s
    });
    let policy = RelayTrustPolicy::deny_relays([src_hex]);
    let mut fwd = QueryForwarder::new(60_000, 64, policy);

    // Success
    let ok_env = query_envelope(10, [0xAA; 32], 4, 0);
    fwd.propagate(&ok_env, 1_000).unwrap();

    // Duplicate
    let dup_env = query_envelope(10, [0xAA; 32], 4, 0);
    let _ = fwd.propagate(&dup_env, 1_001);

    // Hop limit
    let limit_env = query_envelope(11, [0xAA; 32], 2, 2);
    let _ = fwd.propagate(&limit_env, 1_002);

    // Policy rejected
    let policy_env = query_envelope(12, [0xCD; 32], 4, 0);
    let _ = fwd.propagate(&policy_env, 1_003);

    let events = fwd.drain_events();
    assert_eq!(events.len(), 4);
    assert!(matches!(
        events[0],
        QueryEvent::Propagated { query_id: 10, .. }
    ));
    assert!(matches!(
        events[1],
        QueryEvent::DuplicateSuppressed { query_id: 10 }
    ));
    assert!(matches!(
        events[2],
        QueryEvent::HopLimitReached { query_id: 11, .. }
    ));
    assert!(matches!(
        events[3],
        QueryEvent::PolicyRejected { query_id: 12, .. }
    ));
}

// ------------------------------------------------------------------
// Multi-node chain test
// ------------------------------------------------------------------

#[test]
fn three_node_query_chain_hop_index_increments_at_each_node() {
    let policy = RelayTrustPolicy::default();
    let mut node_a = QueryForwarder::new(60_000, 64, policy.clone());
    let mut node_b = QueryForwarder::new(60_000, 64, policy.clone());
    let mut node_c = QueryForwarder::new(60_000, 64, policy);

    let env = query_envelope(99, [0xAA; 32], 4, 0);

    let after_a = node_a.propagate(&env, 1_000).unwrap();
    assert_eq!(after_a.hop_index, 1);

    let after_b = node_b.propagate(&after_a, 1_001).unwrap();
    assert_eq!(after_b.hop_index, 2);

    let after_c = node_c.propagate(&after_b, 1_002).unwrap();
    assert_eq!(after_c.hop_index, 3);
}

// ------------------------------------------------------------------
// Tracker smoke test through QueryForwarder
// ------------------------------------------------------------------

#[test]
fn tracker_still_suppresses_duplicate_queries() {
    let mut fwd = QueryForwarder::new(2_000, 64, RelayTrustPolicy::default());
    let env = query_envelope(200, [0xBB; 32], 4, 0);
    assert!(fwd.propagate(&env, 1_000).is_ok());
    assert!(matches!(
        fwd.propagate(&env, 1_500),
        Err(QueryForwardError::DuplicateDetected)
    ));
}
