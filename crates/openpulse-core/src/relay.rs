use std::collections::{HashMap, HashSet};

use crate::peer_cache::{PeerCache, TrustFilter, TrustLevel};
use crate::wire_query::WireEnvelope;

// ------------------------------------------------------------------
// Errors
// ------------------------------------------------------------------

/// Errors returned by route validation functions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayRouteError {
    /// Route slice is empty; at least one hop is required.
    EmptyRoute,
    /// A peer_id appears more than once in the route (routing loop).
    LoopDetected { peer_id: String },
    /// Route length exceeds the configured `max_hops` limit.
    TooManyHops { hops: usize, max_hops: usize },
    /// An intermediate relay's peer_id was rejected by the trust policy.
    TrustPolicyRejected { peer_id: String },
    /// No candidate route passed policy and loop checks.
    NoValidRoute,
}

/// Errors returned by `RelayForwarder::try_forward`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayForwardError {
    /// hop_index has reached hop_limit; message must be dropped.
    HopLimitExceeded { hop_index: u8, hop_limit: u8 },
    /// (session_id, nonce) already forwarded; duplicate suppressed.
    DuplicateDetected,
    /// src_peer_id is denied by this node's trust policy.
    PolicyRejected { src_peer_id: [u8; 32] },
    /// Replay-suppression table is at capacity; envelope dropped.
    CapacityExceeded,
    /// The envelope's origin signature did not verify against `src_peer_id`.
    AuthenticationFailed { src_peer_id: [u8; 32] },
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
    /// Envelope dropped because the replay-suppression table is full.
    CapacityExceeded { session_id: u64 },
    /// Envelope dropped because its origin signature failed to verify.
    AuthenticationFailed {
        session_id: u64,
        src_peer_id: [u8; 32],
    },
}

// ------------------------------------------------------------------
// Trust policy
// ------------------------------------------------------------------

/// Trust policy applied at relay nodes to filter which originators may be forwarded.
///
/// Two operator controls, both keyed on the (hex) originator peer id: a deny-list (block these) and
/// an optional allow-list (`Some` = forward *only* these; `None` = forward anyone not denied). These
/// operate on the envelope's `src_peer_id`, which — when `require_authentication` is set (the default)
/// — is cryptographically authenticated at the relay via the envelope's Ed25519 origin signature
/// (`src_peer_id` is the originator's verifying key; see [`WireEnvelope::verify_origin`]). A spoofed
/// `src_peer_id` cannot pass this gate. The deny/allow lists remain operator scoping controls layered
/// on top of that authentication (E3, closing the handshake-trust audit finding E1's `auth_tag` half).
#[derive(Debug, Clone)]
pub struct RelayTrustPolicy {
    denied_relays: HashSet<String>,
    /// When `Some`, only these (hex) originator peer ids are forwarded; `None` allows all non-denied.
    allowed_relays: Option<HashSet<String>>,
    /// Minimum trust level required to relay a frame.
    pub min_trust_filter: TrustFilter,
    /// When `true` (default), the relay verifies each envelope's origin signature against
    /// `src_peer_id` and drops it on failure. Disable only for synthetic (non-keyed) test peers.
    pub require_authentication: bool,
}

impl Default for RelayTrustPolicy {
    fn default() -> Self {
        Self {
            denied_relays: HashSet::new(),
            allowed_relays: None,
            min_trust_filter: TrustFilter::default(),
            require_authentication: true,
        }
    }
}

impl RelayTrustPolicy {
    /// Construct a policy that blocks the listed peer IDs; all others are allowed.
    pub fn deny_relays<I, S>(denied: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            denied_relays: denied.into_iter().map(Into::into).collect(),
            ..Self::default()
        }
    }

    /// Construct a policy with both a deny-list and a minimum trust level.
    pub fn with_trust_filter<I, S>(denied: I, min_trust_filter: TrustFilter) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            denied_relays: denied.into_iter().map(Into::into).collect(),
            min_trust_filter,
            ..Self::default()
        }
    }

    /// Enable or disable origin-signature verification at the relay. Enabled by default; disable only
    /// for tests using synthetic peer ids that are not real Ed25519 verifying keys.
    pub fn set_require_authentication(&mut self, require: bool) {
        self.require_authentication = require;
    }

    /// Restrict forwarding to an explicit allow-list of (hex) originator peer ids, on top of any
    /// deny-list. An empty iterator leaves the allow-list unset (forward anyone not denied) rather
    /// than blocking everything — an empty allow-list config means "no restriction", not "deny all".
    pub fn set_allow_list<I, S>(&mut self, allowed: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let set: HashSet<String> = allowed.into_iter().map(Into::into).collect();
        self.allowed_relays = if set.is_empty() { None } else { Some(set) };
    }

    /// Return `true` if `relay_peer_id` may be forwarded: not on the deny-list, and (if an allow-list
    /// is set) on the allow-list.
    pub fn allows(&self, relay_peer_id: &str) -> bool {
        !self.denied_relays.contains(relay_peer_id)
            && self
                .allowed_relays
                .as_ref()
                .is_none_or(|a| a.contains(relay_peer_id))
    }
}

// ------------------------------------------------------------------
// Route validation (existing API, unchanged)
// ------------------------------------------------------------------

/// Validate that a route has no duplicate peer IDs and does not exceed `max_hops`.
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

/// Validate a route for loops, hop count, and intermediate-relay trust policy.
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

/// Select the shortest valid route from `candidates` that passes loop and policy checks.
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

/// Hard cap on the replay-suppression table size.
///
/// A sender that rotates nonces faster than `ttl_ms` expires can otherwise
/// grow the table without bound.  New envelopes are dropped when the cap is
/// reached until TTL eviction frees space.
const MAX_SEEN_ENTRIES: usize = 4096;

/// Stateful relay forwarding node with duplicate suppression and hop limiting.
///
/// Each received `WireEnvelope` is checked for:
/// 1. hop_index < hop_limit — drops the packet if the hop budget is exhausted.
/// 2. origin authentication — verifies the Ed25519 signature against `src_peer_id`
///    (when `require_authentication`), rejecting forged or unsigned frames.
/// 3. (session_id, nonce) uniqueness — suppresses replays.
/// 4. src_peer_id trust policy — rejects envelopes from denied originators.
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
    /// Create a new forwarder with the given replay-suppression TTL and trust policy.
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

        // 2. Origin authentication — verify the Ed25519 signature against src_peer_id before
        //    spending any dedup-table capacity on a forged frame.
        if self.policy.require_authentication && envelope.verify_origin().is_err() {
            self.events.push(RelayEvent::AuthenticationFailed {
                session_id: envelope.session_id,
                src_peer_id: envelope.src_peer_id,
            });
            return Err(RelayForwardError::AuthenticationFailed {
                src_peer_id: envelope.src_peer_id,
            });
        }

        // 3. Capacity guard — reject before inserting when the table is full.
        if self.seen.len() >= MAX_SEEN_ENTRIES {
            self.events.push(RelayEvent::CapacityExceeded {
                session_id: envelope.session_id,
            });
            return Err(RelayForwardError::CapacityExceeded);
        }

        // 4. Duplicate suppression
        let key = (envelope.session_id, envelope.nonce);
        if self.seen.contains_key(&key) {
            self.events.push(RelayEvent::DuplicateSuppressed {
                session_id: envelope.session_id,
                nonce: envelope.nonce,
            });
            return Err(RelayForwardError::DuplicateDetected);
        }

        // 5. Trust policy — check originating peer (src_peer_id)
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
    use ed25519_dalek::SigningKey;

    /// Fixed originator seed for test envelopes; `src_peer_id` is its verifying key.
    const TEST_SEED: [u8; 32] = [0x42; 32];

    fn route(peers: &[&str]) -> Vec<String> {
        peers.iter().map(|v| v.to_string()).collect()
    }

    /// Hex of the test originator's peer id (the verifying key of `TEST_SEED`).
    fn test_src_hex() -> String {
        hex_peer_id(
            &SigningKey::from_bytes(&TEST_SEED)
                .verifying_key()
                .to_bytes(),
        )
    }

    fn test_envelope(session_id: u64, hop_limit: u8, hop_index: u8) -> WireEnvelope {
        let src = SigningKey::from_bytes(&TEST_SEED)
            .verifying_key()
            .to_bytes();
        let mut env = WireEnvelope {
            msg_type: WireMsgType::RelayDataChunk,
            flags: 0,
            session_id,
            src_peer_id: src,
            dst_peer_id: [0xbb; 32],
            nonce: [0x11; 12],
            timestamp_ms: 1_000,
            hop_limit,
            hop_index,
            payload: vec![],
            signature: None,
        };
        env.sign(&TEST_SEED).unwrap();
        env
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
        let policy = RelayTrustPolicy::deny_relays([test_src_hex()]);
        let mut fwd = RelayForwarder::new(60_000, policy);
        let env = test_envelope(1, 3, 0);
        assert!(matches!(
            fwd.forward(&env, 1_000),
            Err(RelayForwardError::PolicyRejected { .. })
        ));
    }

    #[test]
    fn forwarder_allows_non_denied_peer_when_deny_list_active() {
        // deny a different peer id; the test envelope's signed src is not denied — should forward
        let denied_hex = hex_peer_id(&[0xbb; 32]);
        let policy = RelayTrustPolicy::deny_relays([denied_hex]);
        let mut fwd = RelayForwarder::new(60_000, policy);
        let env = test_envelope(1, 3, 0);
        assert!(fwd.forward(&env, 1_000).is_ok());
    }

    #[test]
    fn forwarder_rejects_forged_signature() {
        let mut fwd = RelayForwarder::new(60_000, RelayTrustPolicy::default());
        let mut env = test_envelope(1, 3, 0);
        env.payload = b"tampered".to_vec(); // invalidates the origin signature
        assert!(matches!(
            fwd.forward(&env, 1_000),
            Err(RelayForwardError::AuthenticationFailed { .. })
        ));
    }

    #[test]
    fn forwarder_rejects_unsigned_when_authentication_required() {
        let mut fwd = RelayForwarder::new(60_000, RelayTrustPolicy::default());
        let mut env = test_envelope(1, 3, 0);
        env.signature = None; // strip the signature
        assert!(matches!(
            fwd.forward(&env, 1_000),
            Err(RelayForwardError::AuthenticationFailed { .. })
        ));
    }

    #[test]
    fn forwarder_rejects_spoofed_src_peer_id() {
        let mut fwd = RelayForwarder::new(60_000, RelayTrustPolicy::default());
        let mut env = test_envelope(1, 3, 0);
        // Claim a different (valid) originator without holding its key.
        env.src_peer_id = SigningKey::from_bytes(&[0x99; 32])
            .verifying_key()
            .to_bytes();
        assert!(matches!(
            fwd.forward(&env, 1_000),
            Err(RelayForwardError::AuthenticationFailed { .. })
        ));
    }

    #[test]
    fn forwarder_allows_unsigned_when_authentication_disabled() {
        let mut policy = RelayTrustPolicy::default();
        policy.set_require_authentication(false);
        let mut fwd = RelayForwarder::new(60_000, policy);
        let mut env = test_envelope(1, 3, 0);
        env.signature = None;
        assert!(fwd.forward(&env, 1_000).is_ok());
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
