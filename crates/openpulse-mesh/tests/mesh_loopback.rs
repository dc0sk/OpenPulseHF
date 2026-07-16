//! 3-node mesh relay loopback: A → B → C
//!
//! Verifies that a `RelayDataChunk` frame addressed to node C arrives via relay
//! through node B from node A, using clean loopback channels (no channel distortion).

use bpsk_plugin::BpskPlugin;
use ed25519_dalek::SigningKey;
use openpulse_audio::LoopbackBackend;
use openpulse_core::peer_cache::TrustFilter;
use openpulse_core::relay::{RelayEvent, RelayTrustPolicy};
use openpulse_core::route_discovery::{RouteOriginator, RouteResponder};
use openpulse_core::wire_query::{
    RelayRouteReject, RouteDiscoveryRequest, RouteHop, WireEnvelope, WireMsgType, WireTrustState,
};
use openpulse_mesh::{MeshDaemon, MeshEvent};
use openpulse_modem::ModemEngine;
const MODE: &str = "BPSK250";

fn make_node(lb: &LoopbackBackend, peer_id: [u8; 32]) -> MeshDaemon {
    let mut engine = ModemEngine::new(Box::new(lb.clone_shared()));
    let _ = engine.register_plugin(Box::new(BpskPlugin::default()));
    let mut policy = RelayTrustPolicy::deny_relays([] as [&str; 0]);
    // These routing-mechanics tests use synthetic peer ids that are not real Ed25519 verifying
    // keys, so origin-signature verification is disabled here; the authenticated relay path is
    // covered by `authenticated_relay_forwarding` with real keypairs.
    policy.set_require_authentication(false);
    MeshDaemon::new(
        engine, MODE, peer_id, 3, 0, 300_000, policy, 64, 3_600_000, [0u8; 32], "N0CALL",
    )
}

/// A node with a caller-chosen signing seed, so distinct nodes have distinct route identities
/// (`make_node` signs every node with `[0;32]`, which collides for multi-hop route tests).
fn make_node_seeded(lb: &LoopbackBackend, peer_id: [u8; 32], seed: [u8; 32]) -> MeshDaemon {
    let mut engine = ModemEngine::new(Box::new(lb.clone_shared()));
    let _ = engine.register_plugin(Box::new(BpskPlugin::default()));
    let mut policy = RelayTrustPolicy::deny_relays([] as [&str; 0]);
    // These routing-mechanics tests use synthetic peer ids that are not real Ed25519 verifying
    // keys, so origin-signature verification is disabled here; the authenticated relay path is
    // covered by `authenticated_relay_forwarding` with real keypairs.
    policy.set_require_authentication(false);
    MeshDaemon::new(
        engine, MODE, peer_id, 3, 0, 300_000, policy, 64, 3_600_000, seed, "N0CALL",
    )
}

/// A node with beacons enabled (beacon_interval_s=1).
fn make_beacon_node(lb: &LoopbackBackend, peer_id: [u8; 32]) -> MeshDaemon {
    let mut engine = ModemEngine::new(Box::new(lb.clone_shared()));
    let _ = engine.register_plugin(Box::new(BpskPlugin::default()));
    let mut policy = RelayTrustPolicy::deny_relays([] as [&str; 0]);
    // These routing-mechanics tests use synthetic peer ids that are not real Ed25519 verifying
    // keys, so origin-signature verification is disabled here; the authenticated relay path is
    // covered by `authenticated_relay_forwarding` with real keypairs.
    policy.set_require_authentication(false);
    MeshDaemon::new(
        engine, MODE, peer_id, 3, 1, 300_000, policy, 64, 3_600_000, [0u8; 32], "N0CALL",
    )
}

/// Step `node` up to `max` times (all at `now_ms`), accumulating events, stopping once `pred` matches.
/// A signed control response (e.g. a peer-query or route-discovery reply) now exceeds one modem frame
/// and is SAR-fragmented, so the receiver needs one `step` per fragment to reassemble it.
fn step_until(
    node: &mut MeshDaemon,
    now_ms: u64,
    max: usize,
    pred: impl Fn(&MeshEvent) -> bool,
) -> Vec<MeshEvent> {
    let mut all = Vec::new();
    for _ in 0..max {
        all.extend(node.step(now_ms));
        if all.iter().any(&pred) {
            break;
        }
    }
    all
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
        signature: None,
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

    // A processes B's (SAR-fragmented) response: caches B.
    let events_a2 = step_until(
        &mut node_a,
        1001,
        4,
        |e| matches!(e, MeshEvent::PeerDiscovered { peer_id } if *peer_id == peer_b),
    );
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

fn route_hop(id: u8, rel: u16) -> RouteHop {
    RouteHop {
        hop_peer_id: [id; 32],
        hop_trust_state: WireTrustState::Trusted as u8,
        estimated_latency_ms: 10,
        estimated_reliability_permille: rel,
    }
}

fn maint_envelope(
    msg_type: WireMsgType,
    src: [u8; 32],
    payload: Vec<u8>,
    nonce: u8,
) -> WireEnvelope {
    WireEnvelope {
        msg_type,
        flags: 0,
        session_id: 0,
        src_peer_id: src,
        dst_peer_id: [200u8; 32], // the route's destination (a routing tag)
        nonce: {
            let mut n = [0u8; 12];
            n[0] = nonce;
            n
        },
        timestamp_ms: 1000,
        hop_limit: 3,
        hop_index: 0,
        payload,
        signature: None,
    }
}

/// Route maintenance over-air (0x07 + 0x08): a signed update from a route-holder is verified and applied
/// to the receiver's table (`RouteUpdated`); a reject from an on-path hop then tears that route down
/// (`RouteRejected`). Exercises `apply_route_update` / `apply_route_reject` through the mesh dispatch.
#[test]
fn route_update_then_reject_over_air() {
    let dst = [200u8; 32];
    // The emitter's route-identity == verifying_key([0;32]) (same seed every make_node signs with), so a
    // receiver verifies the update signature against the envelope's src_peer_id.
    let emitter = RouteResponder::new(&[0u8; 32], 0);
    let emitter_id = emitter.peer_id();

    let lb_a = LoopbackBackend::new();
    let lb_e = LoopbackBackend::new();
    let mut node_a = make_node(&lb_a, [1u8; 32]);
    let mut node_e = make_node(&lb_e, emitter_id); // just a transmitter for the crafted envelopes

    // A signed update advertising a route to destination 200 for route_id 42.
    let hops = vec![route_hop(200, 950)];
    let update = emitter.build_route_update(42, 1, 0x0005, hops);
    let update_env = maint_envelope(
        WireMsgType::RelayRouteUpdate,
        emitter_id,
        update.encode().expect("encode update"),
        1,
    );

    node_e.send_relay(update_env).expect("transmit update");
    lb_a.fill_samples(&lb_e.drain_samples());
    let events_a = node_a.step(1_000);
    assert!(
        events_a.iter().any(
            |e| matches!(e, MeshEvent::RouteUpdated { destination, route_id } if *destination == dst && *route_id == 42)
        ),
        "A must verify + apply the route update; got {events_a:?}"
    );
    lb_a.drain_samples(); // clear A's own onward-propagation

    // A reject from the on-path hop (the destination 200) tears down route_id 42.
    let reject = RelayRouteReject {
        route_id: 42,
        reject_hop_peer_id: [200u8; 32],
        reason_code: 0x0002,
        trust_decision: WireTrustState::Untrusted as u8,
        policy_reference: 0,
    };
    let reject_env = maint_envelope(
        WireMsgType::RelayRouteReject,
        emitter_id,
        reject.encode(),
        2,
    );

    node_e.send_relay(reject_env).expect("transmit reject");
    lb_a.fill_samples(&lb_e.drain_samples());
    let events_a = node_a.step(2_000);
    assert!(
        events_a.iter().any(
            |e| matches!(e, MeshEvent::RouteRejected { destination, route_id } if *destination == dst && *route_id == 42)
        ),
        "A must tear down the route on an on-path reject; got {events_a:?}"
    );
}

/// A forwarder appends itself to a route request's source-accumulated path before re-flooding it, so a
/// downstream answerer can build the true multi-hop route. A → B (can't answer) → B re-floods; we decode
/// B's transmitted frame and confirm it carries B on the path, and that a responder for the destination
/// answers it with the full `[B, destination]` route.
#[test]
fn route_request_accumulates_the_forwarder_path() {
    let seed_dst = [7u8; 32];
    let dst_id = RouteResponder::new(&seed_dst, 0).peer_id();

    let lb_a = LoopbackBackend::new();
    let lb_b = LoopbackBackend::new();
    let mut node_a = make_node_seeded(&lb_a, [1u8; 32], [1u8; 32]);
    let mut node_b = make_node_seeded(&lb_b, [2u8; 32], [2u8; 32]);

    node_a
        .discover_route(dst_id, 1_000)
        .expect("discover_route");

    // A → B: B is not the destination and holds no route → it re-floods with itself appended.
    lb_b.fill_samples(&lb_a.drain_samples());
    node_b.step(1_010);
    let b_out = lb_b.drain_samples();
    assert!(!b_out.is_empty(), "B must re-flood the request");

    // Decode B's re-flooded frame.
    let lb_rx = LoopbackBackend::new();
    let mut rx = ModemEngine::new(Box::new(lb_rx.clone_shared()));
    rx.register_plugin(Box::new(BpskPlugin::default())).unwrap();
    lb_rx.fill_samples(&b_out);
    let bytes = rx.receive(MODE, None).expect("decode B's frame");
    let env = WireEnvelope::decode(&bytes).expect("wire envelope");
    assert_eq!(env.msg_type, WireMsgType::RouteDiscoveryRequest);
    let req = RouteDiscoveryRequest::decode(&env.payload).expect("route request");
    assert_eq!(
        req.accumulated_path,
        vec![[2u8; 32]],
        "B appended its own id to the source-accumulated path"
    );
    assert_eq!(req.destination_peer_id, dst_id, "destination is unchanged");

    // A responder for the destination answers the accumulated request with the full route.
    let mut responder = RouteResponder::new(&seed_dst, 0);
    let table = openpulse_core::route_discovery::RouteTable::new(4, 0);
    let resp = responder.answer(&req, &table).expect("destination answers");
    let hop_ids: Vec<[u8; 32]> = resp.hops.iter().map(|h| h.hop_peer_id).collect();
    assert_eq!(
        hop_ids,
        vec![[2u8; 32], dst_id],
        "route is [forwarder B, destination]"
    );
}

/// An envelope too large for one 255-byte modem frame is SAR-fragmented and reassembled on receive,
/// then delivered — the framing that makes signed/large control responses viable. Fragments are
/// delivered one-per-read via the loopback frame queue, matching real per-tick reception.
#[test]
fn oversized_envelope_survives_sar_fragmentation() {
    let peer_a = [1u8; 32];
    let peer_b = [2u8; 32];
    let lb_tx = LoopbackBackend::new();
    let lb_b = LoopbackBackend::new();
    let mut node_b = make_node(&lb_b, peer_b);

    // A relay-data envelope addressed to B, large enough that the whole envelope exceeds one frame.
    let mut env = relay_envelope(peer_a, peer_b, 77, 9);
    env.payload = vec![0xAB; 400];
    let bytes = env.encode().unwrap();
    assert!(
        bytes.len() > 255,
        "the test envelope must exceed one modem frame"
    );
    let frags = openpulse_core::sar::sar_encode(0, &bytes).unwrap();
    assert!(
        frags.len() >= 2,
        "the envelope must fragment; got {}",
        frags.len()
    );

    // Modulate each fragment as its own frame and hand it to B's frame queue (one per read).
    let mut tx_engine = ModemEngine::new(Box::new(lb_tx.clone_shared()));
    tx_engine
        .register_plugin(Box::new(BpskPlugin::default()))
        .unwrap();
    for frag in &frags {
        tx_engine.transmit(frag, MODE, None).unwrap();
        let samples = lb_tx.drain_samples();
        assert!(!samples.is_empty());
        lb_b.push_frame(&samples);
    }

    // B receives one fragment per step and delivers once the envelope reassembles.
    let mut delivered = false;
    for _ in 0..(frags.len() + 2) {
        let events = node_b.step(1000);
        if events
            .iter()
            .any(|e| matches!(e, MeshEvent::FrameDelivered { session_id: 77 }))
        {
            delivered = true;
            break;
        }
    }
    assert!(
        delivered,
        "B must reassemble the SAR-fragmented envelope and deliver it"
    );
}

// ── Origin-authentication tests (E3) ────────────────────────────────────────────

/// A real-keypair node whose `peer_id` is the verifying key of `seed`, with origin authentication
/// enabled (the production default). Returns the derived peer id alongside the daemon.
fn auth_node(lb: &LoopbackBackend, seed: [u8; 32]) -> ([u8; 32], MeshDaemon) {
    let peer_id = SigningKey::from_bytes(&seed).verifying_key().to_bytes();
    let mut engine = ModemEngine::new(Box::new(lb.clone_shared()));
    engine
        .register_plugin(Box::new(BpskPlugin::default()))
        .unwrap();
    let policy = RelayTrustPolicy::deny_relays([] as [&str; 0]); // require_authentication defaults true
    let daemon = MeshDaemon::new(
        engine, MODE, peer_id, 3, 0, 300_000, policy, 64, 3_600_000, seed, "N0CALL",
    );
    (peer_id, daemon)
}

/// A signed relay chunk from A authenticates at B and relays to C: A → B → C with auth enabled.
#[test]
fn authenticated_relay_forwarding() {
    let lb_a = LoopbackBackend::new();
    let lb_b = LoopbackBackend::new();
    let lb_c = LoopbackBackend::new();
    let (peer_a, mut node_a) = auth_node(&lb_a, [0xA1; 32]);
    let (_peer_b, mut node_b) = auth_node(&lb_b, [0xB2; 32]);
    let (peer_c, mut node_c) = auth_node(&lb_c, [0xC3; 32]);

    // A originates a relay chunk to C; send_relay signs it because src == A's peer id.
    let env = relay_envelope(peer_a, peer_c, 71, 1);
    node_a.send_relay(env).unwrap();
    let samples_a = lb_a.drain_samples();
    lb_b.fill_samples(&samples_a);

    let events_b = node_b.step(1000);
    assert!(
        events_b
            .iter()
            .any(|e| matches!(e, MeshEvent::Relay(RelayEvent::Forwarded { .. }))),
        "B must authenticate and forward A's signed frame; got {events_b:?}"
    );
    assert!(
        !events_b
            .iter()
            .any(|e| matches!(e, MeshEvent::Relay(RelayEvent::AuthenticationFailed { .. }))),
        "a validly-signed frame must not raise AuthenticationFailed"
    );

    let samples_b = lb_b.drain_samples();
    assert!(!samples_b.is_empty(), "B must forward the frame");
    lb_c.fill_samples(&samples_b);
    let events_c = node_c.step(1000);
    assert!(
        events_c
            .iter()
            .any(|e| matches!(e, MeshEvent::FrameDelivered { session_id: 71 })),
        "C must receive and deliver the authenticated frame; got {events_c:?}"
    );
}

/// An impersonator transmits an unsigned envelope claiming to originate from A (whose key it does not
/// hold). The relay must reject it as `AuthenticationFailed` and not forward it.
#[test]
fn impersonated_origin_rejected_at_relay() {
    let lb_tx = LoopbackBackend::new();
    let lb_b = LoopbackBackend::new();
    let (_peer_b, mut node_b) = auth_node(&lb_b, [0xB2; 32]);
    let peer_a = SigningKey::from_bytes(&[0xA1; 32])
        .verifying_key()
        .to_bytes();
    let peer_c = SigningKey::from_bytes(&[0xC3; 32])
        .verifying_key()
        .to_bytes();

    // Unsigned envelope (all-zero signature) claiming src = A, addressed to C so B would forward it.
    let env = relay_envelope(peer_a, peer_c, 72, 2);
    let bytes = env.encode().unwrap();

    let mut tx = ModemEngine::new(Box::new(lb_tx.clone_shared()));
    tx.register_plugin(Box::new(BpskPlugin::default())).unwrap();
    tx.transmit(&bytes, MODE, None).unwrap();
    let samples = lb_tx.drain_samples();
    lb_b.fill_samples(&samples);

    let events_b = node_b.step(1000);
    assert!(
        events_b
            .iter()
            .any(|e| matches!(e, MeshEvent::Relay(RelayEvent::AuthenticationFailed { .. }))),
        "B must reject the impersonated (unsigned) frame; got {events_b:?}"
    );
    assert!(
        lb_b.drain_samples().is_empty(),
        "B must not forward an unauthenticated frame"
    );
}
