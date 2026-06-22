//! Shared panel state updated by the connection thread and read by the UI.

use std::collections::VecDeque;

pub use openpulse_daemon::protocol::{DaemonConfig, MessageSummary};

/// Maximum rows kept in the rolling waterfall history.
pub const WATERFALL_ROWS: usize = 64;
/// Maximum samples kept in the ECC-rate history (seconds of 1-Hz Metrics events).
pub const ECC_HISTORY_LEN: usize = 120;

/// Snapshot of a single rig's CAT status (from `RigStatus` events).
#[derive(Debug, Clone, Default)]
pub struct RigSnapshot {
    pub freq_hz: u64,
    pub mode: String,
    pub power_w: Option<f32>,
    pub alc: Option<f32>,
    pub swr: Option<f32>,
}

/// Full panel state shared between the connection thread and the egui update loop.
#[derive(Debug)]
pub struct PanelState {
    /// Whether the TCP connection to `openpulse-server` is currently up.
    pub connected: bool,
    /// Last active modem mode string (e.g. `"BPSK250"`).
    pub mode: String,
    /// Current speed-level label (e.g. `"SL5"`).
    pub speed_level: String,
    /// Numeric speed level (1–20); 0 = unknown.
    pub speed_level_num: u8,
    /// Current HPX FSM state as a display string (e.g. `"ActiveTransfer"`).
    pub hpx_state: String,
    /// AFC frequency offset (Hz) from the last `AfcUpdate` event.
    pub afc_hz: f32,
    /// DCD busy flag from the last `DcdChange` event.
    pub dcd_busy: bool,
    /// DCD RMS energy from the last `DcdChange` event.
    pub dcd_energy: f32,
    /// Effective bit-rate from the last `Metrics` event.
    pub effective_bps: f32,
    /// ECC (FEC) error rate from the last `Metrics` event.
    pub ecc_rate: f32,
    /// Compression ratio from the last `Metrics` event.
    pub compress_ratio: f32,
    /// Signal strength in dBm (from `Metrics` or `RigStatus`).
    pub signal_strength_dbm: Option<i32>,
    /// Daemon-process CPU load, % of all cores (from `SystemMetrics`).
    pub cpu_percent: f32,
    /// Daemon-process resident memory in MiB (from `SystemMetrics`).
    pub ram_mb: f32,
    /// Daemon-process RAM as % of total system memory (from `SystemMetrics`).
    pub ram_percent: f32,
    /// Best-effort system GPU utilisation %, `None` when unavailable (from `SystemMetrics`).
    pub gpu_percent: Option<f32>,
    /// Smoothed modem decode latency in ms (from `SystemMetrics`).
    pub decode_latency_ms: f32,
    /// Latest rig A CAT status.
    pub rig_a: Option<RigSnapshot>,
    /// Latest rig B CAT status.
    pub rig_b: Option<RigSnapshot>,
    /// Rolling event log (newest at front), capped at 100 entries.
    pub event_log: VecDeque<String>,
    /// Token of a pending QSY proposal, if any.
    pub pending_qsy_token: Option<String>,
    /// Most-recent power-spectrum bins (dBFS), 512 values from the daemon.
    pub spectrum_bins: Vec<f32>,
    /// Rolling waterfall history: newest row at index 0, oldest at the back.
    pub spectrum_history: VecDeque<Vec<f32>>,
    /// Monotonically increasing counter bumped each time a spectrum frame arrives.
    /// The UI compares this against its last-seen value to skip redundant texture uploads.
    pub spectrum_generation: u64,
    /// Rolling ECC-rate history (one sample per Metrics event at 1 Hz).
    pub ecc_history: VecDeque<f32>,
    /// Whether the transmitter is currently keyed (PTT asserted).
    pub ptt_active: bool,
    /// Whether an RF peer link is currently active.
    pub rf_connected: bool,
    /// Callsign of the currently connected RF peer, if any.
    pub rf_peer: Option<String>,
    /// Whether the daemon reports repeater runtime enabled.
    pub repeater_enabled: bool,
    /// Most-recent daemon configuration snapshot (from `ConfigData` event).
    pub daemon_config: Option<DaemonConfig>,
    /// Inbox: summaries of all stored messages (from `MessageList` / `MessageReceived` events).
    pub inbox: Vec<MessageSummary>,
    /// Full body of the message currently open in the reader pane, if any.
    pub open_message_body: Option<String>,
    /// ID of the message whose body is loaded in `open_message_body`.
    pub open_message_id: Option<u64>,
}

impl Default for PanelState {
    fn default() -> Self {
        Self {
            connected: false,
            mode: "—".into(),
            speed_level: "—".into(),
            speed_level_num: 0,
            hpx_state: "Idle".into(),
            afc_hz: 0.0,
            dcd_busy: false,
            dcd_energy: 0.0,
            effective_bps: 0.0,
            ecc_rate: 0.0,
            compress_ratio: 1.0,
            signal_strength_dbm: None,
            cpu_percent: 0.0,
            ram_mb: 0.0,
            ram_percent: 0.0,
            gpu_percent: None,
            decode_latency_ms: 0.0,
            rig_a: None,
            rig_b: None,
            event_log: VecDeque::new(),
            pending_qsy_token: None,
            spectrum_bins: Vec::new(),
            spectrum_history: VecDeque::new(),
            spectrum_generation: 0,
            ecc_history: VecDeque::new(),
            ptt_active: false,
            rf_connected: false,
            rf_peer: None,
            repeater_enabled: false,
            daemon_config: None,
            inbox: Vec::new(),
            open_message_body: None,
            open_message_id: None,
        }
    }
}

impl PanelState {
    pub fn push_log(&mut self, entry: String) {
        self.event_log.push_front(entry);
        if self.event_log.len() > 100 {
            self.event_log.pop_back();
        }
    }
}
