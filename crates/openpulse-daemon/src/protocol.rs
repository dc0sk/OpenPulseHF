//! NDJSON-over-TCP control protocol types.
//!
//! Messages are either JSON objects on a single newline-terminated line, or binary
//! spectrum frames that start with [`SPECTRUM_MAGIC`].  Receivers distinguish the two
//! by inspecting the first byte: `{` (0x7B) → JSON, `O` (0x4F) → binary spectrum frame.
//!
//! Server → client: unsolicited [`ControlEvent`] NDJSON stream, interleaved with binary
//! spectrum frames after [`ControlCommand::SubscribeSpectrum`].
//! Client → server: [`ControlCommand`] request; server replies with [`CommandResponse`].

use openpulse_modem::event::EngineEvent;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Binary spectrum frame codec
// ---------------------------------------------------------------------------

/// Magic header for binary spectrum frames: ASCII "OPSP".
pub const SPECTRUM_MAGIC: &[u8; 4] = b"OPSP";

/// Encode a power-spectrum frame.
///
/// Wire layout: `OPSP` (4 B) | fft_size u16 LE | sample_rate u32 LE | bins f32 LE × fft_size
pub fn encode_spectrum_frame(sample_rate: u32, bins: &[f32]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(4 + 2 + 4 + bins.len() * 4);
    buf.extend_from_slice(SPECTRUM_MAGIC);
    buf.extend_from_slice(&(bins.len() as u16).to_le_bytes());
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    for &b in bins {
        buf.extend_from_slice(&b.to_le_bytes());
    }
    buf
}

/// Decode a power-spectrum frame previously encoded by [`encode_spectrum_frame`].
///
/// Returns `(sample_rate, bins)` on success or an error string if the buffer is
/// malformed (bad magic, truncated, wrong length).
pub fn decode_spectrum_frame(data: &[u8]) -> Result<(u32, Vec<f32>), String> {
    if data.len() < 10 {
        return Err(format!("frame too short: {} bytes", data.len()));
    }
    if &data[0..4] != SPECTRUM_MAGIC {
        return Err(format!("bad magic: {:02X?}", &data[0..4]));
    }
    let fft_size = u16::from_le_bytes([data[4], data[5]]) as usize;
    let sample_rate = u32::from_le_bytes([data[6], data[7], data[8], data[9]]);
    let expected = 10 + fft_size * 4;
    if data.len() < expected {
        return Err(format!(
            "truncated: need {expected} bytes, got {}",
            data.len()
        ));
    }
    let bins = data[10..expected]
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    Ok((sample_rate, bins))
}

// ---------------------------------------------------------------------------
// Message store types
// ---------------------------------------------------------------------------

/// Brief description of a stored message, used in inbox listings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSummary {
    /// Unique monotonic message ID within this daemon session.
    pub id: u64,
    /// Sender callsign.
    pub from: String,
    /// Recipient callsign.
    pub to: String,
    /// Message subject line.
    pub subject: String,
    /// Unix timestamp (seconds) when the message was stored.
    pub timestamp_secs: u64,
}

// ---------------------------------------------------------------------------
// Config snapshot
// ---------------------------------------------------------------------------

/// Snapshot of daemon runtime configuration returned by [`ControlCommand::GetConfig`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    /// Station callsign from config file (read-only at runtime).
    pub callsign: String,
    /// Maidenhead grid square from config file (read-only at runtime).
    pub grid_square: String,
    /// Active modem mode string (e.g. `"BPSK250"`).
    pub mode: String,
    /// TX attenuation in dB (0.0 = no attenuation).
    pub tx_attenuation_db: f32,
    /// Whether the QSY frequency-agility protocol is enabled.
    pub qsy_enabled: bool,
    /// Active bandplan guardrail mode.
    /// `"unrestricted"` disables all frequency checks.
    /// Other valid values: `"ham-iaru-r1"`, `"ham-iaru-r2"`, `"ham-iaru-r3"`.
    pub bandplan_mode: String,
    /// Allow integrated tuner operations when SWR is high.
    #[serde(default)]
    pub allow_tuner_on_high_swr: bool,
}

// ---------------------------------------------------------------------------
// Events and commands
// ---------------------------------------------------------------------------

/// Top-level event pushed from server to every connected client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlEvent {
    /// Modem engine state change (forwarded from the broadcast channel).
    EngineEvent { event: EngineEvent },
    /// Periodic modem metrics snapshot (default 1 Hz).
    Metrics {
        effective_bps: f32,
        /// RS/FEC byte-error correction rate; `None` until wired to engine diagnostics.
        ecc_rate: Option<f32>,
        /// Session compression ratio (compressed / raw); `None` until wired.
        compress_ratio: Option<f32>,
        afc_correction_hz: f32,
        signal_strength_dbm: Option<i32>,
    },
    /// Periodic host-resource snapshot for the OpenPulse daemon process (default 1 Hz).
    SystemMetrics {
        /// Daemon-process CPU load, conventional process % (100 = one core; may exceed 100).
        cpu_percent: f32,
        /// Daemon-process resident memory in MiB.
        ram_mb: f32,
        /// Daemon-process RAM as a % of total system memory (0–100).
        ram_percent: f32,
        /// Best-effort system GPU utilisation (0–100); `None` when no source is available.
        gpu_percent: Option<f32>,
        /// Smoothed modem receive-path decode latency in milliseconds.
        decode_latency_ms: f32,
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
    /// PTT state changed (asserted or released).
    PttChanged { active: bool },
    /// RF peer connection state changed.
    RfConnectionChanged {
        connected: bool,
        peer: Option<String>,
    },
    /// Repeater runtime state changed.
    RepeaterChanged { enabled: bool },
    /// New pending QSY proposal token available for operator decision.
    QsyPending { token: String },
    /// QSY decision recorded by daemon runtime.
    QsyDecision { token: String, accepted: bool },
    /// Remote station initiated a QSY negotiation; received over RF.
    QsyIncoming { token: String, n_candidates: u32 },
    /// Response to [`ControlCommand::GetConfig`].
    ConfigData { config: DaemonConfig },
    /// A message was stored (sent or received); broadcast to all clients.
    MessageReceived {
        id: u64,
        from: String,
        to: String,
        subject: String,
        /// First 120 characters of the body for quick preview.
        preview: String,
        /// Unix timestamp (seconds) when the message was stored.
        timestamp_secs: u64,
    },
    /// Full inbox listing; sent only to the requesting client.
    MessageList { messages: Vec<MessageSummary> },
    /// Full message body; sent only to the requesting client.
    MessageData {
        id: u64,
        from: String,
        to: String,
        subject: String,
        body: String,
    },
    /// Structured command execution failure emitted by daemon runtime handlers.
    CommandError {
        /// Command name in snake_case (e.g. `"send_message"`).
        command: String,
        /// Human-readable failure detail.
        reason: String,
    },
    /// Receiver-led OTA adaptive rate-stepping status (periodic / on change).
    OtaStatus {
        /// Whether an OTA session is active.
        active: bool,
        /// Mode string the local station transmits data at (`None` if no session).
        tx_mode: Option<String>,
        /// Current TX speed level name (e.g. `"SL8"`).
        tx_level: Option<String>,
        /// FEC scheme at the current TX level (e.g. `"none"`, `"ldpc"`).
        tx_fec: String,
        /// Absolute level we recommend to the peer for our RX direction.
        rx_recommended_level: Option<String>,
        /// Highest level we have actually decoded (lockstep anchor).
        rx_confirmed_level: Option<String>,
        /// Whether the session is locked to a fixed level (manual override).
        is_locked: bool,
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
    /// Set the TX attenuation for the current band (dB; 0.0 = no attenuation).
    SetTxAttenuation { db: f32, band: Option<String> },
    /// Begin streaming binary spectrum frames to this client at `fps` frames/second.
    SubscribeSpectrum { fps: u32 },
    /// Assert PTT (key the transmitter).
    PttAssert,
    /// Release PTT (unkey the transmitter).
    PttRelease,
    /// Initiate an RF connection to a peer callsign via the TNC.
    ConnectPeer { callsign: String },
    /// Disconnect the current RF peer connection.
    DisconnectPeer,
    /// Request the daemon's current runtime configuration.
    ///
    /// The server responds with [`ControlEvent::ConfigData`] followed immediately
    /// by an `ok` [`CommandResponse`].
    GetConfig,
    /// Apply runtime configuration changes.  Callsign and grid square are
    /// ignored (read-only at runtime); mode and attenuation take effect immediately.
    SetConfig { config: DaemonConfig },
    /// Queue an outbound message; `from` is filled by the daemon using the
    /// configured callsign.  Broadcasts [`ControlEvent::MessageReceived`] to
    /// all clients and forwards the command to the caller via `mpsc`.
    SendMessage {
        to: String,
        subject: String,
        body: String,
    },
    /// Request the full inbox listing.  Server responds with
    /// [`ControlEvent::MessageList`] followed by an `ok` [`CommandResponse`].
    ListMessages,
    /// Fetch the full body of a single message by ID.  Server responds with
    /// [`ControlEvent::MessageData`] followed by an `ok` [`CommandResponse`],
    /// or an error [`CommandResponse`] if the ID is unknown.
    GetMessage { id: u64 },
    /// Delete a stored message by ID.
    DeleteMessage { id: u64 },
    /// Start a receiver-led OTA adaptive session with the named profile.
    StartOtaSession { profile: String },
    /// Stop the active OTA session.
    StopOtaSession,
    /// Clamp the OTA ladder to `[min, max]` (each `None`/empty = profile bound).
    OtaSetLevelBounds {
        min_level: Option<String>,
        max_level: Option<String>,
    },
    /// Lock OTA to a fixed speed level (manual override).
    OtaLockLevel { level: String },
    /// Release the OTA level lock and resume adapting.
    OtaUnlock,
    /// Tune the rate-adaptation hysteresis (anti-oscillation) gates at runtime.
    /// `min_backlog` (bytes) gates AckUp upgrades on queued TX backlog; `0`
    /// disables. `upgrade_hold_frames` suppresses re-upgrades after a downgrade;
    /// `0` disables. Each `None` leaves the current value unchanged.
    OtaSetHysteresis {
        min_backlog: Option<usize>,
        upgrade_hold_frames: Option<u32>,
    },
    /// Apply an aggressiveness preset (`conservative`/`balanced`/`aggressive`) that
    /// sets the A2/A3 hysteresis gates together — one knob instead of two.
    OtaSetAggressiveness { preset: String },
    /// Set the DCD/squelch RMS threshold at runtime (e.g. to clear a band's noise
    /// floor). Holds until the next retune re-applies the per-band/default value.
    SetDcdSquelch { threshold: f32 },
    /// Enable/disable CE-SSB TX envelope conditioning (master switch). Only acts on
    /// high-PAPR multicarrier modes; a no-op for single-carrier modes regardless.
    SetCessb { enabled: bool },
    /// Enable/disable the receiver-side automatic notch (removes out-of-band CW interference
    /// before demod; the protected band tracks the active mode so the signal is never notched).
    SetNotch { enabled: bool },
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

#[cfg(test)]
mod ota_protocol_tests {
    use super::*;

    #[test]
    fn ota_commands_round_trip_via_json() {
        let cmds = vec![
            ControlCommand::StartOtaSession {
                profile: "hpx_modcod".into(),
            },
            ControlCommand::StopOtaSession,
            ControlCommand::OtaSetLevelBounds {
                min_level: Some("SL3".into()),
                max_level: Some("SL10".into()),
            },
            ControlCommand::OtaLockLevel {
                level: "SL6".into(),
            },
            ControlCommand::OtaUnlock,
            ControlCommand::OtaSetHysteresis {
                min_backlog: Some(128),
                upgrade_hold_frames: Some(3),
            },
            ControlCommand::OtaSetAggressiveness {
                preset: "aggressive".into(),
            },
            ControlCommand::SetDcdSquelch { threshold: 0.05 },
            ControlCommand::SetCessb { enabled: false },
        ];
        for c in cmds {
            let json = serde_json::to_string(&c).unwrap();
            let back: ControlCommand = serde_json::from_str(&json).unwrap();
            assert_eq!(format!("{c:?}"), format!("{back:?}"));
        }
    }

    #[test]
    fn ota_status_event_round_trips_and_tags_snake_case() {
        let ev = ControlEvent::OtaStatus {
            active: true,
            tx_mode: Some("QPSK500".into()),
            tx_level: Some("SL6".into()),
            tx_fec: "ldpc".into(),
            rx_recommended_level: Some("SL7".into()),
            rx_confirmed_level: Some("SL6".into()),
            is_locked: false,
        };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains("\"type\":\"ota_status\""), "tag: {json}");
        let back: ControlEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(format!("{ev:?}"), format!("{back:?}"));
    }
}
