//! Main application struct and eframe::App implementation.

use std::sync::{Arc, Mutex};

#[cfg(not(target_arch = "wasm32"))]
use crossbeam_channel::Sender;
use egui::{Color32, RichText};
use openpulse_daemon::protocol::ControlCommand;

use crate::connection::{self, TransportKind};
use crate::state::{DaemonConfig, PanelState};
use crate::ui::{
    build_waterfall_image, draw_event_log, draw_messages_window, draw_rig_bar, draw_session_status,
    draw_spectrum_pane, ComposeState,
};

const MODES: &[&str] = &[
    "BPSK31",
    "BPSK63",
    "BPSK100",
    "BPSK250",
    "QPSK125",
    "QPSK250",
    "QPSK500",
    "QPSK1000",
    "8PSK500",
    "8PSK1000",
    "64QAM500",
    "64QAM1000",
    "64QAM2000-RRC",
    "SCFDMA16",
    "SCFDMA52",
    "SCFDMA52-16QAM",
    "SCFDMA52-64QAM",
    "FSK4-ACK",
];

const BANDPLAN_OPTIONS: &[(&str, &str)] = &[
    ("unrestricted", "Unrestricted"),
    ("ham-iaru-r1", "IARU Region 1"),
    ("ham-iaru-r2", "IARU Region 2"),
    ("ham-iaru-r3", "IARU Region 3"),
];

fn bandplan_label(mode: &str) -> &'static str {
    BANDPLAN_OPTIONS
        .iter()
        .find(|(k, _)| *k == mode)
        .map(|(_, l)| *l)
        .unwrap_or("Unrestricted")
}

pub struct PanelApp {
    /// Shared state read on every repaint.
    shared: Arc<Mutex<PanelState>>,

    // Native-only: background connection thread channels.
    #[cfg(not(target_arch = "wasm32"))]
    cmd_tx: Option<Sender<ControlCommand>>,
    #[cfg(not(target_arch = "wasm32"))]
    stop_tx: Option<crossbeam_channel::Sender<()>>,

    // WASM-only: inline WebSocket transport.
    #[cfg(target_arch = "wasm32")]
    wasm_sender: Option<ewebsock::WsSender>,
    #[cfg(target_arch = "wasm32")]
    wasm_receiver: Option<ewebsock::WsReceiver>,

    // Connection config.
    server_addr: String,
    transport_kind: TransportKind,

    // Toolbar state.
    selected_mode: String,
    repeater_enabled: bool,
    tx_atten_db: f32,

    // RF peer connect.
    peer_callsign_input: String,

    // Config window.
    config_open: bool,
    config_draft: DaemonConfig,
    config_fetch_pending: bool,

    // Messages window.
    messages_open: bool,
    compose_to: String,
    compose_subject: String,
    compose_body: String,

    // Waterfall texture; only rebuilt when spectrum_generation changes.
    waterfall_tex: Option<egui::TextureHandle>,
    waterfall_generation: u64,
}

impl PanelApp {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(Mutex::new(PanelState::default())),
            #[cfg(not(target_arch = "wasm32"))]
            cmd_tx: None,
            #[cfg(not(target_arch = "wasm32"))]
            stop_tx: None,
            #[cfg(target_arch = "wasm32")]
            wasm_sender: None,
            #[cfg(target_arch = "wasm32")]
            wasm_receiver: None,
            // WASM only supports WebSocket; default to the WS endpoint.
            #[cfg(target_arch = "wasm32")]
            server_addr: "ws://127.0.0.1:9001".into(),
            #[cfg(not(target_arch = "wasm32"))]
            server_addr: "127.0.0.1:9000".into(),
            #[cfg(not(target_arch = "wasm32"))]
            transport_kind: TransportKind::Tcp,
            #[cfg(target_arch = "wasm32")]
            transport_kind: TransportKind::WebSocket,
            selected_mode: "BPSK250".into(),
            repeater_enabled: false,
            tx_atten_db: 0.0,
            peer_callsign_input: String::new(),
            config_open: false,
            config_draft: DaemonConfig {
                callsign: String::new(),
                grid_square: String::new(),
                mode: "BPSK250".into(),
                tx_attenuation_db: 0.0,
                qsy_enabled: false,
                bandplan_mode: "unrestricted".into(),
            },
            config_fetch_pending: false,
            messages_open: false,
            compose_to: String::new(),
            compose_subject: String::new(),
            compose_body: String::new(),
            waterfall_tex: None,
            waterfall_generation: u64::MAX,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn connect(&mut self) {
        let (stop_tx, stop_rx) = crossbeam_channel::bounded(1);
        let cmd_tx = connection::spawn(
            self.server_addr.clone(),
            self.transport_kind.clone(),
            Arc::clone(&self.shared),
            stop_rx,
        );
        self.stop_tx = Some(stop_tx);
        self.cmd_tx = Some(cmd_tx);
    }

    #[cfg(target_arch = "wasm32")]
    fn connect(&mut self) {
        let url = if self.server_addr.starts_with("ws") {
            self.server_addr.clone()
        } else {
            format!("ws://{}", self.server_addr)
        };
        match ewebsock::connect(url, ewebsock::Options::default()) {
            Ok((mut sender, receiver)) => {
                if let Ok(s) = serde_json::to_string(&ControlCommand::SubscribeSpectrum { fps: 20 })
                {
                    sender.send(ewebsock::WsMessage::Text(s));
                }
                self.wasm_sender = Some(sender);
                self.wasm_receiver = Some(receiver);
                let mut st = self.shared.lock().unwrap();
                st.push_log(format!("connecting to {}", self.server_addr));
            }
            Err(e) => {
                self.shared
                    .lock()
                    .unwrap()
                    .push_log(format!("WS connect error: {e}"));
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn disconnect_daemon(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        self.cmd_tx = None;
        self.shared.lock().unwrap().connected = false;
    }

    #[cfg(target_arch = "wasm32")]
    fn disconnect_daemon(&mut self) {
        self.wasm_sender = None;
        self.wasm_receiver = None;
        self.shared.lock().unwrap().connected = false;
    }

    /// Returns `true` if the command was successfully enqueued.
    fn send(&mut self, cmd: ControlCommand) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(tx) = &self.cmd_tx {
                return tx.try_send(cmd).is_ok();
            }
            false
        }
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(sender) = &mut self.wasm_sender {
                if let Ok(s) = serde_json::to_string(&cmd) {
                    sender.send(ewebsock::WsMessage::Text(s));
                    return true;
                }
            }
            false
        }
    }

    /// On WASM, drain inbound WebSocket messages from the main thread.
    #[cfg(target_arch = "wasm32")]
    fn poll_wasm(&mut self) {
        let mut disconnected = false;
        if let Some(receiver) = &mut self.wasm_receiver {
            loop {
                match receiver.try_recv() {
                    None => break,
                    Some(ewebsock::WsEvent::Opened) => {
                        self.shared.lock().unwrap().connected = true;
                        self.shared
                            .lock()
                            .unwrap()
                            .push_log(format!("connected to {}", self.server_addr));
                    }
                    Some(ewebsock::WsEvent::Message(ewebsock::WsMessage::Text(line))) => {
                        if !line.is_empty() {
                            connection::apply_event(&line, &self.shared);
                        }
                    }
                    Some(ewebsock::WsEvent::Message(ewebsock::WsMessage::Binary(bytes))) => {
                        connection::apply_spectrum(&bytes, &self.shared);
                    }
                    Some(ewebsock::WsEvent::Message(_)) => {}
                    Some(ewebsock::WsEvent::Error(e)) => {
                        self.shared
                            .lock()
                            .unwrap()
                            .push_log(format!("WS error: {e}"));
                        disconnected = true;
                        break;
                    }
                    Some(ewebsock::WsEvent::Closed) => {
                        self.shared.lock().unwrap().push_log("disconnected".into());
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        if disconnected {
            self.wasm_sender = None;
            self.wasm_receiver = None;
            self.shared.lock().unwrap().connected = false;
        }
    }
}

impl eframe::App for PanelApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // WASM: drain inbound WebSocket events before drawing.
        #[cfg(target_arch = "wasm32")]
        self.poll_wasm();

        // Always repaint while connected to show live events.
        let connected = self.shared.lock().unwrap().connected;
        if connected {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        // ── Toolbar ──────────────────────────────────────────────────────────
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                // Transport picker — TCP only available on native; WASM always uses WS.
                #[cfg(not(target_arch = "wasm32"))]
                egui::ComboBox::from_id_salt("transport")
                    .selected_text(match self.transport_kind {
                        TransportKind::Tcp => "TCP",
                        TransportKind::WebSocket => "WS",
                    })
                    .width(40.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.transport_kind, TransportKind::Tcp, "TCP");
                        ui.selectable_value(
                            &mut self.transport_kind,
                            TransportKind::WebSocket,
                            "WS",
                        );
                    });
                #[cfg(target_arch = "wasm32")]
                ui.label("WS");

                ui.label("Server:");
                ui.add(egui::TextEdit::singleline(&mut self.server_addr).desired_width(160.0));

                if connected {
                    if ui.button("Disconnect").clicked() {
                        self.disconnect_daemon();
                    }
                } else if ui.button("Connect").clicked() {
                    self.connect();
                }

                ui.separator();

                // ── PTT button ────────────────────────────────────────────────
                let ptt_now = self.shared.lock().unwrap().ptt_active;
                let (ptt_color, ptt_label) = if ptt_now {
                    (Color32::RED, "● PTT")
                } else {
                    (Color32::DARK_GRAY, "○ PTT")
                };
                let ptt_btn = ui.add(
                    egui::Button::new(RichText::new(ptt_label).color(ptt_color))
                        .min_size(egui::vec2(60.0, 0.0)),
                );
                if ptt_btn.clicked() {
                    if ptt_now {
                        self.send(ControlCommand::PttRelease);
                    } else {
                        self.send(ControlCommand::PttAssert);
                    }
                }

                ui.separator();

                // ── RF peer connect ───────────────────────────────────────────
                let rf_connected = self.shared.lock().unwrap().rf_connected;
                if rf_connected {
                    let peer = self
                        .shared
                        .lock()
                        .unwrap()
                        .rf_peer
                        .clone()
                        .unwrap_or_default();
                    ui.label(RichText::new(format!("RF: {peer}")).color(Color32::GREEN));
                    if ui.button("Disconnect RF").clicked() {
                        self.send(ControlCommand::DisconnectPeer);
                    }
                } else {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.peer_callsign_input)
                            .hint_text("CALLSIGN")
                            .desired_width(80.0),
                    );
                    if ui.button("Connect RF").clicked() && !self.peer_callsign_input.is_empty() {
                        self.send(ControlCommand::ConnectPeer {
                            callsign: self.peer_callsign_input.trim().to_uppercase(),
                        });
                    }
                }

                ui.separator();

                // ── Mode selector ─────────────────────────────────────────────
                ui.label("Mode:");
                egui::ComboBox::from_id_salt("mode_combo")
                    .selected_text(&self.selected_mode)
                    .show_ui(ui, |ui| {
                        for &m in MODES {
                            if ui
                                .selectable_value(&mut self.selected_mode, m.into(), m)
                                .changed()
                            {
                                self.send(ControlCommand::SetMode {
                                    mode: self.selected_mode.clone(),
                                });
                            }
                        }
                    });

                ui.separator();

                // ── Repeater toggle ───────────────────────────────────────────
                let rep_label = if self.repeater_enabled {
                    "Repeater: ON"
                } else {
                    "Repeater: OFF"
                };
                if ui.button(rep_label).clicked() {
                    self.repeater_enabled = !self.repeater_enabled;
                    if self.repeater_enabled {
                        self.send(ControlCommand::EnableRepeater);
                    } else {
                        self.send(ControlCommand::DisableRepeater);
                    }
                }

                ui.separator();

                // ── TX attenuation ────────────────────────────────────────────
                ui.label("TX Atten:");
                if ui
                    .add(
                        egui::Slider::new(&mut self.tx_atten_db, -30.0_f32..=0.0_f32)
                            .suffix(" dB")
                            .fixed_decimals(1),
                    )
                    .changed()
                {
                    self.send(ControlCommand::SetTxAttenuation {
                        db: self.tx_atten_db,
                        band: None,
                    });
                }

                // ── Config toggle ─────────────────────────────────────────────
                ui.separator();
                if ui.selectable_label(self.config_open, "⚙ Config").clicked() {
                    self.config_open = !self.config_open;
                }

                // ── Messages toggle ───────────────────────────────────────────
                let unread = self.shared.lock().unwrap().inbox.len();
                let msg_label = if unread > 0 {
                    format!("✉ Messages ({})", unread)
                } else {
                    "✉ Messages".into()
                };
                if ui.selectable_label(self.messages_open, msg_label).clicked() {
                    self.messages_open = !self.messages_open;
                    if self.messages_open {
                        self.send(ControlCommand::ListMessages);
                    }
                }

                // ── QSY buttons ───────────────────────────────────────────────
                let qsy_token = self.shared.lock().unwrap().pending_qsy_token.clone();
                if let Some(token) = qsy_token {
                    ui.separator();
                    ui.label(RichText::new("QSY pending").color(egui::Color32::YELLOW));
                    if ui.button("Accept QSY").clicked() {
                        self.send(ControlCommand::AcceptQsy {
                            token: token.clone(),
                        });
                        self.shared.lock().unwrap().pending_qsy_token = None;
                    }
                    if ui.button("Reject QSY").clicked() {
                        self.send(ControlCommand::RejectQsy { token });
                        self.shared.lock().unwrap().pending_qsy_token = None;
                    }
                }

                // ── Connection indicator ──────────────────────────────────────
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (color, label) = if connected {
                        (egui::Color32::GREEN, "●  Connected")
                    } else {
                        (egui::Color32::RED, "●  Disconnected")
                    };
                    ui.label(RichText::new(label).color(color));
                });
            });
        });

        // ── Rig status bar ───────────────────────────────────────────────────
        let has_rigs = {
            let st = self.shared.lock().unwrap();
            st.rig_a.is_some() || st.rig_b.is_some()
        };
        if has_rigs {
            egui::TopBottomPanel::top("rig_bar").show(ctx, |ui| {
                let st = self.shared.lock().unwrap();
                draw_rig_bar(ui, &st);
            });
        }

        // ── Event log (bottom) ───────────────────────────────────────────────
        egui::TopBottomPanel::bottom("event_log")
            .min_height(120.0)
            .show(ctx, |ui| {
                let st = self.shared.lock().unwrap();
                draw_event_log(ui, &st);
            });

        // Rebuild waterfall texture only when new spectrum data has arrived.
        {
            let st = self.shared.lock().unwrap();
            if st.spectrum_generation != self.waterfall_generation
                && !st.spectrum_history.is_empty()
            {
                let image = build_waterfall_image(&st.spectrum_history);
                self.waterfall_generation = st.spectrum_generation;
                drop(st);
                match &mut self.waterfall_tex {
                    Some(tex) => tex.set(image, egui::TextureOptions::NEAREST),
                    None => {
                        self.waterfall_tex = Some(ctx.load_texture(
                            "waterfall",
                            image,
                            egui::TextureOptions::NEAREST,
                        ));
                    }
                }
            }
        }

        // ── Central: spectrum left | session status right ────────────────────
        egui::CentralPanel::default().show(ctx, |ui| {
            let st = self.shared.lock().unwrap();
            ui.columns(2, |cols| {
                draw_spectrum_pane(&mut cols[0], &st, self.waterfall_tex.as_ref());
                draw_session_status(&mut cols[1], &st);
            });
        });

        // ── Config window ────────────────────────────────────────────────────
        // Populate draft when the GetConfig response arrives.
        // Also sync toolbar controls so they reflect the daemon's actual state.
        if self.config_fetch_pending {
            if let Some(cfg) = self.shared.lock().unwrap().daemon_config.clone() {
                self.selected_mode = cfg.mode.clone();
                self.tx_atten_db = cfg.tx_attenuation_db;
                self.config_draft = cfg;
                self.config_fetch_pending = false;
            }
        }

        // ── Messages window ──────────────────────────────────────────────────
        if self.messages_open {
            let mut close = false;
            let mut get_msg_id: Option<u64> = None;
            let mut send_msg: Option<(String, String, String)> = None;
            let mut delete_msg_id: Option<u64> = None;

            egui::Window::new("Messages")
                .resizable(true)
                .collapsible(false)
                .default_size([540.0, 400.0])
                .show(ctx, |ui| {
                    draw_messages_window(
                        ui,
                        &self.shared.lock().unwrap(),
                        &mut ComposeState {
                            to: &mut self.compose_to,
                            subject: &mut self.compose_subject,
                            body: &mut self.compose_body,
                            close: &mut close,
                            get_msg_id: &mut get_msg_id,
                            send_msg: &mut send_msg,
                            delete_msg_id: &mut delete_msg_id,
                        },
                    );
                });

            if close {
                self.messages_open = false;
            }
            if let Some(id) = get_msg_id {
                self.send(ControlCommand::GetMessage { id });
            }
            if let Some((to, subject, body)) = send_msg {
                self.send(ControlCommand::SendMessage { to, subject, body });
                self.compose_to.clear();
                self.compose_subject.clear();
                self.compose_body.clear();
            }
            if let Some(id) = delete_msg_id {
                if self.send(ControlCommand::DeleteMessage { id }) {
                    let mut st = self.shared.lock().unwrap();
                    st.inbox.retain(|m| m.id != id);
                    if st.open_message_id == Some(id) {
                        st.open_message_id = None;
                        st.open_message_body = None;
                    }
                } else {
                    self.shared
                        .lock()
                        .unwrap()
                        .push_log("delete failed: channel full or disconnected".into());
                }
            }
        }

        if self.config_open {
            egui::Window::new("Daemon Config")
                .resizable(true)
                .collapsible(false)
                .default_size([320.0, 220.0])
                .show(ctx, |ui| {
                    egui::Grid::new("cfg_grid")
                        .num_columns(2)
                        .spacing([8.0, 4.0])
                        .show(ui, |ui| {
                            ui.label("Callsign:");
                            ui.label(&self.config_draft.callsign);
                            ui.end_row();

                            ui.label("Grid square:");
                            ui.label(&self.config_draft.grid_square);
                            ui.end_row();

                            ui.label("Mode:");
                            egui::ComboBox::from_id_salt("cfg_mode")
                                .selected_text(&self.config_draft.mode)
                                .show_ui(ui, |ui| {
                                    for &m in MODES {
                                        ui.selectable_value(
                                            &mut self.config_draft.mode,
                                            m.into(),
                                            m,
                                        );
                                    }
                                });
                            ui.end_row();

                            ui.label("TX Atten:");
                            ui.add(
                                egui::Slider::new(
                                    &mut self.config_draft.tx_attenuation_db,
                                    -30.0_f32..=0.0_f32,
                                )
                                .suffix(" dB")
                                .fixed_decimals(1),
                            );
                            ui.end_row();

                            ui.label("QSY:");
                            ui.checkbox(&mut self.config_draft.qsy_enabled, "Enabled");
                            ui.end_row();

                            ui.label("Bandplan:");
                            egui::ComboBox::from_id_salt("cfg_bandplan")
                                .selected_text(bandplan_label(&self.config_draft.bandplan_mode))
                                .show_ui(ui, |ui| {
                                    for &(key, label) in BANDPLAN_OPTIONS {
                                        ui.selectable_value(
                                            &mut self.config_draft.bandplan_mode,
                                            key.into(),
                                            label,
                                        );
                                    }
                                });
                            ui.end_row();
                        });

                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Fetch").clicked() {
                            // Clear stale snapshot so the pending check only resolves
                            // on the new response, not an old one.
                            self.shared.lock().unwrap().daemon_config = None;
                            self.send(ControlCommand::GetConfig);
                            self.config_fetch_pending = true;
                        }
                        if ui.button("Apply").clicked() {
                            self.send(ControlCommand::SetConfig {
                                config: self.config_draft.clone(),
                            });
                        }
                        if ui.button("Close").clicked() {
                            self.config_open = false;
                        }
                    });
                });
        }
    }
}
