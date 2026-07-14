//! 3-node mesh relay loopback: A → B → C
//!
//! Verifies that a `RelayDataChunk` frame addressed to node C arrives via relay
//! through node B from node A, using clean loopback channels (no channel distortion).

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::peer_cache::TrustFilter;
use openpulse_core::relay::{RelayEvent, RelayTrustPolicy};
use openpulse_core::route_discovery::{RouteOriginator, RouteResponder};
use openpulse_core::wire_query::{WireEnvelope, WireMsgType};
use openpulse_mesh::{MeshDaemon, MeshEvent};
use openpulse_modem::ModemEngine;
const MODE: &str = "BPSK250";

fn make_node(lb: &LoopbackBackend, peer_id: [u8; 32]) -> MeshDaemon {
    let mut engine = ModemEngine::new(Box::new(lb.clone_shared()));
    let _ = engine.register_plugin(Box::new(BpskPlugin::default()));
    let policy = RelayTrustPolicy::deny_relays([] as [&str; 0]);
    MeshDaemon::new(
        engine, MODE, peer_id, 3, 0, 300_000, policy, 64, 3_600_000, [0u8; 32], "N0CALL",
    )
}

/// A node with beacons enabled (beacon_interval_s=1).
fn make_beacon_node(lb: &LoopbackBackend, peer_id: [u8; 32]) -> MeshDaemon {
    let mut engine = ModemEngine::new(Box::new(lb.clone_shared()));
    let _ = engine.register_plugin(Box::new(BpskPlugin::default()));
    let policy = RelayTrustPolicy::deny_relays([] as [&str; 0]);
    MeshDaemon::new(
        engine, MODE, peer_id, 3, 1, 300_000, policy, 64, 3_600_000, [0u8; 32], "N0CALL",
    )
}

fn relay_envelope(src: [u8; 32], dst: [u8; 32], session_id: u64, nonce: u8) -> WireEnvelope {
    WireEnvelope {
        msg_type: WireMsgType::RelayDataChunk,
        flags: 0,
        session_id,
        src_peer_id: src,
        dst_peer_id: dst,
        nonce: {
            let mut n = [0u8; 12];
            n[0] = nonce;
            n
        },
        timestamp_ms: 1000,
        hop_limit: 2,
        hop_index: 0,
        payload: b"hello C".to_vec(),
        auth_tag: [0u8; 16],
    }
}

/// A → (clean channel) → B → (clean channel) → C
#[test]
fn three_node_relay_clean() {
    let peer_a = [1u8; 32];
    let peer_b = [2u8; 32];
    let peer_c = [3u8; 32];

    let lb_a = LoopbackBackend::new();
    let lb_b = LoopbackBackend::new();
    let lb_c = LoopbackBackend::new();

    let mut node_a = make_node(&lb_a, peer_a);
    let mut node_b = make_node(&lb_b, peer_b);
    let mut node_c = make_node(&lb_c, peer_c);

    let env = relay_envelope(peer_a, peer_c, 42, 1);
    node_a.send_relay(env).unwrap();

    let samples_a = lb_a.drain_samples();
    assert!(!samples_a.is_empty(), "A must have produced TX samples");
    lb_b.fill_samples(&samples_a);

    let events_b = node_b.step(1000);
    assert!(
        events_b
            .iter()
            .any(|e| matches!(e, MeshEvent::Relay(RelayEvent::Forwarded { .. }))),
        "B must emit a Forwarded relay event; got {events_b:?}"
    );

    let samples_b = lb_b.drain_samples();
    assert!(
        !samples_b.is_empty(),
        "B must have produced TX samples after forwarding"
    );
    lb_c.fill_samples(&samples_b);

    let events_c = node_c.step(1000);
    assert!(
        events_c
            .iter()
            .any(|e| matches!(e, MeshEvent::FrameDelivered { session_id: 42 })),
        "C must receive and deliver the frame (session_id=42); got {events_c:?}"
    );
}

/// Hop limit enforcement: a frame already at hop_index == hop_limit is dropped at B.
#[test]
fn hop_limit_drop_at_relay() {
    let peer_a = [10u8; 32];
    let peer_b = [11u8; 32];
    let peer_c = [12u8; 32];

    let lb_a = LoopbackBackend::new();
    let lb_b = LoopbackBackend::new();

    let mut node_a = make_node(&lb_a, peer_a);
    let mut node_b = make_node(&lb_b, peer_b);

    let mut env = relay_envelope(peer_a, peer_c, 99, 2);
    env.hop_index = 2;
    env.hop_limit = 2;
    node_a.send_relay(env).unwrap();

    let samples_a = lb_a.drain_samples();
    lb_b.fill_samples(&samples_a);

    let events_b = node_b.step(2000);
    assert!(
        events_b
            .iter()
            .any(|e| matches!(e, MeshEvent::Relay(RelayEvent::HopLimitExceeded { .. }))),
        "B must emit HopLimitExceeded; got {events_b:?}"
    );
    assert!(
        lb_b.drain_samples().is_empty(),
        "B must not forward a hop-limit-exceeded frame"
    );
}

/// Duplicate suppression: the same (session_id, nonce) pair is dropped on the second pass.
#[test]
fn duplicate_suppression_at_relay() {
    let peer_a = [20u8; 32];
    let peer_b = [21u8; 32];
    let peer_c = [22u8; 32];

    let lb_a = LoopbackBackend::new();
    let lb_b = LoopbackBackend::new();

    let mut node_a = make_node(&lb_a, peer_a);
    let mut node_b = make_node(&lb_b, peer_b);

    let env = relay_envelope(peer_a, peer_c, 77, 3);

    node_a.send_relay(env.clone()).unwrap();
    let s = lb_a.drain_samples();
    lb_b.fill_samples(&s);
    let events1 = node_b.step(3000);
    assert!(events1
        .iter()
        .any(|e| matches!(e, MeshEvent::Relay(RelayEvent::Forwarded { .. }))));
    lb_b.drain_samples();

    node_a.send_relay(env).unwrap();
    let s = lb_a.drain_samples();
    lb_b.fill_samples(&s);
    let events2 = node_b.step(3001);
    assert!(
        events2
            .iter()
            .any(|e| matches!(e, MeshEvent::Relay(RelayEvent::DuplicateSuppressed { .. }))),
        "B must suppress the duplicate; got {events2:?}"
    );
    assert!(
        lb_b.drain_samples().is_empty(),
        "B must not forward a duplicate frame"
    );
}

/// Peer discovery: A sends a beacon, B receives it, responds with its local cache
/// (which includes B itself), A caches B and emits PeerDiscovered.
#[test]
fn peer_discovery_via_beacon() {
    let peer_a = [30u8; 32];
    let peer_b = [31u8; 32];

    let lb_a = LoopbackBackend::new();
    let lb_b = LoopbackBackend::new();

    // A has beacon_interval_s=1; B has beacons disabled (just responds to queries).
    let mut node_a = make_beacon_node(&lb_a, peer_a);
    let mut node_b = make_node(&lb_b, peer_b);

    // Both start with only themselves in their caches.
    assert_eq!(node_a.peer_cache_len(), 1, "A starts with only self");
    assert_eq!(node_b.peer_cache_len(), 1, "B starts with only self");

    // Tick A at t=1000 ms to fire the beacon (interval=1 s).
    let events_a = node_a.step(1000);
    assert!(
        events_a
            .iter()
            .any(|e| matches!(e, MeshEvent::BeaconSent { .. })),
        "A must send a beacon; got {events_a:?}"
    );

    // Route A's beacon TX → B's RX buffer.
    let samples_a = lb_a.drain_samples();
    assert!(!samples_a.is_empty(), "A must have produced beacon samples");
    lb_b.fill_samples(&samples_a);

    // B processes the beacon: receives PeerQueryRequest, responds with its cache.
    let events_b = node_b.step(1000);
    assert!(
        events_b.iter().any(|e| matches!(
            e,
            MeshEvent::PeerQueried {
                result_count: 1,
                ..
            }
        )),
        "B must answer the query with 1 result (self); got {events_b:?}"
    );

    // Route B's response TX → A's RX buffer.
    let samples_b = lb_b.drain_samples();
    assert!(
        !samples_b.is_empty(),
        "B must have produced response samples"
    );
    lb_a.fill_samples(&samples_b);

    // A processes B's response: caches B.
    let events_a2 = node_a.step(1001);
    assert!(
        events_a2
            .iter()
            .any(|e| matches!(e, MeshEvent::PeerDiscovered { peer_id } if *peer_id == peer_b)),
        "A must emit PeerDiscovered for B; got {events_a2:?}"
    );
    assert_eq!(
        node_a.peer_cache_len(),
        2,
        "A must now have self + B in cache"
    );
}

/// Trust-level policy: a relay node with TrustedOnly policy rejects frames from
/// Unknown peers (not in its cache) and emits PolicyRejected without forwarding.
#[test]
fn policy_rejects_unknown_peer() {
    let peer_a = [40u8; 32];
    let peer_b = [41u8; 32];
    let peer_c = [42u8; 32];

    let lb_a = LoopbackBackend::new();
    let lb_b = LoopbackBackend::new();

    let mut node_a = make_node(&lb_a, peer_a);

    // Node B uses TrustedOnly policy; peer_a is not in its cache.
    let mut engine_b = ModemEngine::new(Box::new(lb_b.clone_shared()));
    let _ = engine_b.register_plugin(Box::new(BpskPlugin::default()));
    let strict_policy =
        RelayTrustPolicy::with_trust_filter([] as [&str; 0], TrustFilter::TrustedOnly);
    let mut node_b = MeshDaemon::new(
        engine_b,
        MODE,
        peer_b,
        3,
        0,
        300_000,
        strict_policy,
        64,
        3_600_000,
        [0u8; 32],
        "N0CALL",
    );

    let env = relay_envelope(peer_a, peer_c, 55, 4);
    node_a.send_relay(env).unwrap();

    let samples_a = lb_a.drain_samples();
    lb_b.fill_samples(&samples_a);

    let events_b = node_b.step(4000);
    assert!(
        events_b
            .iter()
            .any(|e| matches!(e, MeshEvent::Relay(RelayEvent::PolicyRejected { .. }))),
        "B must emit PolicyRejected for unknown peer; got {events_b:?}"
    );
    assert!(
        lb_b.drain_samples().is_empty(),
        "B must not forward a policy-rejected frame"
    );
}

// ── Broadcast tests ───────────────────────────────────────────────────────────

fn make_broadcast_engine(lb: &LoopbackBackend) -> ModemEngine {
    let mut engine = ModemEngine::new(Box::new(lb.clone_shared()));
    let _ = engine.register_plugin(Box::new(BpskPlugin::default()));
    engine
}

/// Node A broadcasts; Node B receives and emits BroadcastReceived.
#[test]
fn broadcast_received_by_neighbour() {
    let lb_a = LoopbackBackend::new();
    let lb_b = LoopbackBackend::new();

    let peer_b = [50u8; 32];
    let mut engine_a = make_broadcast_engine(&lb_a);
    engine_a.set_callsign("KX0ABC");

    let mut node_b = make_node(&lb_b, peer_b);

    engine_a
        .broadcast(b"hello world", MODE, 3, None)
        .expect("broadcast should succeed");

    let samples_a = lb_a.drain_samples();
    assert!(!samples_a.is_empty(), "A must produce TX samples");
    lb_b.fill_samples(&samples_a);

    let events_b = node_b.step(1000);
    assert!(
        events_b.iter().any(|e| matches!(
            e,
            MeshEvent::BroadcastReceived { payload, .. }
            if payload == b"hello world"
        )),
        "B must emit BroadcastReceived with correct payload; got {events_b:?}"
    );
}

/// Node A broadcasts with TTL=2; relay node B re-broadcasts; node C receives with TTL=1.
#[test]
fn broadcast_relayed_with_ttl_decrement() {
    let lb_a = LoopbackBackend::new();
    let lb_b = LoopbackBackend::new();
    let lb_c = LoopbackBackend::new();

    let peer_b = [60u8; 32];
    let peer_c = [61u8; 32];

    let mut engine_a = make_broadcast_engine(&lb_a);
    engine_a.set_callsign("W1AW");

    let mut node_b = make_node(&lb_b, peer_b);
    let mut node_c = make_node(&lb_c, peer_c);

    engine_a
        .broadcast(b"relay me", MODE, 2, None)
        .expect("broadcast from A");

    // A → B
    let samples_a = lb_a.drain_samples();
    lb_b.fill_samples(&samples_a);

    let events_b = node_b.step(1000);
    // B must receive and emit the event.
    assert!(
        events_b
            .iter()
            .any(|e| matches!(e, MeshEvent::BroadcastReceived { ttl: 2, .. })),
        "B must emit BroadcastReceived with ttl=2; got {events_b:?}"
    );
    // B must re-broadcast with ttl-1.
    let samples_b = lb_b.drain_samples();
    assert!(!samples_b.is_empty(), "B must produce relay TX samples");
    lb_c.fill_samples(&samples_b);

    let events_c = node_c.step(1000);
    assert!(
        events_c.iter().any(|e| matches!(
            e,
            MeshEvent::BroadcastReceived { ttl: 1, payload, .. }
            if payload == b"relay me"
        )),
        "C must receive the relay with ttl=1; got {events_c:?}"
    );
}

/// A broadcast with TTL=0 is delivered locally but not re-broadcast.
#[test]
fn broadcast_ttl_zero_not_relayed() {
    let lb_a = LoopbackBackend::new();
    let lb_b = LoopbackBackend::new();
    let lb_c = LoopbackBackend::new();

    let peer_b = [70u8; 32];
    let peer_c = [71u8; 32];

    let mut engine_a = make_broadcast_engine(&lb_a);
    let mut node_b = make_node(&lb_b, peer_b);
    let node_c = make_node(&lb_c, peer_c);

    engine_a
        .broadcast(b"no relay", MODE, 0, None)
        .expect("broadcast TTL=0 from A");

    let samples_a = lb_a.drain_samples();
    lb_b.fill_samples(&samples_a);

    let events_b = node_b.step(1000);
    assert!(
        events_b
            .iter()
            .any(|e| matches!(e, MeshEvent::BroadcastReceived { ttl: 0, .. })),
        "B must receive TTL=0 broadcast; got {events_b:?}"
    );

    // B must NOT re-broadcast.
    let relay_samples = lb_b.drain_samples();
    assert!(
        relay_samples.is_empty(),
        "B must not relay a TTL=0 broadcast"
    );
    drop((node_c, lb_c));
}

/// Route discovery: A floods a `RouteDiscoveryRequest` for B's route-identity; B recognises itself as
/// the destination and answers with a signed `RouteDiscoveryResponse` (RouteAnswered + reply samples).
#[test]
fn route_discovery_destination_answers() {
    // Every `make_node` uses signing seed [0;32], so a node's route-identity (verifying_key(seed))
    // is this — that is what a route request must target to reach it.
    let dst_id = RouteResponder::new(&[0u8; 32], 0).peer_id();

    let lb_a = LoopbackBackend::new();
    let lb_b = LoopbackBackend::new();
    let mut node_a = make_node(&lb_a, [1u8; 32]);
    let mut node_b = make_node(&lb_b, [2u8; 32]);

    // A originates a route request for B (no required capabilities) and transmits it.
    let mut originator = RouteOriginator::new([1u8; 32], 60_000);
    let (qid, req_env) = originator
        .originate(dst_id, 4, 0, 0, [7u8; 12], 1_000)
        .expect("originate");
    node_a.send_relay(req_env).expect("transmit route request");

    // A → B.
    let samples_a = lb_a.drain_samples();
    assert!(!samples_a.is_empty(), "A must produce request samples");
    lb_b.fill_samples(&samples_a);

    // B recognises itself as the destination and answers.
    let events_b = node_b.step(1_010);
    assert!(
        events_b.iter().any(
            |e| matches!(e, MeshEvent::RouteAnswered { route_query_id } if *route_query_id == qid)
        ),
        "B must answer the route request; got {events_b:?}"
    );
    assert!(
        !lb_b.drain_samples().is_empty(),
        "B must transmit a route response"
    );
}

/// Full originator loop: A originates a route query for B, B answers as the destination, A records the
/// route, then A sends relay data along the discovered route and B delivers it. Exercises the daemon's
/// `discover_route` / route-response application / `send_via_route` (route-table + scored-route
/// consumption) end to end.
#[test]
fn originator_discovers_then_sends_along_the_route() {
    // Every `make_node` signs with seed [0;32], so the route-identity is verifying_key([0;32]).
    // Give B that same value as its local_peer_id (as the real binary does) so it is both the route
    // target and the relay-delivery target.
    let dst_id = RouteResponder::new(&[0u8; 32], 0).peer_id();

    let lb_a = LoopbackBackend::new();
    let lb_b = LoopbackBackend::new();
    let mut node_a = make_node(&lb_a, [1u8; 32]);
    let mut node_b = make_node(&lb_b, dst_id);

    // No route yet → a send refuses.
    assert!(matches!(
        node_a.send_via_route(dst_id, 42, b"payload".to_vec(), 1_000),
        Err(openpulse_mesh::MeshError::NoRoute)
    ));

    // A originates a route query for B and transmits it.
    let qid = node_a
        .discover_route(dst_id, 1_000)
        .expect("discover_route");

    // A → B: B recognises itself as the destination and answers.
    lb_b.fill_samples(&lb_a.drain_samples());
    let events_b = node_b.step(1_010);
    assert!(
        events_b.iter().any(
            |e| matches!(e, MeshEvent::RouteAnswered { route_query_id } if *route_query_id == qid)
        ),
        "B must answer the route request; got {events_b:?}"
    );

    // B → A: A verifies the response and records the route.
    lb_a.fill_samples(&lb_b.drain_samples());
    let events_a = node_a.step(1_020);
    assert!(
        events_a
            .iter()
            .any(|e| matches!(e, MeshEvent::RouteDiscovered { destination, .. } if *destination == dst_id)),
        "A must record the discovered route; got {events_a:?}"
    );

    // A now sends relay data along the discovered route (consuming the route table).
    let route = node_a
        .send_via_route(dst_id, 42, b"payload".to_vec(), 1_030)
        .expect("send_via_route after discovery");
    assert_eq!(route.len(), 2, "direct destination route is [self, dst]");

    // A → B: B is the relay-data destination and delivers it.
    lb_b.fill_samples(&lb_a.drain_samples());
    let events_b = node_b.step(1_040);
    assert!(
        events_b
            .iter()
            .any(|e| matches!(e, MeshEvent::FrameDelivered { session_id: 42 })),
        "B must deliver the relay data sent along the route; got {events_b:?}"
    );
}
