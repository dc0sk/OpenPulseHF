//! TUI application state updated from the engine event stream.

use std::collections::VecDeque;

use openpulse_core::hpx::{HpxEvent, HpxState};
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::EngineEvent;

/// Coarse trend direction across the retained speed-level history window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeedTrend {
    Up,
    Down,
    Flat,
}

/// Live state displayed by the TUI.
pub struct App {
    pub hpx_state: HpxState,
    pub speed_level: Option<SpeedLevel>,
    pub current_mode: Option<String>,
    pub dcd_busy: bool,
    pub dcd_energy: f32,
    pub afc_offset_hz: Option<f32>,
    pub afc_correction_hz: Option<f32>,
    /// Last N speed levels for trend display.
    pub speed_history: VecDeque<SpeedLevel>,
    /// Session-level successful transfer count.
    pub transfer_ok: u32,
    /// Session-level transfer error count.
    pub transfer_error: u32,
    /// Last 50 transitions, each formatted as "[HH:MM:SS] From → To (Event)".
    pub transitions: VecDeque<String>,
    pub paused: bool,
    pub scroll_offset: usize,
    /// Set when the background worker exits with a fatal error.
    pub fatal_error: Option<String>,
    /// QSY frequency-agility enabled flag (editable; restart required).
    pub qsy_enabled: bool,
    /// Active bandplan mode string (editable; restart required).
    pub bandplan_mode: String,
    /// Allow integrated tuner operation when SWR is high (editable; restart required).
    pub allow_tuner_on_high_swr: bool,
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
            afc_correction_hz: None,
            speed_history: VecDeque::new(),
            transfer_ok: 0,
            transfer_error: 0,
            transitions: VecDeque::new(),
            paused: false,
            scroll_offset: 0,
            fatal_error: None,
            qsy_enabled: false,
            bandplan_mode: "unrestricted".to_string(),
            allow_tuner_on_high_swr: false,
        }
    }
}

impl App {
    pub fn apply_event(&mut self, event: EngineEvent) {
        if self.paused {
            return;
        }
        match event {
            EngineEvent::AfcUpdate {
                offset_hz,
                correction_hz,
                ..
            } => {
                self.afc_offset_hz = Some(offset_hz);
                self.afc_correction_hz = Some(correction_hz);
            }
            EngineEvent::RateChange {
                speed_level, mode, ..
            } => {
                self.speed_level = Some(speed_level);
                self.current_mode = Some(mode);
                self.speed_history.push_back(speed_level);
                if self.speed_history.len() > 8 {
                    self.speed_history.pop_front();
                }
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
                if event == HpxEvent::TransferComplete && from == HpxState::ActiveTransfer {
                    self.transfer_ok = self.transfer_ok.saturating_add(1);
                }
                if event == HpxEvent::TransferError
                    && matches!(from, HpxState::ActiveTransfer | HpxState::RelayActive)
                {
                    self.transfer_error = self.transfer_error.saturating_add(1);
                }
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

    /// Returns FER as a percentage when at least one transfer outcome is known.
    pub fn fer_percent(&self) -> Option<f32> {
        let total = self.transfer_ok.saturating_add(self.transfer_error);
        if total == 0 {
            return None;
        }
        Some((self.transfer_error as f32 / total as f32) * 100.0)
    }

    /// Returns trend direction across the retained speed history window.
    pub fn speed_trend(&self) -> Option<SpeedTrend> {
        let first = self.speed_history.front()?;
        let last = self.speed_history.back()?;
        let first = *first as u8;
        let last = *last as u8;
        if last > first {
            Some(SpeedTrend::Up)
        } else if last < first {
            Some(SpeedTrend::Down)
        } else {
            Some(SpeedTrend::Flat)
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

#[cfg(test)]
mod tests {
    use super::*;
    use openpulse_core::rate::RateEvent;

    fn rate_change(level: SpeedLevel) -> EngineEvent {
        EngineEvent::RateChange {
            event: RateEvent::Maintained,
            speed_level: level,
            mode: "BPSK31".to_string(),
            direction: None,
            trigger: None,
        }
    }

    fn transition(from: HpxState, to: HpxState, event: HpxEvent) -> EngineEvent {
        EngineEvent::HpxTransition {
            from,
            to,
            event,
            session_id: None,
        }
    }

    #[test]
    fn fer_percent_counts_only_transfer_outcomes() {
        let mut app = App::default();
        app.apply_event(transition(
            HpxState::Training,
            HpxState::ActiveTransfer,
            HpxEvent::TrainingOk,
        ));
        app.apply_event(transition(
            HpxState::ActiveTransfer,
            HpxState::Teardown,
            HpxEvent::TransferComplete,
        ));
        app.apply_event(transition(
            HpxState::RelayActive,
            HpxState::Recovery,
            HpxEvent::TransferError,
        ));

        assert_eq!(app.transfer_ok, 1);
        assert_eq!(app.transfer_error, 1);
        assert_eq!(app.fer_percent(), Some(50.0));
    }

    #[test]
    fn speed_trend_follows_history_window_direction() {
        let mut app = App::default();
        app.apply_event(rate_change(SpeedLevel::Sl2));
        app.apply_event(rate_change(SpeedLevel::Sl4));
        assert_eq!(app.speed_trend(), Some(SpeedTrend::Up));

        let mut app = App::default();
        app.apply_event(rate_change(SpeedLevel::Sl5));
        app.apply_event(rate_change(SpeedLevel::Sl3));
        assert_eq!(app.speed_trend(), Some(SpeedTrend::Down));

        let mut app = App::default();
        app.apply_event(rate_change(SpeedLevel::Sl3));
        app.apply_event(rate_change(SpeedLevel::Sl3));
        assert_eq!(app.speed_trend(), Some(SpeedTrend::Flat));
    }
}
