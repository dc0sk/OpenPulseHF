use std::collections::{HashMap, HashSet, VecDeque};

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
