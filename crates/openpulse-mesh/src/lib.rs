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
    /// A beacon was sent.
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
    hop_limit: u8,
    events: Vec<MeshEvent>,
}

impl MeshDaemon {
    /// Create a new daemon.
    ///
    /// - `engine` — modem engine (already has plugins registered)
    /// - `mode` — modulation mode string (e.g. `"BPSK250"`)
    /// - `local_peer_id` — this node's 32-byte Ed25519 public key / peer ID
    /// - `max_hops` — relay hop limit enforced by the forwarders
    /// - `beacon_interval_s` — seconds between peer-discovery beacons
    /// - `policy` — relay trust policy applied by [`RelayForwarder`]
    pub fn new(
        engine: ModemEngine,
        mode: impl Into<String>,
        local_peer_id: [u8; 32],
        max_hops: u8,
        beacon_interval_s: u64,
        policy: RelayTrustPolicy,
    ) -> Self {
        let ttl_ms = 60_000;
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

    /// One receive-and-process cycle.
    ///
    /// Drains the modem RX buffer, decodes any [`WireEnvelope`], dispatches it
    /// to the relay or query forwarder, emits a beacon if due, and returns all
    /// collected [`MeshEvent`]s since the last call.
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
                let _ = self.engine.transmit(&bytes, &self.mode, None);
            }
            self.events.push(MeshEvent::BeaconSent { query_id });
        }

        std::mem::take(&mut self.events)
    }

    // ── internal ──────────────────────────────────────────────────────────────

    fn dispatch(&mut self, envelope: WireEnvelope, now_ms: u64) {
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
                        if let Ok(bytes) = forwarded.encode() {
                            let _ = self.engine.transmit(&bytes, &self.mode, None);
                        }
                        self.events.extend(
                            self.relay_forwarder
                                .drain_events()
                                .into_iter()
                                .map(MeshEvent::Relay),
                        );
                    }
                    Err(_) => {
                        // hop limit / duplicate / policy — silently drop
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
                    if let Ok(bytes) = forwarded.encode() {
                        let _ = self.engine.transmit(&bytes, &self.mode, None);
                    }
                    self.events.extend(
                        self.query_forwarder
                            .drain_events()
                            .into_iter()
                            .map(MeshEvent::Query),
                    );
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
