//! Main application struct and eframe::App implementation.

use std::sync::{Arc, Mutex};

use crossbeam_channel::Sender;
use openpulse_daemon::protocol::ControlCommand;

use crate::connection;
use crate::state::PanelState;
use crate::ui::{draw_event_log, draw_rig_bar, draw_session_status, draw_spectrum_pane};

const MODES: &[&str] = &[
    "BPSK31", "BPSK63", "BPSK100", "BPSK250", "QPSK125", "QPSK250", "QPSK500", "QPSK1000",
    "8PSK500", "8PSK1000", "FSK4-ACK",
];

pub struct PanelApp {
    /// Shared state read on every repaint.
    shared: Arc<Mutex<PanelState>>,
    /// Channel to send commands to the connection thread.
    cmd_tx: Option<Sender<ControlCommand>>,
    /// Channel to stop the connection thread.
    stop_tx: Option<crossbeam_channel::Sender<()>>,

    // UI-local fields (not shared with connection thread).
    server_addr: String,
    selected_mode: String,
    repeater_enabled: bool,
}

impl PanelApp {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(Mutex::new(PanelState::default())),
            cmd_tx: None,
            stop_tx: None,
            server_addr: "127.0.0.1:9000".into(),
            selected_mode: "BPSK250".into(),
            repeater_enabled: false,
        }
    }

    fn connect(&mut self) {
        let (stop_tx, stop_rx) = crossbeam_channel::bounded(1);
        let cmd_tx = connection::spawn(self.server_addr.clone(), Arc::clone(&self.shared), stop_rx);
        self.stop_tx = Some(stop_tx);
        self.cmd_tx = Some(cmd_tx);
    }

    fn disconnect(&mut self) {
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
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }

        // ── Toolbar ──────────────────────────────────────────────────────────
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Server:");
                ui.add(egui::TextEdit::singleline(&mut self.server_addr).desired_width(160.0));

                if connected {
                    if ui.button("Disconnect").clicked() {
                        self.disconnect();
                    }
                } else if ui.button("Connect").clicked() {
                    self.connect();
                }

                ui.separator();
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

                // QSY buttons — visible only when a proposal is pending.
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

                // Connection indicator.
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
        let st = self.shared.lock().unwrap();
        let has_rigs = st.rig_a.is_some() || st.rig_b.is_some();
        drop(st);

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

use egui::RichText;
