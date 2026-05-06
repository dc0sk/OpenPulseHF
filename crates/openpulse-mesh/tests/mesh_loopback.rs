//! 3-node mesh relay loopback: A → B → C
//!
//! Verifies that a `RelayDataChunk` frame addressed to node C arrives via relay
//! through node B from node A, using clean loopback channels (no channel distortion).

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::peer_cache::TrustFilter;
use openpulse_core::relay::{RelayEvent, RelayTrustPolicy};
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
