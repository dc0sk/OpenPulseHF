use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrustLevel {
    Unknown,
    Reduced,
    PskVerified,
    Verified,
}

/// Filter criterion applied to `PeerCache::query` for the trust dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustFilter {
    /// Only return peers with `PskVerified` or `Verified` trust.
    TrustedOnly,
    /// Return peers with any trust level except `Reduced`.
    TrustedOrUnknown,
    /// No trust filter — return all peers regardless of trust level.
    Any,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerRecord {
    pub peer_id: String,
    pub capability_mask: u32,
    pub route_quality: u8,
    pub trust_level: TrustLevel,
    pub revision: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone)]
pub struct PeerCache {
    capacity: usize,
    ttl_ms: u64,
    entries: HashMap<String, PeerRecord>,
    lru: VecDeque<String>,
}

impl PeerCache {
    pub fn new(capacity: usize, ttl_ms: u64) -> Self {
        Self {
            capacity,
            ttl_ms,
            entries: HashMap::new(),
            lru: VecDeque::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn upsert(&mut self, incoming: PeerRecord, now_ms: u64) -> bool {
        self.evict_expired(now_ms);

        match self.entries.get(&incoming.peer_id) {
            Some(existing) if !Self::should_replace(existing, &incoming) => false,
            _ => {
                let peer_id = incoming.peer_id.clone();
                self.entries.insert(peer_id.clone(), incoming);
                self.touch_lru(&peer_id);
                self.evict_over_capacity();
                true
            }
        }
    }

    pub fn get(&mut self, peer_id: &str, now_ms: u64) -> Option<&PeerRecord> {
        self.evict_expired(now_ms);
        if self.entries.contains_key(peer_id) {
            self.touch_lru(peer_id);
        }
        self.entries.get(peer_id)
    }

    /// Read-only lookup that does not update LRU or evict expired entries.
    pub fn peek(&self, peer_id: &str) -> Option<&PeerRecord> {
        self.entries.get(peer_id)
    }

    /// Return up to `max_results` live peers matching the given filters.
    ///
    /// `capability_mask`: if non-zero, a peer must have all bits set.
    /// `min_quality`: route_quality must be >= this value.
    /// `trust_filter`: controls which trust levels are accepted.
    pub fn query(
        &mut self,
        capability_mask: u32,
        min_quality: u8,
        trust_filter: TrustFilter,
        max_results: usize,
        now_ms: u64,
    ) -> Vec<PeerRecord> {
        self.evict_expired(now_ms);

        let mut results: Vec<PeerRecord> = self
            .entries
            .values()
            .filter(|r| {
                if capability_mask != 0 && (r.capability_mask & capability_mask) != capability_mask
                {
                    return false;
                }
                if r.route_quality < min_quality {
                    return false;
                }
                match trust_filter {
                    TrustFilter::TrustedOnly => {
                        matches!(
                            r.trust_level,
                            TrustLevel::PskVerified | TrustLevel::Verified
                        )
                    }
                    TrustFilter::TrustedOrUnknown => !matches!(r.trust_level, TrustLevel::Reduced),
                    TrustFilter::Any => true,
                }
            })
            .cloned()
            .collect();

        results.sort_by(|a, b| b.route_quality.cmp(&a.route_quality));
        results.truncate(max_results);
        results
    }

    pub fn evict_expired(&mut self, now_ms: u64) -> usize {
        let before = self.entries.len();
        self.entries
            .retain(|_, record| now_ms.saturating_sub(record.updated_at_ms) <= self.ttl_ms);
        self.lru
            .retain(|peer_id| self.entries.contains_key(peer_id.as_str()));
        before.saturating_sub(self.entries.len())
    }

    fn should_replace(existing: &PeerRecord, incoming: &PeerRecord) -> bool {
        if incoming.revision > existing.revision {
            return true;
        }
        if incoming.revision < existing.revision {
            return false;
        }

        if incoming.updated_at_ms > existing.updated_at_ms {
            return true;
        }
        if incoming.updated_at_ms < existing.updated_at_ms {
            return false;
        }

        if incoming.trust_level > existing.trust_level {
            return true;
        }
        if incoming.trust_level < existing.trust_level {
            return false;
        }

        incoming.route_quality >= existing.route_quality
    }

    fn evict_over_capacity(&mut self) {
        while self.entries.len() > self.capacity {
            let Some(oldest) = self.lru.pop_front() else {
                break;
            };
            self.entries.remove(oldest.as_str());
        }
    }

    fn touch_lru(&mut self, peer_id: &str) {
        self.lru.retain(|id| id != peer_id);
        self.lru.push_back(peer_id.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(
        peer_id: &str,
        capability_mask: u32,
        route_quality: u8,
        trust_level: TrustLevel,
        revision: u64,
        updated_at_ms: u64,
    ) -> PeerRecord {
        PeerRecord {
            peer_id: peer_id.to_string(),
            capability_mask,
            route_quality,
            trust_level,
            revision,
            updated_at_ms,
        }
    }

    #[test]
    fn evicts_expired_entries_by_ttl() {
        let mut cache = PeerCache::new(16, 500);
        assert!(cache.upsert(rec("peer-a", 0, 90, TrustLevel::Verified, 1, 1_000), 1_000));
        assert!(cache.upsert(rec("peer-b", 0, 80, TrustLevel::Reduced, 1, 1_200), 1_200));

        let evicted = cache.evict_expired(1_701);
        assert_eq!(evicted, 2);
        assert!(cache.is_empty());
    }

    #[test]
    fn evicts_least_recently_used_when_over_capacity() {
        let mut cache = PeerCache::new(2, 10_000);
        assert!(cache.upsert(rec("peer-a", 0, 60, TrustLevel::Unknown, 1, 1_000), 1_000));
        assert!(cache.upsert(rec("peer-b", 0, 70, TrustLevel::Unknown, 1, 1_000), 1_000));

        let _ = cache.get("peer-a", 1_100);

        assert!(cache.upsert(rec("peer-c", 0, 90, TrustLevel::Reduced, 1, 1_200), 1_200));

        assert!(cache.get("peer-a", 1_200).is_some());
        assert!(cache.get("peer-c", 1_200).is_some());
        assert!(cache.get("peer-b", 1_200).is_none());
    }

    #[test]
    fn rejects_stale_conflict_update() {
        let mut cache = PeerCache::new(8, 10_000);
        assert!(cache.upsert(rec("peer-a", 0, 50, TrustLevel::Reduced, 4, 2_000), 2_000));

        let replaced = cache.upsert(rec("peer-a", 0, 95, TrustLevel::Verified, 3, 2_500), 2_500);
        assert!(!replaced);

        let current = cache
            .get("peer-a", 2_500)
            .expect("peer-a should remain in cache")
            .clone();
        assert_eq!(current.revision, 4);
        assert_eq!(current.trust_level, TrustLevel::Reduced);
        assert_eq!(current.route_quality, 50);
    }

    #[test]
    fn resolves_same_revision_conflict_by_trust_then_quality() {
        let mut cache = PeerCache::new(8, 10_000);
        assert!(cache.upsert(rec("peer-z", 0, 40, TrustLevel::Reduced, 7, 4_000), 4_000));

        assert!(cache.upsert(rec("peer-z", 0, 30, TrustLevel::Verified, 7, 4_000), 4_000));
        let current = cache
            .get("peer-z", 4_100)
            .expect("peer-z should exist")
            .clone();
        assert_eq!(current.trust_level, TrustLevel::Verified);
        assert_eq!(current.route_quality, 30);

        assert!(cache.upsert(rec("peer-z", 0, 75, TrustLevel::Verified, 7, 4_000), 4_000));
        let current = cache
            .get("peer-z", 4_200)
            .expect("peer-z should exist")
            .clone();
        assert_eq!(current.route_quality, 75);
        assert_eq!(current.trust_level, TrustLevel::Verified);
    }

    #[test]
    fn query_filters_by_capability_mask() {
        let mut cache = PeerCache::new(16, 10_000);
        cache.upsert(
            rec("peer-a", 0b0011, 80, TrustLevel::Verified, 1, 1_000),
            1_000,
        );
        cache.upsert(
            rec("peer-b", 0b0001, 90, TrustLevel::Verified, 1, 1_000),
            1_000,
        );
        cache.upsert(
            rec("peer-c", 0b0110, 70, TrustLevel::Verified, 1, 1_000),
            1_000,
        );

        let results = cache.query(0b0011, 0, TrustFilter::Any, 10, 1_000);
        let ids: Vec<&str> = results.iter().map(|r| r.peer_id.as_str()).collect();
        assert!(ids.contains(&"peer-a"));
        assert!(!ids.contains(&"peer-b"));
        assert!(!ids.contains(&"peer-c"));
    }

    #[test]
    fn query_filters_by_trust_level() {
        let mut cache = PeerCache::new(16, 10_000);
        cache.upsert(rec("peer-a", 0, 80, TrustLevel::Verified, 1, 1_000), 1_000);
        cache.upsert(rec("peer-b", 0, 80, TrustLevel::Unknown, 1, 1_000), 1_000);
        cache.upsert(rec("peer-c", 0, 80, TrustLevel::Reduced, 1, 1_000), 1_000);

        let trusted = cache.query(0, 0, TrustFilter::TrustedOnly, 10, 1_000);
        assert_eq!(trusted.len(), 1);
        assert_eq!(trusted[0].peer_id, "peer-a");

        let not_reduced = cache.query(0, 0, TrustFilter::TrustedOrUnknown, 10, 1_000);
        assert_eq!(not_reduced.len(), 2);

        let any = cache.query(0, 0, TrustFilter::Any, 10, 1_000);
        assert_eq!(any.len(), 3);
    }

    #[test]
    fn query_results_sorted_by_quality_descending() {
        let mut cache = PeerCache::new(16, 10_000);
        cache.upsert(rec("peer-a", 0, 50, TrustLevel::Verified, 1, 1_000), 1_000);
        cache.upsert(rec("peer-b", 0, 90, TrustLevel::Verified, 1, 1_000), 1_000);
        cache.upsert(rec("peer-c", 0, 70, TrustLevel::Verified, 1, 1_000), 1_000);

        let results = cache.query(0, 0, TrustFilter::Any, 10, 1_000);
        assert_eq!(results[0].route_quality, 90);
        assert_eq!(results[1].route_quality, 70);
        assert_eq!(results[2].route_quality, 50);
    }

    #[test]
    fn query_respects_max_results() {
        let mut cache = PeerCache::new(16, 10_000);
        for i in 0..5u8 {
            cache.upsert(
                rec(
                    &format!("peer-{i}"),
                    0,
                    i * 10,
                    TrustLevel::Verified,
                    1,
                    1_000,
                ),
                1_000,
            );
        }
        let results = cache.query(0, 0, TrustFilter::Any, 3, 1_000);
        assert_eq!(results.len(), 3);
    }
}
