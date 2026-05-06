//! `openpulse-mesh` — HPX relay mesh daemon library.
//!
//! Exposes [`MeshDaemon`] as a testable unit; the binary (`src/main.rs`) wraps it
//! with config loading and a run loop.

pub mod beacon;

use openpulse_core::query_propagation::{QueryEvent, QueryForwarder};
use openpulse_core::relay::{RelayEvent, RelayForwarder, RelayTrustPolicy};
use openpulse_core::wire_query::{WireEnvelope, WireMsgType};
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
    /// - `policy` — relay trust policy (deny-list of peer IDs)
    pub fn new(
        engine: ModemEngine,
        mode: impl Into<String>,
        local_peer_id: [u8; 32],
        max_hops: u8,
        beacon_interval_s: u64,
        ttl_ms: u64,
        policy: RelayTrustPolicy,
    ) -> Self {
        Self {
            engine,
            mode: mode.into(),
            local_peer_id,
            relay_forwarder: RelayForwarder::new(ttl_ms, policy.clone()),
            query_forwarder: QueryForwarder::new(ttl_ms, 1024, policy),
            beacon: BeaconScheduler::new(beacon_interval_s),
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
                            // Drop relay events — frame was not actually forwarded.
                        }
                    }
                    Err(_) => {
                        // hop limit / duplicate / policy — collect diagnostic events.
                        self.events.extend(
                            self.relay_forwarder
                                .drain_events()
                                .into_iter()
                                .map(MeshEvent::Relay),
                        );
                    }
                }
            }
            // Query / route messages: propagate to all neighbours.
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
}
