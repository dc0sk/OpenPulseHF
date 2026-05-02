use std::collections::{HashMap, HashSet, VecDeque};

use crate::relay::RelayTrustPolicy;
use crate::wire_query::{PeerQueryRequest, WireEnvelope, WireMsgType};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct QueryKey {
    src_peer_id: String,
    query_id: u64,
}

#[derive(Debug, Clone)]
pub struct QueryPropagationTracker {
    ttl_ms: u64,
    capacity: usize,
    seen_queries: HashMap<QueryKey, u64>,
    query_lru: VecDeque<QueryKey>,
    response_seen: HashMap<u64, HashSet<String>>,
}

impl QueryPropagationTracker {
    pub fn new(ttl_ms: u64, capacity: usize) -> Self {
        Self {
            ttl_ms,
            capacity,
            seen_queries: HashMap::new(),
            query_lru: VecDeque::new(),
            response_seen: HashMap::new(),
        }
    }

    pub fn should_forward_query(&mut self, src_peer_id: &str, query_id: u64, now_ms: u64) -> bool {
        self.evict_expired(now_ms);

        let key = QueryKey {
            src_peer_id: src_peer_id.to_string(),
            query_id,
        };

        if self.seen_queries.contains_key(&key) {
            self.touch_query_lru(&key);
            return false;
        }

        self.seen_queries.insert(key.clone(), now_ms);
        self.touch_query_lru(&key);
        self.evict_over_capacity();
        true
    }

    pub fn should_accept_response(
        &mut self,
        query_id: u64,
        responder_peer_id: &str,
        now_ms: u64,
    ) -> bool {
        self.evict_expired(now_ms);

        let responders = self.response_seen.entry(query_id).or_default();
        if responders.contains(responder_peer_id) {
            return false;
        }

        responders.insert(responder_peer_id.to_string());
        true
    }

    pub fn evict_expired(&mut self, now_ms: u64) {
        self.seen_queries
            .retain(|_, first_seen_ms| now_ms.saturating_sub(*first_seen_ms) <= self.ttl_ms);

        self.query_lru
            .retain(|key| self.seen_queries.contains_key(key));

        self.response_seen
            .retain(|query_id, _| self.seen_queries.keys().any(|k| k.query_id == *query_id));
    }

    fn evict_over_capacity(&mut self) {
        while self.seen_queries.len() > self.capacity {
            let Some(oldest) = self.query_lru.pop_front() else {
                break;
            };

            self.seen_queries.remove(&oldest);
            if !self
                .seen_queries
                .keys()
                .any(|k| k.query_id == oldest.query_id)
            {
                self.response_seen.remove(&oldest.query_id);
            }
        }
    }

    fn touch_query_lru(&mut self, key: &QueryKey) {
        self.query_lru.retain(|k| k != key);
        self.query_lru.push_back(key.clone());
    }
}

// ------------------------------------------------------------------
// QueryForwarder
// ------------------------------------------------------------------

/// Errors returned by `QueryForwarder::propagate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryForwardError {
    HopLimitExceeded { hop_index: u8, hop_limit: u8 },
    DuplicateDetected,
    PolicyRejected { src_peer_id: [u8; 32] },
    MsgTypeNotQuery { got: u8 },
}

/// Events emitted by `QueryForwarder` for structured logging.
#[derive(Debug, Clone)]
pub enum QueryEvent {
    Propagated {
        query_id: u64,
        hop_index_out: u8,
        hop_limit: u8,
    },
    HopLimitReached {
        query_id: u64,
        hop_index: u8,
    },
    DuplicateSuppressed {
        query_id: u64,
    },
    PolicyRejected {
        query_id: u64,
        src_peer_id: [u8; 32],
    },
}

/// Stateful query propagation node with hop limiting, duplicate suppression,
/// and trust-policy enforcement.
///
/// Mirrors `RelayForwarder` but operates on `PeerQueryRequest` envelopes
/// (msg_type 0x01).  On success the returned envelope has `hop_index`
/// incremented by one, ready to be broadcast to neighbouring peers.
#[derive(Debug, Clone)]
pub struct QueryForwarder {
    tracker: QueryPropagationTracker,
    policy: RelayTrustPolicy,
    events: Vec<QueryEvent>,
}

impl QueryForwarder {
    pub fn new(ttl_ms: u64, capacity: usize, policy: RelayTrustPolicy) -> Self {
        Self {
            tracker: QueryPropagationTracker::new(ttl_ms, capacity),
            policy,
            events: Vec::new(),
        }
    }

    /// Process one `PeerQueryRequest` envelope for network propagation.
    ///
    /// Returns the modified envelope (hop_index incremented) on success.
    /// The caller is responsible for broadcasting it to neighbouring peers.
    pub fn propagate(
        &mut self,
        envelope: &WireEnvelope,
        now_ms: u64,
    ) -> Result<WireEnvelope, QueryForwardError> {
        // 1. Must be a peer query request
        if envelope.msg_type != WireMsgType::PeerQueryRequest {
            return Err(QueryForwardError::MsgTypeNotQuery {
                got: envelope.msg_type as u8,
            });
        }

        let query_id = PeerQueryRequest::decode(&envelope.payload)
            .map(|r| r.query_id)
            .unwrap_or(0);

        // 2. Hop-limit enforcement
        if envelope.hop_index >= envelope.hop_limit {
            self.events.push(QueryEvent::HopLimitReached {
                query_id,
                hop_index: envelope.hop_index,
            });
            return Err(QueryForwardError::HopLimitExceeded {
                hop_index: envelope.hop_index,
                hop_limit: envelope.hop_limit,
            });
        }

        // 3. Trust policy — check originating peer
        let src_hex = hex_peer_id(&envelope.src_peer_id);
        if !self.policy.allows(&src_hex) {
            self.events.push(QueryEvent::PolicyRejected {
                query_id,
                src_peer_id: envelope.src_peer_id,
            });
            return Err(QueryForwardError::PolicyRejected {
                src_peer_id: envelope.src_peer_id,
            });
        }

        // 4. Duplicate suppression
        if !self
            .tracker
            .should_forward_query(&src_hex, query_id, now_ms)
        {
            self.events
                .push(QueryEvent::DuplicateSuppressed { query_id });
            return Err(QueryForwardError::DuplicateDetected);
        }

        let mut out = envelope.clone();
        out.hop_index += 1;
        self.events.push(QueryEvent::Propagated {
            query_id,
            hop_index_out: out.hop_index,
            hop_limit: out.hop_limit,
        });
        Ok(out)
    }

    /// Drain and return all buffered query events since the last call.
    pub fn drain_events(&mut self) -> Vec<QueryEvent> {
        std::mem::take(&mut self.events)
    }
}

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

    #[test]
    fn suppresses_duplicate_queries_within_ttl() {
        let mut tracker = QueryPropagationTracker::new(2_000, 64);

        assert!(tracker.should_forward_query("peer-a", 100, 1_000));
        assert!(!tracker.should_forward_query("peer-a", 100, 1_500));
    }

    #[test]
    fn allows_query_again_after_ttl_expiry() {
        let mut tracker = QueryPropagationTracker::new(2_000, 64);

        assert!(tracker.should_forward_query("peer-a", 200, 1_000));
        assert!(!tracker.should_forward_query("peer-a", 200, 2_500));

        tracker.evict_expired(3_001);
        assert!(tracker.should_forward_query("peer-a", 200, 3_001));
    }

    #[test]
    fn suppresses_duplicate_responses_per_query_per_responder() {
        let mut tracker = QueryPropagationTracker::new(2_000, 64);
        assert!(tracker.should_forward_query("peer-a", 300, 1_000));

        assert!(tracker.should_accept_response(300, "relay-1", 1_100));
        assert!(!tracker.should_accept_response(300, "relay-1", 1_200));
        assert!(tracker.should_accept_response(300, "relay-2", 1_300));
    }

    #[test]
    fn evicts_oldest_queries_when_capacity_is_exceeded() {
        let mut tracker = QueryPropagationTracker::new(10_000, 2);

        assert!(tracker.should_forward_query("peer-a", 1, 1_000));
        assert!(tracker.should_forward_query("peer-a", 2, 1_001));
        assert!(tracker.should_forward_query("peer-a", 3, 1_002));

        assert!(tracker.should_forward_query("peer-a", 1, 1_100));
    }
}
