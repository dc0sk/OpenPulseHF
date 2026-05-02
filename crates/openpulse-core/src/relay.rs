use std::collections::{HashMap, HashSet};

use crate::peer_cache::{PeerCache, TrustLevel};
use crate::wire_query::WireEnvelope;

// ------------------------------------------------------------------
// Errors
// ------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayRouteError {
    EmptyRoute,
    LoopDetected { peer_id: String },
    TooManyHops { hops: usize, max_hops: usize },
    TrustPolicyRejected { peer_id: String },
    NoValidRoute,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayForwardError {
    /// hop_index has reached hop_limit; message must be dropped.
    HopLimitExceeded { hop_index: u8, hop_limit: u8 },
    /// (session_id, nonce) already forwarded; duplicate suppressed.
    DuplicateDetected,
    /// src_peer_id is denied by this node's trust policy.
    PolicyRejected { src_peer_id: [u8; 32] },
}

// ------------------------------------------------------------------
// Relay observability events
// ------------------------------------------------------------------

/// Events emitted by `RelayForwarder` for structured logging.
#[derive(Debug, Clone)]
pub enum RelayEvent {
    /// A relay envelope was forwarded; hop_index has been incremented.
    Forwarded {
        session_id: u64,
        hop_index_out: u8,
        hop_limit: u8,
    },
    /// Envelope dropped because hop_index reached hop_limit.
    HopLimitExceeded { session_id: u64, hop_index: u8 },
    /// Duplicate envelope suppressed.
    DuplicateSuppressed { session_id: u64, nonce: [u8; 12] },
    /// Envelope dropped due to originator trust policy.
    PolicyRejected {
        session_id: u64,
        src_peer_id: [u8; 32],
    },
}

// ------------------------------------------------------------------
// Trust policy
// ------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct RelayTrustPolicy {
    denied_relays: HashSet<String>,
}

impl RelayTrustPolicy {
    pub fn deny_relays<I, S>(denied: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            denied_relays: denied.into_iter().map(Into::into).collect(),
        }
    }

    pub fn allows(&self, relay_peer_id: &str) -> bool {
        !self.denied_relays.contains(relay_peer_id)
    }
}

// ------------------------------------------------------------------
// Route validation (existing API, unchanged)
// ------------------------------------------------------------------

pub fn validate_route_no_loops(route: &[String], max_hops: usize) -> Result<(), RelayRouteError> {
    if route.is_empty() {
        return Err(RelayRouteError::EmptyRoute);
    }

    let hops = route.len().saturating_sub(1);
    if hops > max_hops {
        return Err(RelayRouteError::TooManyHops { hops, max_hops });
    }

    let mut seen = HashSet::new();
    for peer in route {
        if !seen.insert(peer) {
            return Err(RelayRouteError::LoopDetected {
                peer_id: peer.clone(),
            });
        }
    }

    Ok(())
}

pub fn validate_route_with_policy(
    route: &[String],
    max_hops: usize,
    policy: &RelayTrustPolicy,
) -> Result<(), RelayRouteError> {
    validate_route_no_loops(route, max_hops)?;

    if route.len() <= 2 {
        return Ok(());
    }

    for relay in &route[1..route.len() - 1] {
        if !policy.allows(relay) {
            return Err(RelayRouteError::TrustPolicyRejected {
                peer_id: relay.clone(),
            });
        }
    }

    Ok(())
}

pub fn select_best_valid_route(
    candidates: &[Vec<String>],
    max_hops: usize,
    policy: &RelayTrustPolicy,
) -> Result<Vec<String>, RelayRouteError> {
    let mut best: Option<&Vec<String>> = None;

    for route in candidates {
        if validate_route_with_policy(route, max_hops, policy).is_ok() {
            match best {
                Some(current_best) if route.len() >= current_best.len() => {}
                _ => best = Some(route),
            }
        }
    }

    best.cloned().ok_or(RelayRouteError::NoValidRoute)
}

// ------------------------------------------------------------------
// Trust/quality-scored path planning
// ------------------------------------------------------------------

fn trust_weight(level: TrustLevel) -> u32 {
    match level {
        TrustLevel::Verified => 4,
        TrustLevel::PskVerified => 3,
        TrustLevel::Unknown => 2,
        TrustLevel::Reduced => 1,
    }
}

/// Composite score for a relay hop: trust_weight × route_quality.
/// Higher is better.  Returns 0 if the peer is not in the cache.
fn hop_score(peer_id: &str, cache: &PeerCache) -> u32 {
    cache
        .peek(peer_id)
        .map(|r| trust_weight(r.trust_level) * r.route_quality as u32)
        .unwrap_or(0)
}

/// Score a route by the bottleneck (minimum) hop score across intermediate relays.
///
/// A direct route (no intermediate hops) receives the maximum score so it is
/// never penalised for skipping hops.
pub fn score_route(route: &[String], cache: &PeerCache) -> u32 {
    if route.len() <= 2 {
        return u32::MAX;
    }
    route[1..route.len() - 1]
        .iter()
        .map(|peer_id| hop_score(peer_id, cache))
        .min()
        .unwrap_or(0)
}

/// Select the valid route with the highest composite score.
///
/// When scores are tied the shorter route wins.  Returns `NoValidRoute` if no
/// candidate passes policy and loop checks.
pub fn select_best_scored_route(
    candidates: &[Vec<String>],
    max_hops: usize,
    policy: &RelayTrustPolicy,
    cache: &PeerCache,
) -> Result<Vec<String>, RelayRouteError> {
    let mut best: Option<(&Vec<String>, u32)> = None;

    for route in candidates {
        if validate_route_with_policy(route, max_hops, policy).is_err() {
            continue;
        }
        let score = score_route(route, cache);
        let is_better = match best {
            None => true,
            Some((current, best_score)) => {
                score > best_score || (score == best_score && route.len() < current.len())
            }
        };
        if is_better {
            best = Some((route, score));
        }
    }

    best.map(|(r, _)| r.clone())
        .ok_or(RelayRouteError::NoValidRoute)
}

// ------------------------------------------------------------------
// Relay forwarder
// ------------------------------------------------------------------

/// Stateful relay forwarding node with duplicate suppression and hop limiting.
///
/// Each received `WireEnvelope` is checked for:
/// 1. hop_index < hop_limit — drops the packet if the hop budget is exhausted.
/// 2. (session_id, nonce) uniqueness — suppresses replays.
/// 3. src_peer_id trust policy — rejects envelopes from denied originators.
///
/// On success the returned envelope has `hop_index` incremented by one, ready
/// to be forwarded to the next hop.
#[derive(Debug, Clone)]
pub struct RelayForwarder {
    policy: RelayTrustPolicy,
    /// Maps (session_id, nonce) → first_seen_ms for replay suppression.
    seen: HashMap<(u64, [u8; 12]), u64>,
    ttl_ms: u64,
    events: Vec<RelayEvent>,
}

impl RelayForwarder {
    pub fn new(ttl_ms: u64, policy: RelayTrustPolicy) -> Self {
        Self {
            policy,
            seen: HashMap::new(),
            ttl_ms,
            events: Vec::new(),
        }
    }

    /// Process one envelope for relay forwarding.
    ///
    /// Returns the modified envelope (hop_index incremented) on success.
    /// The caller is responsible for routing it to the next hop peer.
    pub fn forward(
        &mut self,
        envelope: &WireEnvelope,
        now_ms: u64,
    ) -> Result<WireEnvelope, RelayForwardError> {
        self.evict_expired(now_ms);

        // 1. Hop-limit enforcement
        if envelope.hop_index >= envelope.hop_limit {
            self.events.push(RelayEvent::HopLimitExceeded {
                session_id: envelope.session_id,
                hop_index: envelope.hop_index,
            });
            return Err(RelayForwardError::HopLimitExceeded {
                hop_index: envelope.hop_index,
                hop_limit: envelope.hop_limit,
            });
        }

        // 2. Duplicate suppression
        let key = (envelope.session_id, envelope.nonce);
        if self.seen.contains_key(&key) {
            self.events.push(RelayEvent::DuplicateSuppressed {
                session_id: envelope.session_id,
                nonce: envelope.nonce,
            });
            return Err(RelayForwardError::DuplicateDetected);
        }

        // 3. Trust policy — check originating peer (src_peer_id)
        // Convert [u8;32] to a hex string for the policy lookup.
        let src_hex = hex_peer_id(&envelope.src_peer_id);
        if !self.policy.allows(&src_hex) {
            self.events.push(RelayEvent::PolicyRejected {
                session_id: envelope.session_id,
                src_peer_id: envelope.src_peer_id,
            });
            return Err(RelayForwardError::PolicyRejected {
                src_peer_id: envelope.src_peer_id,
            });
        }

        // Record and forward
        self.seen.insert(key, now_ms);
        let mut out = envelope.clone();
        out.hop_index += 1;
        self.events.push(RelayEvent::Forwarded {
            session_id: out.session_id,
            hop_index_out: out.hop_index,
            hop_limit: out.hop_limit,
        });
        Ok(out)
    }

    /// Drain and return all buffered relay events since the last call.
    pub fn drain_events(&mut self) -> Vec<RelayEvent> {
        std::mem::take(&mut self.events)
    }

    /// Remove expired replay-suppression entries.
    pub fn evict_expired(&mut self, now_ms: u64) {
        self.seen
            .retain(|_, first_seen| now_ms.saturating_sub(*first_seen) <= self.ttl_ms);
    }
}

/// Format a peer_id byte array as a lowercase hex string for policy lookup.
fn hex_peer_id(id: &[u8; 32]) -> String {
    id.iter().fold(String::with_capacity(64), |mut s, b| {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
        s
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire_query::{WireEnvelope, WireMsgType};

    fn route(peers: &[&str]) -> Vec<String> {
        peers.iter().map(|v| v.to_string()).collect()
    }

    fn test_envelope(session_id: u64, hop_limit: u8, hop_index: u8) -> WireEnvelope {
        WireEnvelope {
            msg_type: WireMsgType::RelayDataChunk,
            flags: 0,
            session_id,
            src_peer_id: [0xaa; 32],
            dst_peer_id: [0xbb; 32],
            nonce: [0x11; 12],
            timestamp_ms: 1_000,
            hop_limit,
            hop_index,
            payload: vec![],
            auth_tag: [0; 16],
        }
    }

    #[test]
    fn multi_hop_route_passes_when_within_hop_limit() {
        let route = route(&["src", "relay-a", "relay-b", "dst"]);
        assert!(validate_route_no_loops(&route, 3).is_ok());
    }

    #[test]
    fn route_fails_when_loop_is_detected() {
        let route = route(&["src", "relay-a", "relay-b", "relay-a", "dst"]);
        let err = validate_route_no_loops(&route, 5).expect_err("loop must be rejected");
        assert_eq!(
            err,
            RelayRouteError::LoopDetected {
                peer_id: "relay-a".to_string()
            }
        );
    }

    #[test]
    fn route_fails_when_hop_count_exceeds_limit() {
        let route = route(&["src", "relay-a", "relay-b", "dst"]);
        let err = validate_route_no_loops(&route, 2).expect_err("too many hops");
        assert_eq!(
            err,
            RelayRouteError::TooManyHops {
                hops: 3,
                max_hops: 2,
            }
        );
    }

    #[test]
    fn trust_policy_failure_rejects_route() {
        let policy = RelayTrustPolicy::deny_relays(["relay-b"]);
        let route = route(&["src", "relay-a", "relay-b", "dst"]);
        let err = validate_route_with_policy(&route, 3, &policy)
            .expect_err("untrusted relay must be rejected");
        assert_eq!(
            err,
            RelayRouteError::TrustPolicyRejected {
                peer_id: "relay-b".to_string()
            }
        );
    }

    #[test]
    fn selects_shortest_valid_route_and_skips_policy_failures() {
        let policy = RelayTrustPolicy::deny_relays(["relay-x"]);
        let candidates = vec![
            route(&["src", "relay-x", "dst"]),
            route(&["src", "relay-a", "relay-b", "dst"]),
            route(&["src", "relay-a", "dst"]),
        ];

        let selected = select_best_valid_route(&candidates, 4, &policy)
            .expect("a valid route should be selected");
        assert_eq!(selected, route(&["src", "relay-a", "dst"]));
    }

    #[test]
    fn no_valid_route_when_all_candidates_fail_policy_or_loops() {
        let policy = RelayTrustPolicy::deny_relays(["relay-a", "relay-b"]);
        let candidates = vec![
            route(&["src", "relay-a", "dst"]),
            route(&["src", "relay-b", "dst"]),
            route(&["src", "relay-c", "relay-c", "dst"]),
        ];

        let err = select_best_valid_route(&candidates, 4, &policy)
            .expect_err("all routes should be rejected");
        assert_eq!(err, RelayRouteError::NoValidRoute);
    }

    #[test]
    fn forwarder_increments_hop_index_on_success() {
        let mut fwd = RelayForwarder::new(60_000, RelayTrustPolicy::default());
        let env = test_envelope(1, 3, 0);
        let out = fwd.forward(&env, 1_000).unwrap();
        assert_eq!(out.hop_index, 1);
    }

    #[test]
    fn forwarder_enforces_hop_limit() {
        let mut fwd = RelayForwarder::new(60_000, RelayTrustPolicy::default());
        let env = test_envelope(1, 3, 3); // hop_index == hop_limit
        assert!(matches!(
            fwd.forward(&env, 1_000),
            Err(RelayForwardError::HopLimitExceeded { .. })
        ));
    }

    #[test]
    fn forwarder_suppresses_duplicate_nonce() {
        let mut fwd = RelayForwarder::new(60_000, RelayTrustPolicy::default());
        let env = test_envelope(1, 3, 0);
        fwd.forward(&env, 1_000).unwrap();
        assert!(matches!(
            fwd.forward(&env, 1_001),
            Err(RelayForwardError::DuplicateDetected)
        ));
    }

    #[test]
    fn forwarder_rejects_denied_src_peer() {
        let src_hex = hex_peer_id(&[0xaa; 32]);
        let policy = RelayTrustPolicy::deny_relays([src_hex]);
        let mut fwd = RelayForwarder::new(60_000, policy);
        let env = test_envelope(1, 3, 0);
        assert!(matches!(
            fwd.forward(&env, 1_000),
            Err(RelayForwardError::PolicyRejected { .. })
        ));
    }

    #[test]
    fn forwarder_emits_events() {
        let mut fwd = RelayForwarder::new(60_000, RelayTrustPolicy::default());
        let env = test_envelope(42, 3, 0);
        fwd.forward(&env, 1_000).unwrap();
        let events = fwd.drain_events();
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0],
            RelayEvent::Forwarded { session_id: 42, .. }
        ));
        assert!(fwd.drain_events().is_empty());
    }
}
