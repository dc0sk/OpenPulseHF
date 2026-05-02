use openpulse_core::{
    score_route, select_best_scored_route, AckStatus, PeerCache, PeerRecord, RelayDataChunk,
    RelayEvent, RelayForwardError, RelayForwarder, RelayHopAck, RelayTrustPolicy, TrustFilter,
    TrustLevel, WireEnvelope, WireMsgType,
};

// ------------------------------------------------------------------
// Helpers
// ------------------------------------------------------------------

fn make_peer(peer_id: &str, quality: u8, trust: TrustLevel) -> PeerRecord {
    PeerRecord {
        peer_id: peer_id.to_string(),
        capability_mask: 0,
        route_quality: quality,
        trust_level: trust,
        revision: 1,
        updated_at_ms: 1_000,
    }
}

fn route(peers: &[&str]) -> Vec<String> {
    peers.iter().map(|s| s.to_string()).collect()
}

fn relay_envelope(session_id: u64, nonce: [u8; 12], hop_limit: u8, hop_index: u8) -> WireEnvelope {
    WireEnvelope {
        msg_type: WireMsgType::RelayDataChunk,
        flags: 0,
        session_id,
        src_peer_id: [0xaa; 32],
        dst_peer_id: [0xbb; 32],
        nonce,
        timestamp_ms: 1_000,
        hop_limit,
        hop_index,
        payload: vec![0xAB; 8],
        auth_tag: [0; 16],
    }
}

// ------------------------------------------------------------------
// Path scoring tests
// ------------------------------------------------------------------

#[test]
fn score_route_prefers_high_quality_verified_hop() {
    let mut cache = PeerCache::new(16, 60_000);
    cache.upsert(make_peer("relay-good", 90, TrustLevel::Verified), 1_000);
    cache.upsert(make_peer("relay-poor", 20, TrustLevel::Reduced), 1_000);

    // route A: single hop via relay-good → score = 4 * 90 = 360
    let score_a = score_route(&route(&["src", "relay-good", "dst"]), &cache);
    // route B: single hop via relay-poor → score = 1 * 20 = 20
    let score_b = score_route(&route(&["src", "relay-poor", "dst"]), &cache);

    assert!(
        score_a > score_b,
        "high-quality verified relay should score higher"
    );
}

#[test]
fn score_route_uses_bottleneck_across_hops() {
    let mut cache = PeerCache::new(16, 60_000);
    cache.upsert(make_peer("relay-a", 90, TrustLevel::Verified), 1_000);
    cache.upsert(make_peer("relay-b", 10, TrustLevel::Unknown), 1_000);

    // bottleneck at relay-b: min(4*90, 2*10) = min(360, 20) = 20
    let score = score_route(&route(&["src", "relay-a", "relay-b", "dst"]), &cache);
    assert_eq!(score, 20);
}

#[test]
fn select_best_scored_route_chooses_highest_score() {
    let mut cache = PeerCache::new(16, 60_000);
    cache.upsert(make_peer("relay-verified", 80, TrustLevel::Verified), 1_000);
    cache.upsert(make_peer("relay-unknown", 80, TrustLevel::Unknown), 1_000);

    let policy = RelayTrustPolicy::default();
    let candidates = vec![
        route(&["src", "relay-unknown", "dst"]), // score = 2*80 = 160
        route(&["src", "relay-verified", "dst"]), // score = 4*80 = 320
    ];
    let best = select_best_scored_route(&candidates, 4, &policy, &cache).unwrap();
    assert!(best.contains(&"relay-verified".to_string()));
}

#[test]
fn select_best_scored_route_skips_policy_denied_candidates() {
    let mut cache = PeerCache::new(16, 60_000);
    cache.upsert(make_peer("relay-denied", 90, TrustLevel::Verified), 1_000);
    cache.upsert(make_peer("relay-ok", 50, TrustLevel::Verified), 1_000);

    let policy = RelayTrustPolicy::deny_relays(["relay-denied"]);
    let candidates = vec![
        route(&["src", "relay-denied", "dst"]),
        route(&["src", "relay-ok", "dst"]),
    ];
    let best = select_best_scored_route(&candidates, 4, &policy, &cache).unwrap();
    assert!(best.contains(&"relay-ok".to_string()));
}

// ------------------------------------------------------------------
// Three-hop relay forwarding tests
// ------------------------------------------------------------------

#[test]
fn three_hop_relay_path_increments_hop_index_at_each_node() {
    let policy = RelayTrustPolicy::default();
    let mut node_a = RelayForwarder::new(60_000, policy.clone());
    let mut node_b = RelayForwarder::new(60_000, policy.clone());
    let mut node_c = RelayForwarder::new(60_000, policy);

    let env = relay_envelope(1, [0x11; 12], 4, 0);

    // src → relay-a
    let after_a = node_a.forward(&env, 1_000).unwrap();
    assert_eq!(after_a.hop_index, 1);

    // relay-a → relay-b
    let after_b = node_b.forward(&after_a, 1_001).unwrap();
    assert_eq!(after_b.hop_index, 2);

    // relay-b → relay-c → dst
    let after_c = node_c.forward(&after_b, 1_002).unwrap();
    assert_eq!(after_c.hop_index, 3);
}

#[test]
fn hop_limit_exceeded_drops_at_correct_node() {
    let mut fwd = RelayForwarder::new(60_000, RelayTrustPolicy::default());

    // hop_index == hop_limit → must be dropped
    let env = relay_envelope(2, [0x22; 12], 3, 3);
    assert!(matches!(
        fwd.forward(&env, 1_000),
        Err(RelayForwardError::HopLimitExceeded {
            hop_index: 3,
            hop_limit: 3
        })
    ));
}

#[test]
fn duplicate_suppression_prevents_replay_across_hops() {
    let mut fwd = RelayForwarder::new(60_000, RelayTrustPolicy::default());
    let env = relay_envelope(3, [0x33; 12], 4, 0);

    fwd.forward(&env, 1_000).unwrap(); // first pass: ok
    assert!(matches!(
        fwd.forward(&env, 1_001), // second pass: rejected
        Err(RelayForwardError::DuplicateDetected)
    ));
}

#[test]
fn trust_policy_rejects_at_hop() {
    // Convert src_peer_id [0xaa; 32] to hex for deny list
    let src_hex: String = [0xaau8; 32].iter().fold(String::new(), |mut s, b| {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
        s
    });
    let policy = RelayTrustPolicy::deny_relays([src_hex]);
    let mut fwd = RelayForwarder::new(60_000, policy);
    let env = relay_envelope(4, [0x44; 12], 4, 0);
    assert!(matches!(
        fwd.forward(&env, 1_000),
        Err(RelayForwardError::PolicyRejected { .. })
    ));
}

#[test]
fn relay_events_emitted_for_forwarded_and_dropped() {
    let mut fwd = RelayForwarder::new(60_000, RelayTrustPolicy::default());

    let ok_env = relay_envelope(5, [0x55; 12], 4, 0);
    fwd.forward(&ok_env, 1_000).unwrap();

    let dup_env = relay_envelope(5, [0x55; 12], 4, 0); // same nonce
    let _ = fwd.forward(&dup_env, 1_001);

    let limit_env = relay_envelope(6, [0x66; 12], 2, 2); // at limit
    let _ = fwd.forward(&limit_env, 1_002);

    let events = fwd.drain_events();
    assert_eq!(events.len(), 3);
    assert!(matches!(
        events[0],
        RelayEvent::Forwarded { session_id: 5, .. }
    ));
    assert!(matches!(
        events[1],
        RelayEvent::DuplicateSuppressed { session_id: 5, .. }
    ));
    assert!(matches!(
        events[2],
        RelayEvent::HopLimitExceeded { session_id: 6, .. }
    ));
}

// ------------------------------------------------------------------
// Relay payload codec tests
// ------------------------------------------------------------------

#[test]
fn relay_data_chunk_round_trip() {
    let chunk = RelayDataChunk {
        transfer_id: 0xDEAD_BEEF,
        chunk_seq: 7,
        total_chunks: 20,
        chunk_hash: [0xAA; 32],
        e2e_manifest_hash: [0xBB; 32],
        chunk_signature: vec![0xCC; 64],
        chunk_data: b"hello relay".to_vec(),
    };
    let encoded = chunk.encode().unwrap();
    let decoded = RelayDataChunk::decode(&encoded).unwrap();
    assert_eq!(decoded.transfer_id, chunk.transfer_id);
    assert_eq!(decoded.chunk_seq, chunk.chunk_seq);
    assert_eq!(decoded.chunk_hash, chunk.chunk_hash);
    assert_eq!(decoded.chunk_signature, chunk.chunk_signature);
    assert_eq!(decoded.chunk_data, chunk.chunk_data);
}

#[test]
fn relay_hop_ack_round_trip() {
    let ack = RelayHopAck {
        transfer_id: 0x1234,
        chunk_seq: 3,
        hop_peer_id: [0xDD; 32],
        ack_status: AckStatus::Retry,
        retry_after_ms: 500,
        reason_code: 0x0009, // congestion_backoff
    };
    let encoded = ack.encode();
    assert_eq!(encoded.len(), RelayHopAck::SIZE);
    let decoded = RelayHopAck::decode(&encoded).unwrap();
    assert_eq!(decoded, ack);
}

// ------------------------------------------------------------------
// Query propagation still works with peer cache
// ------------------------------------------------------------------

#[test]
fn peer_cache_query_unaffected_by_relay_additions() {
    let mut cache = PeerCache::new(16, 60_000);
    cache.upsert(make_peer("relay-1", 80, TrustLevel::Verified), 1_000);
    cache.upsert(make_peer("relay-2", 60, TrustLevel::Unknown), 1_000);
    cache.upsert(make_peer("relay-3", 40, TrustLevel::Reduced), 1_000);

    let results = cache.query(0, 50, TrustFilter::TrustedOrUnknown, 10, 1_000);
    assert_eq!(results.len(), 2); // relay-1 (80, Verified) and relay-2 (60, Unknown)
    assert!(results[0].route_quality >= results[1].route_quality);
}
