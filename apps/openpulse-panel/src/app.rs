//! Main application struct and eframe::App implementation.

use std::sync::{Arc, Mutex};

use crossbeam_channel::Sender;
use egui::{Color32, RichText};
use openpulse_daemon::protocol::ControlCommand;

use crate::connection::{self, TransportKind};
use crate::state::PanelState;
use crate::ui::{draw_event_log, draw_rig_bar, draw_session_status, draw_spectrum_pane};

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
    "FSK4-ACK",
];

pub struct PanelApp {
    /// Shared state read on every repaint.
    shared: Arc<Mutex<PanelState>>,
    /// Channel to send commands to the connection thread.
    cmd_tx: Option<Sender<ControlCommand>>,
    /// Channel to stop the connection thread.
    stop_tx: Option<crossbeam_channel::Sender<()>>,

    // Connection config.
    server_addr: String,
    transport_kind: TransportKind,

    // Toolbar state.
    selected_mode: String,
    repeater_enabled: bool,
    tx_atten_db: f32,

    // RF peer connect.
    peer_callsign_input: String,
}

impl PanelApp {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(Mutex::new(PanelState::default())),
            cmd_tx: None,
            stop_tx: None,
            server_addr: "127.0.0.1:9000".into(),
            transport_kind: TransportKind::Tcp,
            selected_mode: "BPSK250".into(),
            repeater_enabled: false,
            tx_atten_db: 0.0,
            peer_callsign_input: String::new(),
        }
    }

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

    fn disconnect_daemon(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        self.cmd_tx = None;
        self.shared.lock().unwrap().connected = false;
    }

    fn send(&self, cmd: ControlCommand) {
        if let Some(tx) = &self.cmd_tx {
            let _ = tx.try_send(cmd);
        }
    }
}

impl eframe::App for PanelApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Always repaint while connected to show live events.
        let connected = self.shared.lock().unwrap().connected;
        if connected {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        // ── Toolbar ──────────────────────────────────────────────────────────
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Transport picker.
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

        // ── Central: spectrum left | session status right ────────────────────
        egui::CentralPanel::default().show(ctx, |ui| {
            let st = self.shared.lock().unwrap();
            ui.columns(2, |cols| {
                draw_spectrum_pane(&mut cols[0], &st);
                draw_session_status(&mut cols[1], &st);
            });
        });
    }
}
