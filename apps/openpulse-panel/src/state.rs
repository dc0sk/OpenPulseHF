//! Shared panel state updated by the connection thread and read by the UI.

use std::collections::VecDeque;

pub use openpulse_daemon::protocol::DaemonConfig;

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
    /// Whether the transmitter is currently keyed (PTT asserted).
    pub ptt_active: bool,
    /// Whether an RF peer link is currently active.
    pub rf_connected: bool,
    /// Callsign of the currently connected RF peer, if any.
    pub rf_peer: Option<String>,
    /// Most-recent daemon configuration snapshot (from `ConfigData` event).
    pub daemon_config: Option<DaemonConfig>,
}

impl Default for PanelState {
    fn default() -> Self {
        Self {
            connected: false,
            mode: "—".into(),
            speed_level: "—".into(),
            afc_hz: 0.0,
            dcd_busy: false,
            dcd_energy: 0.0,
            effective_bps: 0.0,
            ecc_rate: 0.0,
            compress_ratio: 1.0,
            signal_strength_dbm: None,
            rig_a: None,
            rig_b: None,
            event_log: VecDeque::new(),
            pending_qsy_token: None,
            spectrum_bins: Vec::new(),
            ptt_active: false,
            rf_connected: false,
            rf_peer: None,
            daemon_config: None,
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
