//! On-demand route discovery: originate a `RouteDiscoveryRequest`, answer it when this node is the
//! destination (or already knows a route), and apply the returned `RouteDiscoveryResponse` into a
//! local route table.
//!
//! The wire codecs live in [`crate::wire_query`]; this module is the missing *driver* — the logic
//! that actually creates, answers, and records those messages (they were codec-only). Propagation of
//! the flood reuses [`crate::query_propagation::QueryPropagationTracker`] for `route_query_id` dedup;
//! forwarding itself is done by the existing forwarders.
//!
//! **Path model.** A [`WireEnvelope`] carries no accumulating hop trail (only a `hop_index` counter),
//! so a forwarded request does not reveal the path it took. The answerer therefore builds the
//! response's `hops` from what *it* can vouch for: the destination answers with a single hop (itself),
//! and a node holding a cached route answers with that route. Full source-accumulated multi-hop paths
//! would need a new request field (a future TLV extension) and are out of scope here.
//!
//! **Signatures are self-authenticating** (as in [`crate::peer_descriptor`]): the responder signs the
//! response with the Ed25519 key whose public bytes *are* its `peer_id`, so the originator verifies
//! with the responder id off the reply envelope — no external key store.

use std::collections::HashMap;
use std::collections::VecDeque;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use thiserror::Error;

use crate::wire_query::{
    RouteDiscoveryRequest, RouteDiscoveryResponse, RouteHop, WireEnvelope, WireMsgType,
    WireQueryError, WireTrustState,
};

/// Errors from applying a received route-discovery response.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RouteApplyError {
    /// The response's `route_query_id` matches no request this node originated (or it expired).
    #[error("no pending route query {0}")]
    UnknownQuery(u64),
    /// The response signature did not verify against the responder's peer id.
    #[error("invalid route-response signature")]
    BadSignature,
    /// The response carried no hops — an empty route is not a route.
    #[error("route response has no hops")]
    EmptyRoute,
}

/// A discovered end-to-end route to a destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteEntry {
    /// The destination this route reaches.
    pub destination_peer_id: [u8; 32],
    /// Opaque route id minted by the responder.
    pub route_id: u64,
    /// The hop sequence toward the destination (as vouched by the responder).
    pub hops: Vec<RouteHop>,
    /// When this route was recorded (caller clock, ms).
    pub discovered_at_ms: u64,
}

impl RouteEntry {
    /// The route's bottleneck reliability (min over hops), in permille. `0` for an empty route.
    pub fn min_reliability_permille(&self) -> u16 {
        self.hops
            .iter()
            .map(|h| h.estimated_reliability_permille)
            .min()
            .unwrap_or(0)
    }

    /// `true` if the candidate is a better route to the same destination than `self`: fewer hops
    /// wins; on a tie, higher bottleneck reliability wins.
    fn is_improved_by(&self, other: &RouteEntry) -> bool {
        match other.hops.len().cmp(&self.hops.len()) {
            std::cmp::Ordering::Less => true,
            std::cmp::Ordering::Greater => false,
            std::cmp::Ordering::Equal => {
                other.min_reliability_permille() > self.min_reliability_permille()
            }
        }
    }
}

/// A bounded, TTL-expiring store of discovered routes, keyed by destination peer id (best route kept).
#[derive(Debug)]
pub struct RouteTable {
    capacity: usize,
    ttl_ms: u64,
    entries: HashMap<[u8; 32], RouteEntry>,
    lru: VecDeque<[u8; 32]>,
}

impl RouteTable {
    /// Create an empty table holding at most `capacity` destinations, expiring entries after `ttl_ms`.
    pub fn new(capacity: usize, ttl_ms: u64) -> Self {
        Self {
            capacity: capacity.max(1),
            ttl_ms,
            entries: HashMap::new(),
            lru: VecDeque::new(),
        }
    }

    /// Number of destinations with a stored route.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` when no routes are stored.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Record a route, keeping only the better one when a route to the same destination already
    /// exists (fewer hops, then higher bottleneck reliability; an equal route refreshes the entry).
    /// Returns `true` if the stored best route for that destination changed.
    pub fn record(&mut self, entry: RouteEntry, now_ms: u64) -> bool {
        self.evict_expired(now_ms);
        let dst = entry.destination_peer_id;
        let changed = match self.entries.get(&dst) {
            Some(existing) => existing.is_improved_by(&entry),
            None => true,
        };
        if changed {
            self.entries.insert(dst, entry);
        } else if let Some(existing) = self.entries.get_mut(&dst) {
            existing.discovered_at_ms = now_ms; // refresh TTL on an equal-or-worse re-confirmation
        }
        self.touch(dst);
        self.enforce_capacity();
        changed
    }

    /// The best known route to `destination`, or `None`.
    pub fn best_route(&self, destination: &[u8; 32]) -> Option<&RouteEntry> {
        self.entries.get(destination)
    }

    /// Drop entries older than the TTL.
    pub fn evict_expired(&mut self, now_ms: u64) {
        if self.ttl_ms == 0 {
            return;
        }
        let ttl = self.ttl_ms;
        self.entries
            .retain(|_, e| now_ms.saturating_sub(e.discovered_at_ms) < ttl);
        self.lru.retain(|k| self.entries.contains_key(k));
    }

    fn touch(&mut self, dst: [u8; 32]) {
        self.lru.retain(|k| *k != dst);
        self.lru.push_back(dst);
    }

    fn enforce_capacity(&mut self) {
        while self.entries.len() > self.capacity {
            if let Some(oldest) = self.lru.pop_front() {
                self.entries.remove(&oldest);
            } else {
                break;
            }
        }
    }
}

/// Canonical byte serialization signed by a route response: the query/route ids and every hop, in
/// order. Deterministic and independent of the wire framing.
fn route_response_canonical(query_id: u64, route_id: u64, hops: &[RouteHop]) -> Vec<u8> {
    let mut out = Vec::with_capacity(16 + 1 + hops.len() * 37);
    out.extend_from_slice(&query_id.to_le_bytes());
    out.extend_from_slice(&route_id.to_le_bytes());
    out.push(hops.len() as u8);
    for h in hops {
        out.extend_from_slice(&h.hop_peer_id);
        out.push(h.hop_trust_state);
        out.extend_from_slice(&h.estimated_latency_ms.to_le_bytes());
        out.extend_from_slice(&h.estimated_reliability_permille.to_le_bytes());
    }
    out
}

/// Sign the `route_signature` field of a response with the responder's Ed25519 key.
pub fn sign_route_response(
    query_id: u64,
    route_id: u64,
    hops: &[RouteHop],
    signing_key: &SigningKey,
) -> Vec<u8> {
    let canonical = route_response_canonical(query_id, route_id, hops);
    let sig: Signature = signing_key.sign(&canonical);
    sig.to_bytes().to_vec()
}

/// Verify a response's signature against `responder_peer_id` (the responder's peer id, which *is* its
/// Ed25519 verifying key — taken from the reply envelope's `src_peer_id`).
pub fn verify_route_response(
    response: &RouteDiscoveryResponse,
    responder_peer_id: &[u8; 32],
) -> Result<(), RouteApplyError> {
    let key =
        VerifyingKey::from_bytes(responder_peer_id).map_err(|_| RouteApplyError::BadSignature)?;
    let Ok(sig_arr): Result<[u8; 64], _> = response.route_signature.as_slice().try_into() else {
        return Err(RouteApplyError::BadSignature);
    };
    let sig = Signature::from_bytes(&sig_arr);
    let canonical =
        route_response_canonical(response.route_query_id, response.route_id, &response.hops);
    key.verify(&canonical, &sig)
        .map_err(|_| RouteApplyError::BadSignature)
}

/// Answers route-discovery requests: replies when this node is the destination (and meets the
/// requested capabilities) or already holds a route to it.
pub struct RouteResponder {
    my_peer_id: [u8; 32],
    my_capability_mask: u32,
    signing_key: SigningKey,
    next_route_id: u64,
}

impl RouteResponder {
    /// Build a responder from a 32-byte Ed25519 seed; the derived verifying key is this node's
    /// `peer_id`. `capability_mask` advertises what this node can do (matched against a request's
    /// `required_capability_mask`).
    pub fn new(signing_key_seed: &[u8; 32], capability_mask: u32) -> Self {
        let signing_key = SigningKey::from_bytes(signing_key_seed);
        let my_peer_id = signing_key.verifying_key().to_bytes();
        // Seed route ids from the peer id so distinct nodes don't collide on low ids.
        let next_route_id = u64::from_le_bytes(my_peer_id[..8].try_into().unwrap_or([0u8; 8])) | 1;
        Self {
            my_peer_id,
            my_capability_mask: capability_mask,
            signing_key,
            next_route_id,
        }
    }

    /// This node's peer id (the Ed25519 verifying key bytes).
    pub fn peer_id(&self) -> [u8; 32] {
        self.my_peer_id
    }

    /// Decide whether to answer `request`. Returns a signed [`RouteDiscoveryResponse`] when this node
    /// is the destination (and satisfies `required_capability_mask`) or holds a cached route to it;
    /// `None` when it cannot answer and the caller should forward the request onward.
    pub fn answer(
        &mut self,
        request: &RouteDiscoveryRequest,
        route_table: &RouteTable,
    ) -> Option<RouteDiscoveryResponse> {
        let required = request.required_capability_mask;

        // Case 1: this node IS the destination.
        if request.destination_peer_id == self.my_peer_id {
            if self.my_capability_mask & required != required {
                return None; // I'm the target but can't meet the requested capabilities.
            }
            let hops = vec![RouteHop {
                hop_peer_id: self.my_peer_id,
                hop_trust_state: WireTrustState::Trusted as u8,
                estimated_latency_ms: 0,
                estimated_reliability_permille: 1000,
            }];
            return Some(self.build_signed(request.route_query_id, hops));
        }

        // Case 2: this node already knows a route to the destination.
        if let Some(entry) = route_table.best_route(&request.destination_peer_id) {
            if entry.hops.is_empty() {
                return None;
            }
            return Some(self.build_signed(request.route_query_id, entry.hops.clone()));
        }

        None
    }

    fn build_signed(&mut self, query_id: u64, hops: Vec<RouteHop>) -> RouteDiscoveryResponse {
        let route_id = self.next_route_id;
        self.next_route_id = self.next_route_id.wrapping_add(1);
        let route_signature = sign_route_response(query_id, route_id, &hops, &self.signing_key);
        RouteDiscoveryResponse {
            route_query_id: query_id,
            route_id,
            hops,
            route_signature,
        }
    }
}

/// Tracks a pending originated route query.
#[derive(Debug, Clone)]
struct PendingQuery {
    destination_peer_id: [u8; 32],
    deadline_ms: u64,
}

/// Originates route-discovery requests, tracks the outstanding ones, and applies responses into a
/// [`RouteTable`].
pub struct RouteOriginator {
    my_peer_id: [u8; 32],
    next_query_id: u64,
    query_timeout_ms: u64,
    pending: HashMap<u64, PendingQuery>,
}

impl RouteOriginator {
    /// Create an originator for `my_peer_id`. Outstanding queries expire `query_timeout_ms` after they
    /// are sent (a response arriving later is treated as unknown).
    pub fn new(my_peer_id: [u8; 32], query_timeout_ms: u64) -> Self {
        // Seed query ids from the peer id so a fresh node doesn't reuse a neighbour's low ids.
        let next_query_id = u64::from_le_bytes(my_peer_id[..8].try_into().unwrap_or([0u8; 8])) | 1;
        Self {
            my_peer_id,
            next_query_id,
            query_timeout_ms,
            pending: HashMap::new(),
        }
    }

    /// Number of outstanding (un-answered, un-expired) route queries.
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// Originate a route query for `destination_peer_id`. Returns the minted `route_query_id` and the
    /// `RouteDiscoveryRequest` envelope ready to transmit (msg type `RouteDiscoveryRequest`, hop limit
    /// `max_hops`). The caller supplies a unique `nonce` for envelope-level replay suppression.
    pub fn originate(
        &mut self,
        destination_peer_id: [u8; 32],
        max_hops: u8,
        required_capability_mask: u32,
        policy_flags: u16,
        nonce: [u8; 12],
        now_ms: u64,
    ) -> Result<(u64, WireEnvelope), WireQueryError> {
        self.expire_pending(now_ms);
        let route_query_id = self.next_query_id;
        self.next_query_id = self.next_query_id.wrapping_add(1);

        let request = RouteDiscoveryRequest {
            route_query_id,
            destination_peer_id,
            max_hops,
            required_capability_mask,
            policy_flags,
        };
        let envelope = WireEnvelope {
            msg_type: WireMsgType::RouteDiscoveryRequest,
            flags: 0,
            session_id: route_query_id,
            src_peer_id: self.my_peer_id,
            dst_peer_id: destination_peer_id,
            nonce,
            timestamp_ms: now_ms,
            hop_limit: max_hops,
            hop_index: 0,
            payload: request.encode(),
            auth_tag: [0u8; 16],
        };
        // Validate the envelope encodes before we commit the pending entry.
        envelope.encode()?;
        self.pending.insert(
            route_query_id,
            PendingQuery {
                destination_peer_id,
                deadline_ms: now_ms.saturating_add(self.query_timeout_ms),
            },
        );
        Ok((route_query_id, envelope))
    }

    /// Apply a response to one of our outstanding queries: verify its signature against
    /// `responder_peer_id` (the reply envelope's `src_peer_id`), then record the route in `table`.
    /// Returns the destination the route reaches. The query is consumed (a duplicate/late response
    /// then reports `UnknownQuery`).
    pub fn apply_response(
        &mut self,
        response: &RouteDiscoveryResponse,
        responder_peer_id: &[u8; 32],
        table: &mut RouteTable,
        now_ms: u64,
    ) -> Result<[u8; 32], RouteApplyError> {
        self.expire_pending(now_ms);
        let pending = self
            .pending
            .get(&response.route_query_id)
            .cloned()
            .ok_or(RouteApplyError::UnknownQuery(response.route_query_id))?;
        if response.hops.is_empty() {
            return Err(RouteApplyError::EmptyRoute);
        }
        verify_route_response(response, responder_peer_id)?;

        let dst = pending.destination_peer_id;
        table.record(
            RouteEntry {
                destination_peer_id: dst,
                route_id: response.route_id,
                hops: response.hops.clone(),
                discovered_at_ms: now_ms,
            },
            now_ms,
        );
        self.pending.remove(&response.route_query_id);
        Ok(dst)
    }

    /// Drop outstanding queries whose deadline has passed.
    pub fn expire_pending(&mut self, now_ms: u64) {
        self.pending.retain(|_, p| now_ms < p.deadline_ms);
    }

    /// This node's peer id.
    pub fn peer_id(&self) -> [u8; 32] {
        self.my_peer_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn responder(seed: u8, cap: u32) -> RouteResponder {
        RouteResponder::new(&[seed; 32], cap)
    }

    fn a_hop(id: u8, rel: u16) -> RouteHop {
        RouteHop {
            hop_peer_id: [id; 32],
            hop_trust_state: WireTrustState::Trusted as u8,
            estimated_latency_ms: 10,
            estimated_reliability_permille: rel,
        }
    }

    #[test]
    fn destination_answers_with_a_verifiable_single_hop() {
        let mut dst = responder(7, 0xFF);
        let dst_id = dst.peer_id();
        let mut orig = RouteOriginator::new([1u8; 32], 60_000);
        let (_qid, env) = orig.originate(dst_id, 4, 0x01, 0, [9u8; 12], 1000).unwrap();
        let req = RouteDiscoveryRequest::decode(&env.payload).unwrap();

        let table = RouteTable::new(16, 0);
        let resp = dst.answer(&req, &table).expect("destination must answer");
        assert_eq!(resp.hops.len(), 1);
        assert_eq!(resp.hops[0].hop_peer_id, dst_id);
        verify_route_response(&resp, &dst_id).expect("signature must verify");
    }

    #[test]
    fn non_destination_without_a_route_does_not_answer() {
        let mut node = responder(3, 0xFF);
        let req = RouteDiscoveryRequest {
            route_query_id: 5,
            destination_peer_id: [200u8; 32], // not this node
            max_hops: 4,
            required_capability_mask: 0,
            policy_flags: 0,
        };
        let table = RouteTable::new(16, 0);
        assert!(node.answer(&req, &table).is_none());
    }

    #[test]
    fn destination_declines_when_capabilities_are_unmet() {
        let mut dst = responder(7, 0x01); // only bit 0
        let dst_id = dst.peer_id();
        let req = RouteDiscoveryRequest {
            route_query_id: 5,
            destination_peer_id: dst_id,
            max_hops: 4,
            required_capability_mask: 0x02, // requires bit 1, which we lack
            policy_flags: 0,
        };
        let table = RouteTable::new(16, 0);
        assert!(dst.answer(&req, &table).is_none());
    }

    #[test]
    fn round_trip_originate_answer_apply_records_the_route() {
        let mut dst = responder(7, 0xFF);
        let dst_id = dst.peer_id();
        let mut orig = RouteOriginator::new([1u8; 32], 60_000);
        let mut table = RouteTable::new(16, 0);

        let (qid, env) = orig.originate(dst_id, 4, 0x01, 0, [9u8; 12], 1000).unwrap();
        assert_eq!(orig.pending_len(), 1);
        let req = RouteDiscoveryRequest::decode(&env.payload).unwrap();
        let resp = dst.answer(&req, &table).unwrap();
        assert_eq!(resp.route_query_id, qid);

        let learned = orig
            .apply_response(&resp, &dst_id, &mut table, 1100)
            .expect("apply must succeed");
        assert_eq!(learned, dst_id);
        assert_eq!(orig.pending_len(), 0, "query is consumed");
        assert_eq!(
            table.best_route(&dst_id).unwrap().hops[0].hop_peer_id,
            dst_id
        );
    }

    #[test]
    fn apply_rejects_a_tampered_signature() {
        let mut dst = responder(7, 0xFF);
        let dst_id = dst.peer_id();
        let mut orig = RouteOriginator::new([1u8; 32], 60_000);
        let mut table = RouteTable::new(16, 0);
        let (_qid, env) = orig.originate(dst_id, 4, 0, 0, [9u8; 12], 1000).unwrap();
        let req = RouteDiscoveryRequest::decode(&env.payload).unwrap();
        let mut resp = dst.answer(&req, &table).unwrap();
        resp.hops[0].estimated_reliability_permille = 500; // tamper after signing

        assert_eq!(
            orig.apply_response(&resp, &dst_id, &mut table, 1100),
            Err(RouteApplyError::BadSignature)
        );
    }

    #[test]
    fn apply_rejects_an_unknown_or_expired_query() {
        let mut dst = responder(7, 0xFF);
        let dst_id = dst.peer_id();
        let mut orig = RouteOriginator::new([1u8; 32], 5_000);
        let mut table = RouteTable::new(16, 0);
        let (_qid, env) = orig.originate(dst_id, 4, 0, 0, [9u8; 12], 1000).unwrap();
        let req = RouteDiscoveryRequest::decode(&env.payload).unwrap();
        let resp = dst.answer(&req, &table).unwrap();

        // Response arrives after the query timed out (1000 + 5000).
        let err = orig
            .apply_response(&resp, &dst_id, &mut table, 7000)
            .unwrap_err();
        assert_eq!(err, RouteApplyError::UnknownQuery(resp.route_query_id));
    }

    #[test]
    fn route_table_keeps_the_shorter_route_and_expires() {
        let mut table = RouteTable::new(16, 10_000);
        let dst = [42u8; 32];
        let two_hop = RouteEntry {
            destination_peer_id: dst,
            route_id: 1,
            hops: vec![a_hop(2, 900), a_hop(3, 900)],
            discovered_at_ms: 0,
        };
        let one_hop = RouteEntry {
            destination_peer_id: dst,
            route_id: 2,
            hops: vec![a_hop(2, 800)],
            discovered_at_ms: 100,
        };
        assert!(table.record(two_hop, 0));
        assert!(
            table.record(one_hop, 100),
            "a shorter route must replace a longer one"
        );
        assert_eq!(table.best_route(&dst).unwrap().hops.len(), 1);

        table.evict_expired(20_000);
        assert!(table.best_route(&dst).is_none(), "route expired past TTL");
    }

    #[test]
    fn route_table_prefers_higher_reliability_on_equal_length() {
        let mut table = RouteTable::new(16, 0);
        let dst = [42u8; 32];
        table.record(
            RouteEntry {
                destination_peer_id: dst,
                route_id: 1,
                hops: vec![a_hop(2, 700)],
                discovered_at_ms: 0,
            },
            0,
        );
        let improved = table.record(
            RouteEntry {
                destination_peer_id: dst,
                route_id: 2,
                hops: vec![a_hop(3, 950)],
                discovered_at_ms: 1,
            },
            1,
        );
        assert!(improved);
        assert_eq!(table.best_route(&dst).unwrap().route_id, 2);
    }
}
