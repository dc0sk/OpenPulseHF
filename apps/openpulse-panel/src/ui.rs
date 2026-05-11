//! UI drawing functions for openpulse-panel.

use egui::{Color32, RichText, Ui};
use egui_plot::{Line, Plot, PlotPoints};

use crate::state::{PanelState, RigSnapshot};

// ---------------------------------------------------------------------------
// Rig bar
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Spectrum + waterfall
// ---------------------------------------------------------------------------

/// Left pane: real power-spectrum plot, waterfall, and DCD energy bar.
pub fn draw_spectrum_pane(
    ui: &mut Ui,
    st: &PanelState,
    waterfall_tex: Option<&egui::TextureHandle>,
) {
    ui.vertical(|ui| {
        ui.label(RichText::new("Spectrum").strong());

        if st.spectrum_bins.is_empty() {
            ui.label(RichText::new("waiting for spectrum…").color(Color32::DARK_GRAY));
        } else {
            let points: PlotPoints = st
                .spectrum_bins
                .iter()
                .enumerate()
                .map(|(i, &v)| [i as f64, v as f64])
                .collect();
            Plot::new("spectrum")
                .height(120.0)
                .include_y(-120.0)
                .include_y(0.0)
                .x_axis_label("bin")
                .y_axis_label("dBFS")
                .show(ui, |plot_ui| {
                    plot_ui.line(Line::new(points).color(Color32::from_rgb(100, 200, 100)));
                });
        }

        // Waterfall texture.
        ui.label(RichText::new("Waterfall").strong());
        if let Some(tex) = waterfall_tex {
            let size = egui::vec2(ui.available_width().min(512.0), 64.0);
            ui.image((tex.id(), size));
        } else {
            ui.label(RichText::new("waiting for waterfall…").color(Color32::DARK_GRAY));
        }

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

/// Build a `ColorImage` from the rolling spectrum history (newest row at top).
/// Each pixel column is one FFT bin; each row is one time snapshot.
pub fn build_waterfall_image(history: &std::collections::VecDeque<Vec<f32>>) -> egui::ColorImage {
    const COLS: usize = 512;
    const ROWS: usize = crate::state::WATERFALL_ROWS;

    let mut pixels = vec![egui::Color32::BLACK; COLS * ROWS];

    for (row_idx, bins) in history.iter().enumerate() {
        for col in 0..COLS {
            let dbfs = bins.get(col).copied().unwrap_or(-120.0);
            // Map -120..0 dBFS → 0.0..1.0.
            let t = ((dbfs + 120.0) / 120.0).clamp(0.0, 1.0);
            pixels[row_idx * COLS + col] = plasma(t);
        }
    }

    egui::ColorImage {
        size: [COLS, ROWS],
        pixels,
    }
}

/// Plasma colormap approximation: 0.0 = dark blue, 0.5 = red, 1.0 = bright yellow.
fn plasma(t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let (r, g, b) = if t < 0.25 {
        let s = t * 4.0;
        (
            lerp(13.0, 94.0, s),
            lerp(8.0, 0.0, s),
            lerp(135.0, 165.0, s),
        )
    } else if t < 0.5 {
        let s = (t - 0.25) * 4.0;
        (
            lerp(94.0, 200.0, s),
            lerp(0.0, 18.0, s),
            lerp(165.0, 75.0, s),
        )
    } else if t < 0.75 {
        let s = (t - 0.5) * 4.0;
        (
            lerp(200.0, 253.0, s),
            lerp(18.0, 141.0, s),
            lerp(75.0, 26.0, s),
        )
    } else {
        let s = (t - 0.75) * 4.0;
        (
            lerp(253.0, 252.0, s),
            lerp(141.0, 255.0, s),
            lerp(26.0, 164.0, s),
        )
    };
    Color32::from_rgb(r as u8, g as u8, b as u8)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

// ---------------------------------------------------------------------------
// HPX state diagram
// ---------------------------------------------------------------------------

const HPX_STATES: &[&str] = &[
    "Idle",
    "Discovery",
    "Training",
    "ActiveTransfer",
    "Recovery",
    "RelayActive",
    "Teardown",
    "Failed",
];

/// Horizontal row of HPX state chips; active state is highlighted.
pub fn draw_hpx_state(ui: &mut Ui, st: &PanelState) {
    ui.label(RichText::new("HPX State").strong());
    ui.horizontal_wrapped(|ui| {
        for &state in HPX_STATES {
            let active = st.hpx_state == state;
            let (bg, fg) = hpx_state_colors(state, active);
            let label = RichText::new(state).color(fg).small();
            let frame = egui::Frame::none()
                .fill(bg)
                .inner_margin(egui::Margin::symmetric(4.0, 2.0))
                .rounding(egui::Rounding::same(3.0));
            frame.show(ui, |ui| {
                ui.label(label);
            });
        }
    });
}

fn hpx_state_colors(state: &str, active: bool) -> (Color32, Color32) {
    let base = match state {
        "Idle" => Color32::from_rgb(60, 60, 60),
        "Discovery" => Color32::from_rgb(30, 80, 160),
        "Training" => Color32::from_rgb(30, 130, 160),
        "ActiveTransfer" => Color32::from_rgb(30, 140, 50),
        "Recovery" => Color32::from_rgb(160, 130, 20),
        "RelayActive" => Color32::from_rgb(100, 30, 160),
        "Teardown" => Color32::from_rgb(160, 80, 20),
        "Failed" => Color32::from_rgb(160, 30, 30),
        _ => Color32::DARK_GRAY,
    };
    if active {
        // Brighten and use white text.
        let brightened = Color32::from_rgb(
            (base.r() as u16 + 60).min(255) as u8,
            (base.g() as u16 + 60).min(255) as u8,
            (base.b() as u16 + 60).min(255) as u8,
        );
        (brightened, Color32::WHITE)
    } else {
        (Color32::from_gray(30), Color32::from_gray(100))
    }
}

// ---------------------------------------------------------------------------
// Rate ladder bar
// ---------------------------------------------------------------------------

/// Horizontal strip of SL1–SL20 blocks with the current level highlighted.
pub fn draw_rate_ladder(ui: &mut Ui, st: &PanelState) {
    ui.label(RichText::new("Rate Ladder").strong());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for sl in 1u8..=20 {
            let active = st.speed_level_num == sl;
            let label = format!("{sl}");
            let (bg, fg) = if active {
                (Color32::from_rgb(80, 200, 80), Color32::BLACK)
            } else if sl <= 6 {
                (Color32::from_gray(45), Color32::from_gray(130))
            } else if sl <= 11 {
                (Color32::from_gray(45), Color32::from_gray(110))
            } else {
                (Color32::from_gray(35), Color32::from_gray(90))
            };
            let frame = egui::Frame::none()
                .fill(bg)
                .inner_margin(egui::Margin::symmetric(3.0, 1.0))
                .rounding(egui::Rounding::same(2.0));
            frame.show(ui, |ui| {
                ui.label(RichText::new(&label).color(fg).small().monospace());
            });
        }
    });
    // Sub-labels: HPX500 / HPX2300 / Wideband HD.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.label(
            RichText::new("← HPX500 ")
                .color(Color32::from_gray(100))
                .small(),
        );
        ui.label(
            RichText::new("HPX2300 ")
                .color(Color32::from_gray(100))
                .small(),
        );
        ui.label(
            RichText::new("Wideband HD →")
                .color(Color32::from_gray(100))
                .small(),
        );
    });
}

// ---------------------------------------------------------------------------
// BER trend
// ---------------------------------------------------------------------------

/// Small line plot of rolling ECC-rate history (newest on the right).
pub fn draw_ber_trend(ui: &mut Ui, st: &PanelState) {
    ui.label(RichText::new("ECC Rate (2 min)").strong());
    if st.ber_history.len() < 2 {
        ui.label(RichText::new("collecting…").color(Color32::DARK_GRAY));
        return;
    }
    let len = st.ber_history.len();
    // Oldest sample is at the back; plot left-to-right oldest→newest.
    let points: PlotPoints = st
        .ber_history
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &v)| [i as f64, v as f64 * 100.0])
        .collect();
    Plot::new("ber_trend")
        .height(60.0)
        .include_y(0.0)
        .include_y(10.0)
        .include_x(0.0)
        .include_x((len - 1) as f64)
        .x_axis_label("s ago")
        .y_axis_label("ECC %")
        .show_axes([false, true])
        .show(ui, |plot_ui| {
            plot_ui.line(Line::new(points).color(Color32::from_rgb(220, 140, 40)));
        });
}

// ---------------------------------------------------------------------------
// Session status grid
// ---------------------------------------------------------------------------

/// Right pane: HPX state, rate ladder, session stats, BER trend.
pub fn draw_session_status(ui: &mut Ui, st: &PanelState) {
    ui.vertical(|ui| {
        draw_hpx_state(ui, st);
        ui.add_space(4.0);
        draw_rate_ladder(ui, st);
        ui.add_space(4.0);

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

        ui.add_space(4.0);
        draw_ber_trend(ui, st);
    });
}

fn kv(ui: &mut Ui, key: &str, val: &str) {
    ui.label(RichText::new(key).color(Color32::GRAY));
    ui.label(val);
    ui.end_row();
}

// ---------------------------------------------------------------------------
// Event log
// ---------------------------------------------------------------------------

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
