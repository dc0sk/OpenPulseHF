//! `openpulse-mesh` — HPX relay mesh daemon library.
//!
//! Exposes [`MeshDaemon`] as a testable unit; the binary (`src/main.rs`) wraps it
//! with config loading and a run loop.

pub mod beacon;

use openpulse_core::peer_cache::{PeerCache, PeerRecord, TrustFilter, TrustLevel};
use openpulse_core::query_propagation::{QueryEvent, QueryForwarder};
use openpulse_core::relay::{RelayEvent, RelayForwarder, RelayTrustPolicy};
use openpulse_core::wire_query::{
    PeerQueryRequest, PeerQueryResponse, PeerQueryResult, WireEnvelope, WireMsgType,
};
use openpulse_modem::ModemEngine;
use thiserror::Error;

use crate::beacon::BeaconScheduler;

// ── Errors ────────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum MeshError {
    #[error("modem transmit error: {0}")]
    Transmit(#[from] openpulse_core::error::ModemError),
    #[error("wire encode error: {0}")]
    Encode(#[from] openpulse_core::wire_query::WireQueryError),
}

// ── Events ────────────────────────────────────────────────────────────────────

/// Observability events emitted by [`MeshDaemon::step`].
#[derive(Debug)]
pub enum MeshEvent {
    /// A relay data frame was forwarded to the next hop.
    Relay(RelayEvent),
    /// A query frame was propagated.
    Query(QueryEvent),
    /// A beacon was sent (encode + transmit both succeeded).
    BeaconSent { query_id: u64 },
    /// A relay data frame addressed to this node was received.
    FrameDelivered { session_id: u64 },
    /// A peer-query request was answered with `result_count` cache entries.
    PeerQueried { query_id: u64, result_count: usize },
    /// A previously unknown peer was added to the local peer cache.
    PeerDiscovered { peer_id: [u8; 32] },
}

// ── MeshDaemon ────────────────────────────────────────────────────────────────

/// Stateful relay mesh node.
///
/// Wraps a [`ModemEngine`] and drives [`RelayForwarder`] / [`QueryForwarder`]
/// based on the type of each received [`WireEnvelope`].
pub struct MeshDaemon {
    engine: ModemEngine,
    mode: String,
    local_peer_id: [u8; 32],
    relay_forwarder: RelayForwarder,
    query_forwarder: QueryForwarder,
    beacon: BeaconScheduler,
    peer_cache: PeerCache,
    /// Local maximum hop count. Envelopes with hop_limit > this are clamped
    /// before being passed to the forwarders, preventing senders from bypassing
    /// the node's configured relay policy.
    hop_limit: u8,
    events: Vec<MeshEvent>,
}

impl MeshDaemon {
    /// Create a new daemon.
    ///
    /// - `engine` — modem engine (already has plugins registered)
    /// - `mode` — modulation mode string (e.g. `"BPSK250"`)
    /// - `local_peer_id` — this node's 32-byte Ed25519 public key / peer ID
    /// - `max_hops` — relay hop limit enforced locally; any received envelope
    ///   with a higher `hop_limit` is clamped to this value before forwarding
    /// - `beacon_interval_s` — seconds between peer-discovery beacons; 0 disables
    /// - `ttl_ms` — store-and-forward frame TTL (also used as peer cache TTL)
    /// - `policy` — relay trust policy (deny-list of peer IDs)
    /// - `peer_cache_capacity` — maximum entries in the local peer cache
    /// - `peer_cache_ttl_ms` — peer cache entry TTL in milliseconds
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        engine: ModemEngine,
        mode: impl Into<String>,
        local_peer_id: [u8; 32],
        max_hops: u8,
        beacon_interval_s: u64,
        ttl_ms: u64,
        policy: RelayTrustPolicy,
        peer_cache_capacity: usize,
        peer_cache_ttl_ms: u64,
    ) -> Self {
        let mut peer_cache = PeerCache::new(peer_cache_capacity, peer_cache_ttl_ms);
        // Seed cache with self so we always include ourselves in query responses.
        peer_cache.upsert(
            PeerRecord {
                peer_id: peer_id_hex(&local_peer_id),
                capability_mask: 0,
                route_quality: 255,
                trust_level: TrustLevel::Verified,
                revision: 0,
                updated_at_ms: 0,
            },
            0,
        );

        Self {
            engine,
            mode: mode.into(),
            local_peer_id,
            relay_forwarder: RelayForwarder::new(ttl_ms, policy.clone()),
            query_forwarder: QueryForwarder::new(ttl_ms, 1024, policy),
            beacon: BeaconScheduler::new(beacon_interval_s),
            peer_cache,
            hop_limit: max_hops,
            events: Vec::new(),
        }
    }

    /// Transmit a relay envelope (called by the originating node).
    pub fn send_relay(&mut self, envelope: WireEnvelope) -> Result<(), MeshError> {
        let bytes = envelope.encode()?;
        self.engine.transmit(&bytes, &self.mode, None)?;
        Ok(())
    }

    /// Number of peers currently in the local cache (including self).
    pub fn peer_cache_len(&self) -> usize {
        self.peer_cache.len()
    }

    /// One receive/process cycle.
    ///
    /// Attempts a single receive call on the modem engine.  If a frame is decoded
    /// successfully it is dispatched to the relay or query forwarder.  A beacon is
    /// emitted if one is due.  Returns all [`MeshEvent`]s collected since the last call.
    pub fn step(&mut self, now_ms: u64) -> Vec<MeshEvent> {
        if let Ok(bytes) = self.engine.receive(&self.mode, None) {
            if !bytes.is_empty() {
                if let Ok(envelope) = WireEnvelope::decode(&bytes) {
                    self.dispatch(envelope, now_ms);
                }
            }
        }

        if self.beacon.is_due(now_ms) {
            let (beacon_env, query_id) =
                self.beacon
                    .next_beacon(now_ms, self.local_peer_id, self.hop_limit);
            if let Ok(bytes) = beacon_env.encode() {
                if self.engine.transmit(&bytes, &self.mode, None).is_ok() {
                    self.events.push(MeshEvent::BeaconSent { query_id });
                } else {
                    tracing::warn!(query_id, "beacon transmit failed");
                }
            }
        }

        std::mem::take(&mut self.events)
    }

    // ── internal ──────────────────────────────────────────────────────────────

    fn dispatch(&mut self, mut envelope: WireEnvelope, now_ms: u64) {
        // Clamp the envelope's hop_limit to this node's configured maximum so
        // senders cannot bypass local relay policy by advertising a larger limit.
        if envelope.hop_limit > self.hop_limit {
            envelope.hop_limit = self.hop_limit;
        }

        match envelope.msg_type {
            // Relay data / ack: deliver if we are the destination, else forward.
            WireMsgType::RelayDataChunk | WireMsgType::RelayHopAck => {
                if envelope.dst_peer_id == self.local_peer_id {
                    self.events.push(MeshEvent::FrameDelivered {
                        session_id: envelope.session_id,
                    });
                    return;
                }
                match self.relay_forwarder.forward(&envelope, now_ms) {
                    Ok(forwarded) => {
                        let tx_ok = forwarded
                            .encode()
                            .ok()
                            .and_then(|b| self.engine.transmit(&b, &self.mode, None).ok())
                            .is_some();
                        let relay_events = self.relay_forwarder.drain_events();
                        if tx_ok {
                            self.events
                                .extend(relay_events.into_iter().map(MeshEvent::Relay));
                        } else {
                            tracing::warn!(
                                session_id = envelope.session_id,
                                "relay forward: encode/transmit failed"
                            );
                        }
                    }
                    Err(_) => {
                        self.events.extend(
                            self.relay_forwarder
                                .drain_events()
                                .into_iter()
                                .map(MeshEvent::Relay),
                        );
                    }
                }
            }

            // Peer discovery request: answer from local cache, then propagate.
            WireMsgType::PeerQueryRequest => {
                self.handle_peer_query_request(&envelope, now_ms);
            }

            // Peer discovery response: cache results; responses are broadcast so
            // no re-propagation is needed (all nodes already heard it).
            WireMsgType::PeerQueryResponse => {
                self.handle_peer_query_response(&envelope, now_ms);
            }

            // All other query / route messages: propagate to neighbours.
            _ => match self.query_forwarder.propagate(&envelope, now_ms) {
                Ok(forwarded) => {
                    let tx_ok = forwarded
                        .encode()
                        .ok()
                        .and_then(|b| self.engine.transmit(&b, &self.mode, None).ok())
                        .is_some();
                    let query_events = self.query_forwarder.drain_events();
                    if tx_ok {
                        self.events
                            .extend(query_events.into_iter().map(MeshEvent::Query));
                    } else {
                        tracing::warn!(
                            session_id = envelope.session_id,
                            "query propagate: encode/transmit failed"
                        );
                    }
                }
                Err(_) => {
                    self.events.extend(
                        self.query_forwarder
                            .drain_events()
                            .into_iter()
                            .map(MeshEvent::Query),
                    );
                }
            },
        }
    }

    /// Answer a peer-query request from our local cache, then propagate the request.
    fn handle_peer_query_request(&mut self, envelope: &WireEnvelope, now_ms: u64) {
        let Ok(req) = PeerQueryRequest::decode(&envelope.payload) else {
            return;
        };

        let trust_filter = wire_trust_filter(req.trust_filter);
        let min_quality = req.min_link_quality.min(255) as u8;
        let max_results = req.max_results as usize;

        let records = self.peer_cache.query(
            req.capability_mask,
            min_quality,
            trust_filter,
            max_results,
            now_ms,
        );

        let result_count = records.len();
        let results: Vec<PeerQueryResult> = records
            .into_iter()
            .filter_map(|r| {
                let peer_id_bytes = parse_peer_id_hex(&r.peer_id)?;
                Some(PeerQueryResult {
                    peer_id: peer_id_bytes,
                    // TODO: store callsign_hash and descriptor_signature in cache
                    // once PeerDescriptor signing is implemented.
                    callsign_hash: [0u8; 32],
                    capability_mask: r.capability_mask,
                    last_seen_ms: r.updated_at_ms,
                    trust_state: trust_level_to_wire(r.trust_level),
                    descriptor_signature: vec![],
                })
            })
            .collect();

        let resp = PeerQueryResponse {
            query_id: req.query_id,
            results,
        };

        if let Ok(payload) = resp.encode() {
            let resp_env = WireEnvelope {
                msg_type: WireMsgType::PeerQueryResponse,
                flags: 0,
                session_id: req.query_id,
                src_peer_id: self.local_peer_id,
                dst_peer_id: [0u8; 32], // broadcast so all nodes can update caches
                nonce: nonce_from_id(req.query_id),
                timestamp_ms: now_ms,
                hop_limit: self.hop_limit,
                hop_index: 0,
                payload,
                auth_tag: [0u8; 16],
            };
            if let Ok(bytes) = resp_env.encode() {
                if self.engine.transmit(&bytes, &self.mode, None).is_ok() {
                    self.events.push(MeshEvent::PeerQueried {
                        query_id: req.query_id,
                        result_count,
                    });
                } else {
                    tracing::warn!(
                        query_id = req.query_id,
                        "peer query response transmit failed"
                    );
                }
            }
        }

        // Propagate the request so multi-hop peers can also respond.
        match self.query_forwarder.propagate(envelope, now_ms) {
            Ok(forwarded) => {
                let tx_ok = forwarded
                    .encode()
                    .ok()
                    .and_then(|b| self.engine.transmit(&b, &self.mode, None).ok())
                    .is_some();
                let query_events = self.query_forwarder.drain_events();
                if tx_ok {
                    self.events
                        .extend(query_events.into_iter().map(MeshEvent::Query));
                }
            }
            Err(_) => {
                self.events.extend(
                    self.query_forwarder
                        .drain_events()
                        .into_iter()
                        .map(MeshEvent::Query),
                );
            }
        }
    }

    /// Cache peer records from an incoming peer-query response.
    fn handle_peer_query_response(&mut self, envelope: &WireEnvelope, now_ms: u64) {
        let Ok(resp) = PeerQueryResponse::decode(&envelope.payload) else {
            return;
        };

        // route_quality decreases with distance; hop_index=0 means direct neighbour.
        let route_quality = (255u16 / (envelope.hop_index as u16 + 1)) as u8;

        for result in &resp.results {
            // Skip our own entry — we already have it seeded with quality=255.
            if result.peer_id == self.local_peer_id {
                continue;
            }
            let peer_id_str = peer_id_hex(&result.peer_id);
            let is_new = self.peer_cache.peek(&peer_id_str).is_none();

            let record = PeerRecord {
                peer_id: peer_id_str,
                capability_mask: result.capability_mask,
                route_quality,
                trust_level: wire_trust_level(result.trust_state),
                revision: 0,
                updated_at_ms: now_ms,
            };
            self.peer_cache.upsert(record, now_ms);

            if is_new {
                self.events.push(MeshEvent::PeerDiscovered {
                    peer_id: result.peer_id,
                });
            }
        }
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn peer_id_hex(bytes: &[u8; 32]) -> String {
    bytes.iter().fold(String::with_capacity(64), |mut s, b| {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn parse_peer_id_hex(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        let hi = hex_nibble(chunk[0])?;
        let lo = hex_nibble(chunk[1])?;
        out[i] = (hi << 4) | lo;
    }
    Some(out)
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn wire_trust_filter(code: u8) -> TrustFilter {
    match code {
        0x00 => TrustFilter::TrustedOnly,
        0x01 => TrustFilter::TrustedOrUnknown,
        _ => TrustFilter::Any,
    }
}

fn wire_trust_level(code: u8) -> TrustLevel {
    match code {
        0x00 => TrustLevel::Verified,
        0x01 => TrustLevel::Unknown,
        _ => TrustLevel::Reduced,
    }
}

fn trust_level_to_wire(level: TrustLevel) -> u8 {
    match level {
        TrustLevel::Verified | TrustLevel::PskVerified => 0x00,
        TrustLevel::Unknown => 0x01,
        TrustLevel::Reduced => 0x02,
    }
}

fn nonce_from_id(id: u64) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[..8].copy_from_slice(&id.to_le_bytes());
    n
}
