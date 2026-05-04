use crate::signal_path::spawn_signal_thread;
use crate::state::AppState;
use crate::ui::{draw_signal_panel, draw_stats, draw_toolbar};

pub struct TestbenchApp {
    state: AppState,
    textures: [Option<egui::TextureHandle>; 4],
    last_gen: [u64; 4],
}

impl TestbenchApp {
    pub fn new() -> Self {
        Self {
            state: AppState::new(),
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

        spawn_signal_thread(
            self.state.config.clone(),
            self.state.taps.clone(),
            self.state.stats.clone(),
            stop_rx,
        );
    }

    fn stop(&mut self) {
        if let Some(tx) = self.state.stop_tx.take() {
            let _ = tx.send(());
        }
        self.state.running = false;
    }
}

impl eframe::App for TestbenchApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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
            let panel_names = [
                "TX (clean)",
                "Noise channel",
                "Mixed (TX+noise)",
                "RX (decoded)",
            ];
            let available_width = ui.available_width();
            let col_width = available_width / 4.0;

            ui.horizontal(|ui| {
                for (i, &name) in panel_names.iter().enumerate() {
                    ui.allocate_ui(egui::vec2(col_width, ui.available_height()), |ui| {
                        draw_signal_panel(
                            ui,
                            name,
                            &self.state.taps[i],
                            &mut self.textures[i],
                            &mut self.last_gen[i],
                            &config,
                        );
                    });
                }
            });
        });
    }
}
