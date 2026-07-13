//! End-to-end route discovery: an originator floods a `RouteDiscoveryRequest`, an intermediate relay
//! propagates it (hop-limit + dedup), the destination answers with a signed `RouteDiscoveryResponse`,
//! and the originator applies it into its route table. Exercises the drivers built on the previously
//! codec-only wire types.

use openpulse_core::query_propagation::{QueryForwardError, QueryForwarder};
use openpulse_core::relay::RelayTrustPolicy;
use openpulse_core::route_discovery::{
    RouteApplyError, RouteOriginator, RouteResponder, RouteTable,
};
use openpulse_core::wire_query::{RouteDiscoveryRequest, WireMsgType};

#[test]
fn originate_propagate_answer_apply_full_round_trip() {
    // Destination advertises capability bit 0; originator wants a route that supports it.
    let mut destination = RouteResponder::new(&[7u8; 32], 0x01);
    let dst_id = destination.peer_id();

    let mut originator = RouteOriginator::new([1u8; 32], 60_000);
    let mut table = RouteTable::new(64, 0);

    // 1. Originate the request.
    let (qid, req_env) = originator
        .originate(dst_id, 4, 0x01, 0, [9u8; 12], 1_000)
        .expect("originate");
    assert_eq!(req_env.msg_type, WireMsgType::RouteDiscoveryRequest);
    assert_eq!(originator.pending_len(), 1);

    // 2. An intermediate relay propagates the flooded request (hop_index bumps).
    let policy = RelayTrustPolicy::default();
    let mut relay = QueryForwarder::new(30_000, 256, policy);
    let fwd = relay
        .propagate(&req_env, 1_010)
        .expect("relay must propagate a route request");
    assert_eq!(fwd.hop_index, req_env.hop_index + 1);
    // A second copy of the same flood is suppressed.
    assert!(matches!(
        relay.propagate(&req_env, 1_020),
        Err(QueryForwardError::DuplicateDetected)
    ));

    // 3. The destination decodes the (forwarded) request and answers.
    let req = RouteDiscoveryRequest::decode(&fwd.payload).expect("decode request");
    let resp = destination
        .answer(&req, &table)
        .expect("destination answers");
    assert_eq!(resp.route_query_id, qid);
    assert_eq!(resp.hops[0].hop_peer_id, dst_id);

    // 4. The originator applies the response (verifying the signature against the responder id).
    let learned = originator
        .apply_response(&resp, &dst_id, &mut table, 1_100)
        .expect("apply");
    assert_eq!(learned, dst_id);
    assert_eq!(originator.pending_len(), 0);
    assert_eq!(table.best_route(&dst_id).unwrap().route_id, resp.route_id);

    // 5. A replayed response is now unknown (the query was consumed).
    assert_eq!(
        originator.apply_response(&resp, &dst_id, &mut table, 1_110),
        Err(RouteApplyError::UnknownQuery(qid))
    );
}

#[test]
fn relay_drops_a_route_request_past_the_hop_limit() {
    let mut originator = RouteOriginator::new([1u8; 32], 60_000);
    let (_qid, mut env) = originator
        .originate([9u8; 32], 2, 0, 0, [3u8; 12], 1_000)
        .expect("originate");
    env.hop_index = env.hop_limit; // already at the limit

    let mut relay = QueryForwarder::new(30_000, 256, RelayTrustPolicy::default());
    assert!(matches!(
        relay.propagate(&env, 1_010),
        Err(QueryForwardError::HopLimitExceeded { .. })
    ));
}

#[test]
fn an_intermediate_holding_a_cached_route_answers_on_the_destinations_behalf() {
    // Node R is not the destination D, but already has a route to D; it must answer.
    let mut relay_node = RouteResponder::new(&[5u8; 32], 0xFF);
    let d_id = [200u8; 32];

    // Seed R's table with a known route to D (as if learned earlier).
    let mut table = RouteTable::new(64, 0);
    table.record(
        openpulse_core::route_discovery::RouteEntry {
            destination_peer_id: d_id,
            route_id: 77,
            hops: vec![openpulse_core::wire_query::RouteHop {
                hop_peer_id: d_id,
                hop_trust_state: openpulse_core::wire_query::WireTrustState::Trusted as u8,
                estimated_latency_ms: 20,
                estimated_reliability_permille: 950,
            }],
            discovered_at_ms: 0,
        },
        0,
    );

    let mut originator = RouteOriginator::new([1u8; 32], 60_000);
    let (_qid, env) = originator
        .originate(d_id, 4, 0, 0, [9u8; 12], 1_000)
        .unwrap();
    let req = RouteDiscoveryRequest::decode(&env.payload).unwrap();

    let resp = relay_node
        .answer(&req, &table)
        .expect("a node with a cached route answers");
    // Signed by R, so it must verify against R's id (self-authenticating).
    let mut orig_table = RouteTable::new(64, 0);
    let learned = originator
        .apply_response(&resp, &relay_node.peer_id(), &mut orig_table, 1_100)
        .expect("apply cached-route answer");
    assert_eq!(learned, d_id);
    assert_eq!(
        orig_table.best_route(&d_id).unwrap().hops[0].hop_peer_id,
        d_id
    );
}
