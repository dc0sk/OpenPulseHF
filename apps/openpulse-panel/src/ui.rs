//! UI drawing functions for openpulse-panel.

use egui::{Color32, RichText, Ui};
use egui_plot::{Line, Plot, PlotPoints};

use crate::state::{PanelState, RigSnapshot};

/// Top rig-status bar: one row per configured rig.
pub fn draw_rig_bar(ui: &mut Ui, st: &PanelState) {
    ui.horizontal(|ui| {
        rig_widget(ui, "Rig A", st.rig_a.as_ref());
        ui.separator();
        rig_widget(ui, "Rig B", st.rig_b.as_ref());
    });
}

fn rig_widget(ui: &mut Ui, label: &str, snap: Option<&RigSnapshot>) {
    ui.label(RichText::new(label).strong());
    match snap {
        None => {
            ui.label(RichText::new("not configured").color(Color32::DARK_GRAY));
        }
        Some(s) => {
            let freq_mhz = s.freq_hz as f64 / 1_000_000.0;
            ui.label(format!("{freq_mhz:.3} MHz"));
            ui.label(&s.mode);
            if let Some(p) = s.power_w {
                ui.label(format!("{p:.0}W"));
            }
            if let Some(swr) = s.swr {
                let color = if swr > 2.0 {
                    Color32::YELLOW
                } else {
                    Color32::GREEN
                };
                ui.label(RichText::new(format!("SWR {swr:.1}")).color(color));
            }
            if let Some(alc) = s.alc {
                if alc > 0.5 {
                    ui.label(RichText::new(format!("ALC {alc:.2}")).color(Color32::YELLOW));
                }
            }
        }
    }
}

/// Left pane: spectrum placeholder + DCD energy bar.
pub fn draw_spectrum_pane(ui: &mut Ui, st: &PanelState) {
    ui.vertical(|ui| {
        ui.label(RichText::new("Spectrum / AFC").strong());

        // Simple AFC offset plot using the last known value.
        let afc = st.afc_hz;
        let points: PlotPoints = (0..=100)
            .map(|i| {
                let x = (i as f64 - 50.0) * 30.0; // ±1500 Hz range
                let dist = (x - afc as f64).abs();
                let y = (-dist * dist / (2.0 * 150.0 * 150.0)).exp();
                [x + 1500.0, y]
            })
            .collect();

        Plot::new("spectrum")
            .height(120.0)
            .include_y(0.0)
            .include_y(1.1)
            .x_axis_label("Hz")
            .show(ui, |plot_ui| {
                plot_ui.line(Line::new(points).color(Color32::from_rgb(100, 200, 100)));
            });

        // DCD energy bar.
        ui.horizontal(|ui| {
            ui.label("DCD");
            let energy_norm = (st.dcd_energy * 10.0).min(1.0);
            let color = if st.dcd_busy {
                Color32::RED
            } else {
                Color32::GREEN
            };
            let (rect, _) = ui.allocate_exact_size(egui::vec2(120.0, 14.0), egui::Sense::hover());
            if ui.is_rect_visible(rect) {
                let filled = egui::Rect::from_min_size(
                    rect.min,
                    egui::vec2(rect.width() * energy_norm, rect.height()),
                );
                ui.painter().rect_filled(rect, 2.0, Color32::DARK_GRAY);
                ui.painter().rect_filled(filled, 2.0, color);
            }
            ui.label(format!(
                "{:.1} dBFS",
                20.0 * st.dcd_energy.log10().max(-99.0)
            ));
        });

        if let Some(dbm) = st.signal_strength_dbm {
            ui.label(format!("S-meter: {dbm} dBm"));
        }
    });
}

/// Right pane: session status grid.
pub fn draw_session_status(ui: &mut Ui, st: &PanelState) {
    ui.vertical(|ui| {
        ui.label(RichText::new("Session Status").strong());
        egui::Grid::new("session_grid")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                kv(ui, "Mode", &st.mode);
                kv(ui, "Speed", &st.speed_level);
                kv(ui, "Eff. bps", &format!("{:.0}", st.effective_bps));
                kv(ui, "ECC rate", &format!("{:.1}%", st.ecc_rate * 100.0));
                kv(ui, "Compress", &format!("{:.2}×", st.compress_ratio));
                kv(ui, "AFC", &format!("{:+.1} Hz", st.afc_hz));
                kv(ui, "DCD", if st.dcd_busy { "BUSY" } else { "CLEAR" });
            });
    });
}

fn kv(ui: &mut Ui, key: &str, val: &str) {
    ui.label(RichText::new(key).color(Color32::GRAY));
    ui.label(val);
    ui.end_row();
}

/// Bottom scrollable event log (last 100 entries).
pub fn draw_event_log(ui: &mut Ui, st: &PanelState) {
    ui.label(RichText::new("Event Log").strong());
    egui::ScrollArea::vertical()
        .id_salt("event_log")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for entry in &st.event_log {
                ui.label(entry);
            }
        });
}
