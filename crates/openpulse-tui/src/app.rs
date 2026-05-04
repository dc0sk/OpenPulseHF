//! TUI application state updated from the engine event stream.

use std::collections::VecDeque;

use openpulse_core::hpx::HpxState;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::EngineEvent;

/// Live state displayed by the TUI.
pub struct App {
    pub hpx_state: HpxState,
    pub speed_level: Option<SpeedLevel>,
    pub current_mode: Option<String>,
    pub dcd_busy: bool,
    pub dcd_energy: f32,
    pub afc_offset_hz: Option<f32>,
    /// Last 50 transitions, each formatted as "[HH:MM:SS] From → To (Event)".
    pub transitions: VecDeque<String>,
    pub paused: bool,
    pub scroll_offset: usize,
    /// Set when the background worker exits with a fatal error.
    pub fatal_error: Option<String>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            hpx_state: HpxState::Idle,
            speed_level: None,
            current_mode: None,
            dcd_busy: false,
            dcd_energy: 0.0,
            afc_offset_hz: None,
            transitions: VecDeque::new(),
            paused: false,
            scroll_offset: 0,
            fatal_error: None,
        }
    }
}

impl App {
    pub fn apply_event(&mut self, event: EngineEvent) {
        if self.paused {
            return;
        }
        match event {
            EngineEvent::AfcUpdate { offset_hz, .. } => {
                self.afc_offset_hz = Some(offset_hz);
            }
            EngineEvent::RateChange {
                speed_level, mode, ..
            } => {
                self.speed_level = Some(speed_level);
                self.current_mode = Some(mode);
            }
            EngineEvent::DcdChange { busy, energy } => {
                self.dcd_busy = busy;
                self.dcd_energy = energy;
            }
            EngineEvent::HpxTransition {
                from,
                to,
                event,
                session_id,
            } => {
                self.hpx_state = to;
                let ts = chrono_hms();
                let sid = session_id
                    .as_deref()
                    .map(|s| format!(" [{s}]"))
                    .unwrap_or_default();
                let entry = format!("[{ts}] {from:?} → {to:?} ({event:?}){sid}");
                self.transitions.push_back(entry);
                if self.transitions.len() > 50 {
                    self.transitions.pop_front();
                }
            }
            EngineEvent::FrameTransmitted { mode, .. } => {
                self.current_mode = Some(mode);
            }
            EngineEvent::FrameReceived { mode, .. } => {
                self.current_mode = Some(mode);
            }
            EngineEvent::SessionStarted { .. } | EngineEvent::SessionEnded { .. } => {}
        }
    }
}

fn chrono_hms() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let h = (secs / 3600) % 24;
    let m = (secs / 60) % 60;
    let s = secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}
