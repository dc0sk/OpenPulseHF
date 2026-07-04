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
use crate::state::{DaemonConfig, PanelState};
use crate::theme::{role_rgb, shade_rgb, ColorRole, EffectiveTheme, Shade, ThemeMode};
use crate::ui;

fn default_config() -> DaemonConfig {
    DaemonConfig {
        callsign: String::new(),
        grid_square: String::new(),
        mode: "BPSK250".into(),
        tx_attenuation_db: 0.0,
        qsy_enabled: false,
        bandplan_mode: "unrestricted".into(),
        allow_tuner_on_high_swr: false,
    }
}

/// Speed-level rungs shown on the ladder (SL1..=SLN).
pub const LADDER_RUNGS: u8 = 20;

/// Which of the lower panel's tabs is shown (Info → Config → Messages → Event log).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Info,
    Config,
    Messages,
    Log,
}

/// Top-level panel application state.
pub struct App {
    // --- theme ---
    pub theme_mode: ThemeMode,
    pub system_is_dark: bool,

    // --- daemon connection ---
    pub shared: Arc<Mutex<PanelState>>,
    pub cmd_tx: Option<Sender<ControlCommand>>,
    pub stop_tx: Option<Sender<()>>,
    /// Control-port address (`host:port` for TCP, `ws://host:port` for WebSocket).
    pub addr: String,
    /// Selected transport (TCP or WebSocket).
    pub transport_kind: TransportKind,

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

    // --- messages ---
    pub msg_to: String,
    pub msg_subject: String,
    pub msg_body: String,

    // --- config editor ---
    pub config_draft: DaemonConfig,
    pub config_fetch_pending: bool,

    /// Active Messages/Log tab.
    pub active_tab: Tab,

    pub tick: u32,
}

/// UI messages.
#[derive(Debug, Clone)]
pub enum Message {
    ToggleTheme,
    Tick,
    SelectTab(Tab),
    AddrChanged(String),
    SelectTransport(bool),
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
    // messages
    MsgTo(String),
    MsgSubject(String),
    MsgBody(String),
    SendMsg,
    RefreshInbox,
    OpenMsg(u64),
    DeleteMsg(u64),
    // config
    FetchConfig,
    ApplyConfig,
    CfgMode(String),
    CfgAtten(f32),
    CfgQsy(bool),
    CfgBandplan(String),
    CfgTuneSwr(bool),
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
            transport_kind: TransportKind::Tcp,
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
            msg_to: String::new(),
            msg_subject: String::new(),
            msg_body: String::new(),
            config_draft: default_config(),
            config_fetch_pending: false,
            active_tab: Tab::default(),
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
            self.transport_kind.clone(),
            self.shared.clone(),
            stop_rx,
        );
        self.stop_tx = Some(stop_tx);
        self.cmd_tx = Some(cmd_tx);
        // Pull the daemon config so the always-visible Config panel populates.
        self.config_fetch_pending = true;
        self.send(ControlCommand::GetConfig);
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
            Message::SelectTab(t) => self.active_tab = t,
            Message::Tick => {
                self.tick = self.tick.wrapping_add(1);
                // Config fetch is async: once the daemon's ConfigData arrives, seed the draft.
                if self.config_fetch_pending {
                    if let Ok(st) = self.shared.lock() {
                        if let Some(cfg) = &st.daemon_config {
                            self.config_draft = cfg.clone();
                            self.config_fetch_pending = false;
                        }
                    }
                }
            }
            Message::AddrChanged(a) => self.addr = a,
            Message::SelectTransport(ws) => {
                self.transport_kind = if ws {
                    TransportKind::WebSocket
                } else {
                    TransportKind::Tcp
                };
                // Offer a sensible default address for the chosen scheme.
                if ws && !self.addr.starts_with("ws") {
                    self.addr = "ws://127.0.0.1:9001".into();
                } else if !ws && self.addr.starts_with("ws") {
                    self.addr = "127.0.0.1:9000".into();
                }
            }
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
            // --- messages ---
            Message::MsgTo(v) => self.msg_to = v,
            Message::MsgSubject(v) => self.msg_subject = v,
            Message::MsgBody(v) => self.msg_body = v,
            Message::SendMsg => {
                let (to, subject, body) = (
                    self.msg_to.trim().to_uppercase(),
                    self.msg_subject.trim().to_string(),
                    self.msg_body.clone(),
                );
                if !to.is_empty() && !subject.is_empty() && !body.is_empty() {
                    self.send(ControlCommand::SendMessage { to, subject, body });
                    self.msg_to.clear();
                    self.msg_subject.clear();
                    self.msg_body.clear();
                }
            }
            Message::RefreshInbox => self.send(ControlCommand::ListMessages),
            Message::OpenMsg(id) => self.send(ControlCommand::GetMessage { id }),
            Message::DeleteMsg(id) => {
                self.send(ControlCommand::DeleteMessage { id });
                if let Ok(mut st) = self.shared.lock() {
                    st.inbox.retain(|m| m.id != id);
                    if st.open_message_id == Some(id) {
                        st.open_message_id = None;
                        st.open_message_body = None;
                    }
                }
            }
            // --- config ---
            Message::FetchConfig => {
                if let Ok(mut st) = self.shared.lock() {
                    st.daemon_config = None;
                }
                self.config_fetch_pending = true;
                self.send(ControlCommand::GetConfig);
            }
            Message::ApplyConfig => self.send(ControlCommand::SetConfig {
                config: self.config_draft.clone(),
            }),
            Message::CfgMode(m) => self.config_draft.mode = m,
            Message::CfgAtten(db) => self.config_draft.tx_attenuation_db = db,
            Message::CfgQsy(b) => self.config_draft.qsy_enabled = b,
            Message::CfgBandplan(b) => self.config_draft.bandplan_mode = b,
            Message::CfgTuneSwr(b) => self.config_draft.allow_tuner_on_high_swr = b,
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
