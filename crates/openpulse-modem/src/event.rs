//! Real-time engine event types for the broadcast subscriber API.

use openpulse_core::hpx::{HpxEvent, HpxState};
use openpulse_core::rate::{RateEvent, SpeedLevel};
use serde::{Deserialize, Serialize};

/// A discrete event emitted by [`ModemEngine`](crate::ModemEngine) at every
/// significant state change.
///
/// Subscribers receive these via [`ModemEngine::subscribe`](crate::ModemEngine::subscribe)
/// and can serialize them as NDJSON for piping or TUI consumption.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EngineEvent {
    /// AFC frequency offset estimate updated after a receive call.
    AfcUpdate { offset_hz: f32, mode: String },
    /// Rate adapter advanced after an ACK was applied.
    RateChange {
        event: RateEvent,
        speed_level: SpeedLevel,
        mode: String,
    },
    /// DCD channel-busy status changed.
    DcdChange { busy: bool, energy: f32 },
    /// HPX session state machine transitioned.
    HpxTransition {
        from: HpxState,
        to: HpxState,
        event: HpxEvent,
        session_id: Option<String>,
    },
    /// A frame was successfully transmitted.
    FrameTransmitted { mode: String, bytes: usize },
    /// A frame was successfully received and decoded.
    FrameReceived { mode: String, bytes: usize },
    /// A secure HPX session started.
    SessionStarted {
        session_id: Option<String>,
        peer_modes: String,
    },
    /// A secure HPX session ended.
    SessionEnded {
        session_id: Option<String>,
        reason: String,
    },
}
