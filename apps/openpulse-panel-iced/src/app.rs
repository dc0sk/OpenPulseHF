//! Application state, update loop, connection lifecycle, and theme wiring for the iced operator
//! panel (REQ-UX-04).
//!
//! The panel connects to the daemon control port (reusing the egui panel's transport/connection/
//! state core), reads a shared `PanelState` updated by the background thread, and sends
//! `ControlCommand`s. The view renders the fixed vertical stack — spectrum → waterfall → ladder →
//! additional info → controls — with selectable Dark/Light/Contrast/System themes.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossbeam_channel::Sender;
use iced::{Subscription, Task, Theme};
use openpulse_daemon::protocol::ControlCommand;

use crate::connection::{self, TransportKind};
use crate::state::PanelState;
use crate::theme::{role_rgb, shade_rgb, ColorRole, EffectiveTheme, Shade, ThemeMode};
use crate::ui;

/// Speed-level rungs shown on the ladder (SL1..=SLN).
pub const LADDER_RUNGS: u8 = 20;

/// Top-level panel application state.
pub struct App {
    // --- theme ---
    pub theme_mode: ThemeMode,
    pub system_is_dark: bool,

    // --- daemon connection ---
    pub shared: Arc<Mutex<PanelState>>,
    pub cmd_tx: Option<Sender<ControlCommand>>,
    pub stop_tx: Option<Sender<()>>,
    /// Control-port address (TCP host:port).
    pub addr: String,

    // --- UI-local input state (not part of PanelState) ---
    pub mode_sel: String,
    pub freq_khz: String,
    pub peer_call: String,
    pub ota_profile: String,
    pub tx_atten_db: f32,
    pub squelch: f32,
    /// Optimistic local mirrors for toggles the daemon doesn't echo in PanelState.
    pub cessb_on: bool,
    pub notch_on: bool,
    pub agc_on: bool,
    pub logbook_on: bool,

    pub tick: u32,
}

/// UI messages.
#[derive(Debug, Clone)]
pub enum Message {
    ToggleTheme,
    Tick,
    AddrChanged(String),
    ConnectToggle,
    Ptt,
    ModeSelected(String),
    FreqChanged(String),
    TuneFreq,
    PeerCallChanged(String),
    ConnectPeer,
    DisconnectPeer,
    ToggleRepeater,
    ToggleCessb,
    ToggleNotch,
    ToggleAgc,
    ToggleLogbook,
    AttenChanged(f32),
    SquelchChanged(f32),
    AcceptQsy,
    RejectQsy,
    OtaProfileChanged(String),
    StartOta,
    StopOta,
    OtaLockToggle,
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let app = App {
            theme_mode: ThemeMode::default(),
            system_is_dark: crate::ui::detect_system_dark(),
            shared: Arc::new(Mutex::new(PanelState::default())),
            cmd_tx: None,
            stop_tx: None,
            addr: "127.0.0.1:9000".into(),
            mode_sel: "BPSK250".into(),
            freq_khz: "14100.000".into(),
            peer_call: String::new(),
            ota_profile: "hpx_hf".into(),
            tx_atten_db: 0.0,
            squelch: 0.02,
            cessb_on: false,
            notch_on: false,
            agc_on: false,
            logbook_on: false,
            tick: 0,
        };
        (app, Task::none())
    }

    /// Whether a connection worker is currently spawned.
    pub fn is_connected(&self) -> bool {
        self.cmd_tx.is_some()
    }

    fn send(&self, cmd: ControlCommand) {
        if let Some(tx) = &self.cmd_tx {
            let _ = tx.try_send(cmd);
        }
    }

    fn connect(&mut self) {
        let (stop_tx, stop_rx) = crossbeam_channel::bounded(1);
        let cmd_tx = connection::spawn(
            self.addr.clone(),
            TransportKind::Tcp,
            self.shared.clone(),
            stop_rx,
        );
        self.stop_tx = Some(stop_tx);
        self.cmd_tx = Some(cmd_tx);
    }

    fn disconnect(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        self.cmd_tx = None;
        if let Ok(mut st) = self.shared.lock() {
            st.connected = false;
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ToggleTheme => self.theme_mode = self.theme_mode.next(),
            Message::Tick => self.tick = self.tick.wrapping_add(1),
            Message::AddrChanged(a) => self.addr = a,
            Message::ConnectToggle => {
                if self.is_connected() {
                    self.disconnect();
                } else {
                    self.connect();
                }
            }
            Message::Ptt => {
                let active = self.shared.lock().map(|s| s.ptt_active).unwrap_or(false);
                self.send(if active {
                    ControlCommand::PttRelease
                } else {
                    ControlCommand::PttAssert
                });
            }
            Message::ModeSelected(m) => {
                self.mode_sel = m.clone();
                self.send(ControlCommand::SetMode { mode: m });
            }
            Message::FreqChanged(v) => self.freq_khz = v,
            Message::TuneFreq => {
                if let Ok(khz) = self.freq_khz.trim().parse::<f64>() {
                    self.send(ControlCommand::SetFreq {
                        rig: "rigctld".into(),
                        freq_hz: (khz * 1000.0) as u64,
                    });
                }
            }
            Message::PeerCallChanged(c) => self.peer_call = c,
            Message::ConnectPeer => {
                let call = self.peer_call.trim().to_uppercase();
                if !call.is_empty() {
                    self.send(ControlCommand::ConnectPeer { callsign: call });
                }
            }
            Message::DisconnectPeer => self.send(ControlCommand::DisconnectPeer),
            Message::ToggleRepeater => {
                let on = self
                    .shared
                    .lock()
                    .map(|s| s.repeater_enabled)
                    .unwrap_or(false);
                self.send(if on {
                    ControlCommand::DisableRepeater
                } else {
                    ControlCommand::EnableRepeater
                });
            }
            Message::ToggleCessb => {
                self.cessb_on = !self.cessb_on;
                self.send(ControlCommand::SetCessb {
                    enabled: self.cessb_on,
                });
            }
            Message::ToggleNotch => {
                self.notch_on = !self.notch_on;
                self.send(ControlCommand::SetNotch {
                    enabled: self.notch_on,
                });
            }
            Message::ToggleAgc => {
                self.agc_on = !self.agc_on;
                self.send(ControlCommand::SetAgc {
                    enabled: self.agc_on,
                });
            }
            Message::ToggleLogbook => {
                self.logbook_on = !self.logbook_on;
                self.send(ControlCommand::SetLogbook {
                    enabled: self.logbook_on,
                });
            }
            Message::AttenChanged(db) => {
                self.tx_atten_db = db;
                self.send(ControlCommand::SetTxAttenuation { db, band: None });
            }
            Message::SquelchChanged(t) => {
                self.squelch = t;
                self.send(ControlCommand::SetDcdSquelch { threshold: t });
            }
            Message::AcceptQsy => {
                if let Some(token) = self.pending_qsy() {
                    self.send(ControlCommand::AcceptQsy { token });
                }
            }
            Message::RejectQsy => {
                if let Some(token) = self.pending_qsy() {
                    self.send(ControlCommand::RejectQsy { token });
                }
            }
            Message::OtaProfileChanged(p) => self.ota_profile = p,
            Message::StartOta => self.send(ControlCommand::StartOtaSession {
                profile: self.ota_profile.clone(),
            }),
            Message::StopOta => self.send(ControlCommand::StopOtaSession),
            Message::OtaLockToggle => {
                let (locked, level) = self
                    .shared
                    .lock()
                    .map(|s| (s.ota_is_locked, s.ota_tx_level.clone()))
                    .unwrap_or((false, None));
                if locked {
                    self.send(ControlCommand::OtaUnlock);
                } else {
                    self.send(ControlCommand::OtaLockLevel {
                        level: level.unwrap_or_else(|| "SL2".into()),
                    });
                }
            }
        }
        Task::none()
    }

    fn pending_qsy(&self) -> Option<String> {
        self.shared
            .lock()
            .ok()
            .and_then(|s| s.pending_qsy_token.clone())
    }

    pub fn subscription(&self) -> Subscription<Message> {
        // ~10 Hz refresh so live spectrum/metrics repaint from the shared state.
        iced::time::every(Duration::from_millis(100)).map(|_| Message::Tick)
    }

    pub fn effective_theme(&self) -> EffectiveTheme {
        self.theme_mode.effective(self.system_is_dark)
    }

    pub fn theme(&self) -> Theme {
        let eff = self.effective_theme();
        let c = |rgb: (u8, u8, u8)| iced::Color::from_rgb8(rgb.0, rgb.1, rgb.2);
        Theme::custom(
            format!("OpenPulse {}", self.theme_mode.label()),
            iced::theme::Palette {
                background: c(shade_rgb(eff, Shade::Bg)),
                text: c(role_rgb(eff, ColorRole::RxValue)),
                primary: c(role_rgb(eff, ColorRole::Signal)),
                success: c(role_rgb(eff, ColorRole::Locked)),
                danger: c(role_rgb(eff, ColorRole::TxActive)),
            },
        )
    }

    pub fn view(&self) -> iced::Element<'_, Message> {
        ui::view(self)
    }
}
