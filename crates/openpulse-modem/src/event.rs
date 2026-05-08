//! Real-time engine event types for the broadcast subscriber API.

use openpulse_core::hpx::{HpxEvent, HpxState};
use openpulse_core::rate::{RateEvent, RateTrigger, SpeedLevel};
use serde::{Deserialize, Serialize};

/// Rate-change direction for bidirectional sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateDirection {
    /// Our outgoing TX path adapted.
    Tx,
    /// Our incoming RX path adapted (from peer's reverse_ack report).
    Rx,
}

/// A discrete event emitted by [`ModemEngine`](crate::ModemEngine) at every
/// significant state change.
///
/// Subscribers receive these via [`ModemEngine::subscribe`](crate::ModemEngine::subscribe)
/// and can serialize them as NDJSON for piping or TUI consumption.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EngineEvent {
    /// AFC frequency offset estimate updated after a receive call.
    AfcUpdate {
        /// Residual frequency error measured at the corrected reference (Hz).
        /// The total offset from the nominal centre frequency is approximately
        /// `correction_hz + offset_hz`.
        offset_hz: f32,
        /// Accumulated carrier correction that will be applied to subsequent
        /// demodulation calls (Hz).  Defaults to 0.0 when deserialising older
        /// event streams that predate this field.
        #[serde(default)]
        correction_hz: f32,
        mode: String,
    },
    /// Rate adapter advanced after an ACK was applied.
    RateChange {
        event: RateEvent,
        speed_level: SpeedLevel,
        mode: String,
        /// Which direction adapted.  `None` in sessions without bidirectional
        /// tracking (legacy compatibility).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        direction: Option<RateDirection>,
        /// What triggered the rate change.  `None` for ACK-only sessions.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        trigger: Option<RateTrigger>,
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
