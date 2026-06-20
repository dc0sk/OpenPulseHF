use std::sync::{Arc, RwLock};

use crate::signal_path::spawn_signal_thread;
use crate::state::{AppState, AudioSource};
use crate::ui::{
    draw_scatter_cell, draw_spectrum_cell, draw_stats, draw_toolbar, draw_waterfall_cell,
};

pub struct TestbenchApp {
    state: AppState,
    signal_thread: Option<std::thread::JoinHandle<()>>,
    textures: [Option<egui::TextureHandle>; 4],
    last_gen: [u64; 4],
}

impl TestbenchApp {
    pub fn new() -> Self {
        Self {
            state: AppState::new(),
            signal_thread: None,
            textures: [None, None, None, None],
            last_gen: [u64::MAX; 4],
        }
    }

    fn start(&mut self) {
        self.state.reset();
        self.last_gen = [u64::MAX; 4];
        let (stop_tx, stop_rx) = crossbeam_channel::bounded(1);
        self.state.stop_tx = Some(stop_tx);
        self.state.running = true;

        let shared_cfg = Arc::new(RwLock::new(self.state.config.clone()));
        self.state.shared_config = Some(Arc::clone(&shared_cfg));

        self.signal_thread = Some(spawn_signal_thread(
            shared_cfg,
            self.state.taps.clone(),
            self.state.stats.clone(),
            stop_rx,
        ));
    }

    fn stop(&mut self) {
        if let Some(tx) = self.state.stop_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.signal_thread.take() {
            let _ = handle.join();
        }
        self.state.running = false;
        self.state.shared_config = None;
    }
}

impl eframe::App for TestbenchApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Detect a signal thread that exited early (e.g. audio open failure).
        if self.state.running
            && self
                .signal_thread
                .as_ref()
                .map(|h| h.is_finished())
                .unwrap_or(false)
        {
            self.signal_thread = None;
            self.state.running = false;
            self.state.shared_config = None;
        }

        // Propagate any UI config changes to the running signal thread.
        if self.state.running {
            if let Some(shared) = &self.state.shared_config {
                *shared.write().unwrap() = self.state.config.clone();
            }
        }

        if self.state.running {
            ctx.request_repaint();
        }

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            let mut run = false;
            let mut stop = false;
            draw_toolbar(ui, &mut self.state, || run = true, || stop = true);
            if run {
                self.start();
            }
            if stop {
                self.stop();
            }
        });

        egui::TopBottomPanel::bottom("stats").show(ctx, |ui| {
            draw_stats(ui, &self.state);
        });

        let config = self.state.config.clone();
        egui::CentralPanel::default().show(ctx, |ui| {
            let panel_names = match self.state.config.audio_source {
                #[cfg(feature = "cpal")]
                AudioSource::LiveCapture => ["TX (ref)", "(silent)", "Captured", "Demodulated"],
                #[cfg(feature = "cpal")]
                AudioSource::HardwareLoop => {
                    ["TX (out)", "(silent)", "Captured (in)", "Demodulated"]
                }
                AudioSource::VirtualLoop | AudioSource::TestMatrix => [
                    "TX (clean)",
                    "Channel impairment",
                    "Post-channel",
                    "RX (decoded)",
                ],
                AudioSource::Synthetic => [
                    "TX (clean)",
                    "Noise channel",
                    "Mixed (TX+noise)",
                    "RX (decoded)",
                ],
            };
            let col_width = ui.available_width() / 4.0;

            // Explicit 2×4 grid: row 1 = spectra, row 2 = waterfalls, row 3 = RX scatter.
            // Section heights are derived from the available height so all three rows
            // stay visible (the waterfall row never falls below the fold).
            let avail_h = ui.available_height();
            let caption_h = 20.0;
            let row_label_h = 18.0;
            let scatter_h = 150.0_f32;
            let body = (avail_h - caption_h - 2.0 * row_label_h - scatter_h - 24.0).max(160.0);
            let spectrum_h = (body * 0.5).clamp(120.0, 320.0);
            let waterfall_h = (body - spectrum_h).clamp(100.0, 320.0);

            // Column captions.
            ui.horizontal(|ui| {
                for &name in &panel_names {
                    ui.allocate_ui(egui::vec2(col_width, caption_h), |ui| {
                        ui.vertical_centered(|ui| {
                            ui.strong(name);
                        });
                    });
                }
            });

            // Row 1 — spectra.
            ui.horizontal(|ui| {
                for (i, &name) in panel_names.iter().enumerate() {
                    ui.allocate_ui(egui::vec2(col_width, spectrum_h), |ui| {
                        draw_spectrum_cell(ui, name, &self.state.taps[i], &config, spectrum_h);
                    });
                }
            });

            // Row 2 — waterfalls.
            ui.add_space(2.0);
            ui.label(egui::RichText::new("Waterfalls").weak());
            ui.horizontal(|ui| {
                for (i, &name) in panel_names.iter().enumerate() {
                    ui.allocate_ui(egui::vec2(col_width, waterfall_h), |ui| {
                        draw_waterfall_cell(
                            ui,
                            name,
                            &self.state.taps[i],
                            &mut self.textures[i],
                            &mut self.last_gen[i],
                            &config,
                            waterfall_h,
                        );
                    });
                }
            });

            // Row 3 — RX constellation (last column only).
            ui.add_space(2.0);
            ui.label(egui::RichText::new("RX constellation").weak());
            ui.horizontal(|ui| {
                for (i, &name) in panel_names.iter().enumerate() {
                    ui.allocate_ui(egui::vec2(col_width, scatter_h), |ui| {
                        if i == 3 {
                            draw_scatter_cell(ui, name, &self.state.taps[i], scatter_h);
                        }
                    });
                }
            });
        });
    }
}
