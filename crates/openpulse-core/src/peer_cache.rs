use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrustLevel {
    Unknown,
    Reduced,
    PskVerified,
    Verified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerRecord {
    pub peer_id: String,
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
        route_quality: u8,
        trust_level: TrustLevel,
        revision: u64,
        updated_at_ms: u64,
    ) -> PeerRecord {
        PeerRecord {
            peer_id: peer_id.to_string(),
            route_quality,
            trust_level,
            revision,
            updated_at_ms,
        }
    }

    #[test]
    fn evicts_expired_entries_by_ttl() {
        let mut cache = PeerCache::new(16, 500);
        assert!(cache.upsert(rec("peer-a", 90, TrustLevel::Verified, 1, 1_000), 1_000));
        assert!(cache.upsert(rec("peer-b", 80, TrustLevel::Reduced, 1, 1_200), 1_200));

        let evicted = cache.evict_expired(1_701);
        assert_eq!(evicted, 2);
        assert!(cache.is_empty());
    }

    #[test]
    fn evicts_least_recently_used_when_over_capacity() {
        let mut cache = PeerCache::new(2, 10_000);
        assert!(cache.upsert(rec("peer-a", 60, TrustLevel::Unknown, 1, 1_000), 1_000));
        assert!(cache.upsert(rec("peer-b", 70, TrustLevel::Unknown, 1, 1_000), 1_000));

        let _ = cache.get("peer-a", 1_100);

        assert!(cache.upsert(rec("peer-c", 90, TrustLevel::Reduced, 1, 1_200), 1_200));

        assert!(cache.get("peer-a", 1_200).is_some());
        assert!(cache.get("peer-c", 1_200).is_some());
        assert!(cache.get("peer-b", 1_200).is_none());
    }

    #[test]
    fn rejects_stale_conflict_update() {
        let mut cache = PeerCache::new(8, 10_000);
        assert!(cache.upsert(rec("peer-a", 50, TrustLevel::Reduced, 4, 2_000), 2_000));

        let replaced = cache.upsert(rec("peer-a", 95, TrustLevel::Verified, 3, 2_500), 2_500);
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
        assert!(cache.upsert(rec("peer-z", 40, TrustLevel::Reduced, 7, 4_000), 4_000));

        assert!(cache.upsert(rec("peer-z", 30, TrustLevel::Verified, 7, 4_000), 4_000));
        let current = cache
            .get("peer-z", 4_100)
            .expect("peer-z should exist")
            .clone();
        assert_eq!(current.trust_level, TrustLevel::Verified);
        assert_eq!(current.route_quality, 30);

        assert!(cache.upsert(rec("peer-z", 75, TrustLevel::Verified, 7, 4_000), 4_000));
        let current = cache
            .get("peer-z", 4_200)
            .expect("peer-z should exist")
            .clone();
        assert_eq!(current.route_quality, 75);
        assert_eq!(current.trust_level, TrustLevel::Verified);
    }
}
