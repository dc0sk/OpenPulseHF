//! Peer discovery beacon scheduler.

use openpulse_core::wire_query::{PeerQueryRequest, WireEnvelope, WireMsgType};

/// Schedules periodic peer-discovery beacons.
pub struct BeaconScheduler {
    interval_ms: u64,
    last_sent_ms: u64,
    next_query_id: u64,
}

impl BeaconScheduler {
    pub fn new(interval_s: u64) -> Self {
        Self {
            interval_ms: interval_s * 1000,
            last_sent_ms: 0,
            next_query_id: 1,
        }
    }

    /// Returns `true` if a beacon is due at `now_ms`.
    pub fn is_due(&self, now_ms: u64) -> bool {
        now_ms >= self.last_sent_ms + self.interval_ms
    }

    /// Build the next beacon envelope and advance the scheduler.
    pub fn next_beacon(
        &mut self,
        now_ms: u64,
        local_peer_id: [u8; 32],
        hop_limit: u8,
    ) -> (WireEnvelope, u64) {
        let query_id = self.next_query_id;
        self.next_query_id += 1;
        self.last_sent_ms = now_ms;

        let req = PeerQueryRequest {
            query_id,
            capability_mask: 0,
            min_link_quality: 0,
            trust_filter: 0x02, // Any
            max_results: 16,
        };

        let envelope = WireEnvelope {
            msg_type: WireMsgType::PeerQueryRequest,
            flags: 0,
            session_id: query_id,
            src_peer_id: local_peer_id,
            dst_peer_id: [0u8; 32], // broadcast
            nonce: nonce_from(query_id),
            timestamp_ms: now_ms,
            hop_limit,
            hop_index: 0,
            payload: req.encode(),
            auth_tag: [0u8; 16],
        };

        (envelope, query_id)
    }
}

fn nonce_from(id: u64) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[..8].copy_from_slice(&id.to_le_bytes());
    n
}
