//! NDJSON-over-TCP control protocol types.
//!
//! All messages are JSON objects serialised on a single line (newline-terminated).
//! Server → client: unsolicited [`ControlEvent`] stream.
//! Client → server: [`ControlCommand`] request; server replies with [`CommandResponse`].

use openpulse_modem::event::EngineEvent;
use serde::{Deserialize, Serialize};

/// Top-level event pushed from server to every connected client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlEvent {
    /// Modem engine state change (forwarded from the broadcast channel).
    EngineEvent { event: EngineEvent },
    /// Periodic modem metrics snapshot (default 1 Hz).
    Metrics {
        effective_bps: f32,
        ecc_rate: f32,
        compress_ratio: f32,
        afc_correction_hz: f32,
        signal_strength_dbm: Option<i32>,
    },
    /// Periodic rig CAT status snapshot (default 2 Hz, only when rig configured).
    RigStatus {
        rig: String,
        freq_hz: u64,
        mode: String,
        power_w: Option<f32>,
        alc: Option<f32>,
        swr: Option<f32>,
    },
}

/// Command sent from a client to the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum ControlCommand {
    /// Switch the modem to a different mode string (e.g. `"BPSK250"`).
    SetMode { mode: String },
    /// Command the rig's CAT interface to change frequency.
    SetFreq { rig: String, freq_hz: u64 },
    /// Accept a pending QSY proposal identified by `token`.
    AcceptQsy { token: String },
    /// Reject a pending QSY proposal identified by `token`.
    RejectQsy { token: String },
    /// Enable the cross-band repeater.
    EnableRepeater,
    /// Disable the cross-band repeater.
    DisableRepeater,
}

/// Per-command response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl CommandResponse {
    pub fn ok() -> Self {
        Self {
            ok: true,
            error: None,
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: Some(msg.into()),
        }
    }
}
