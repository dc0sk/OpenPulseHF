//! Real-time engine event types for the broadcast subscriber API.

use openpulse_core::hpx::{HpxEvent, HpxState};
use openpulse_core::ota_rate::RateDecision;
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
    /// The receiver-led OTA rate controller made a decision about a received data frame.
    ///
    /// Emitted on **every** decision, including ones that change nothing — a failed decode that
    /// leaves the level where it was is precisely the case the periodic `OtaStatus` snapshot
    /// cannot show, because that snapshot reports state and the state did not move. Without this
    /// the controller's failure path is entirely dark.
    OtaRateDecision {
        /// Recommendation before this frame.
        from: SpeedLevel,
        /// Recommendation after this frame. Equal to `from` on a hold.
        to: SpeedLevel,
        /// Whether the frame demodulated. `None` means no candidate mode decoded.
        decoded_level: Option<SpeedLevel>,
        /// Measured SNR fed to the decision (dB), or `None` when the controller acted on **no
        /// reading** — the normal case on a failed decode since #1142, where the burst's frame
        /// position is exactly what the failed decode could not establish. This is the reading the
        /// controller acted on, which is not recoverable from any other event; `None` is itself
        /// information, and must not be flattened to a number.
        snr_db: Option<f32>,
        /// Which branch fired — the field that makes a transition attributable.
        decision: RateDecision,
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
