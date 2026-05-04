use egui::Ui;
use egui_plot::{Line, Plot, PlotPoints};
use openpulse_channel::dsp::{FREQ_BINS, WATERFALL_ROWS};

use crate::colormap::plasma;
use crate::state::{AppConfig, AppState, NoiseModel, Tap, ALL_MODES};

const SPECTRUM_H: f32 = 130.0;
const WATERFALL_H: f32 = 200.0;

// ── Toolbar ───────────────────────────────────────────────────────────────────

pub fn draw_toolbar(
    ui: &mut Ui,
    state: &mut AppState,
    on_run: impl FnOnce(),
    on_stop: impl FnOnce(),
) {
    ui.horizontal(|ui| {
        if state.running {
            if ui.button("■ Stop").clicked() {
                on_stop();
            }
        } else if ui.button("▶ Run").clicked() {
            on_run();
        }

        ui.separator();

        ui.label("Mode:");
        egui::ComboBox::from_id_salt("mode_combo")
            .selected_text(&state.config.mode)
            .show_ui(ui, |ui| {
                for &mode in ALL_MODES {
                    ui.selectable_value(&mut state.config.mode, mode.into(), mode);
                }
            });

        ui.separator();

        ui.label("Noise:");
        egui::ComboBox::from_id_salt("noise_combo")
            .selected_text(state.config.noise_model.label())
            .show_ui(ui, |ui| {
                for model in NoiseModel::all() {
                    ui.selectable_value(
                        &mut state.config.noise_model,
                        model.clone(),
                        model.label(),
                    );
                }
            });

        ui.separator();

        ui.label("SNR:");
        ui.add(
            egui::Slider::new(&mut state.config.snr_db, -30.0..=30.0)
                .suffix(" dB")
                .step_by(0.5),
        );

        ui.separator();

        ui.checkbox(&mut state.config.fec_enabled, "FEC");

        ui.separator();

        ui.label("Seed:");
        ui.add(egui::TextEdit::singleline(&mut state.config.seed_str).desired_width(60.0));

        ui.separator();

        ui.label("dB range:");
        ui.add(
            egui::Slider::new(&mut state.config.min_db, -140.0..=-20.0)
                .suffix(" min")
                .step_by(5.0),
        );
        ui.add(
            egui::Slider::new(&mut state.config.max_db, -40.0..=10.0)
                .suffix(" max")
                .step_by(5.0),
        );
    });
}

// ── Statistics bar ────────────────────────────────────────────────────────────

pub fn draw_stats(ui: &mut Ui, state: &AppState) {
    let stats = state.stats.read().unwrap();
    ui.horizontal(|ui| {
        ui.label(format!("Runs: {}", stats.runs));
        ui.separator();
        ui.label(format!("OK: {}", stats.ok));
        ui.separator();
        ui.colored_label(
            if stats.fail > 0 {
                egui::Color32::LIGHT_RED
            } else {
                egui::Color32::GRAY
            },
            format!("Fail: {}", stats.fail),
        );
        ui.separator();
        ui.label(format!("BER: {:.4}", stats.ber()));
        ui.separator();

        // Event log: last 5 entries inline
        if let Some(last) = stats.event_log.back() {
            ui.label(format!("Last event: {last}"));
        }
    });
}

// ── Signal path panel ─────────────────────────────────────────────────────────

pub fn draw_signal_panel(
    ui: &mut Ui,
    label: &str,
    tap: &Tap,
    texture: &mut Option<egui::TextureHandle>,
    last_gen: &mut u64,
    config: &AppConfig,
) {
    let (spectrum, gen) = {
        let t = tap.read().unwrap();
        (t.latest_spectrum.clone(), t.generation)
    };

    ui.heading(label);

    // Spectrum line plot
    let plot_points: PlotPoints = spectrum
        .iter()
        .enumerate()
        .map(|(i, &db)| {
            let freq = i as f64 * 8000.0 / FREQ_BINS as f64;
            [freq, db as f64]
        })
        .collect();

    Plot::new(format!("spectrum_{label}"))
        .height(SPECTRUM_H)
        .allow_zoom(false)
        .allow_drag(false)
        .include_y(config.min_db as f64)
        .include_y(config.max_db as f64)
        .x_axis_label("Hz")
        .y_axis_label("dBFS")
        .show(ui, |plot_ui| {
            plot_ui.line(Line::new(plot_points).color(egui::Color32::from_rgb(100, 200, 100)));
        });

    // Waterfall texture
    if gen != *last_gen {
        let t = tap.read().unwrap();
        let image = build_waterfall_image(&t.waterfall, config.min_db, config.max_db);
        *texture = Some(ui.ctx().load_texture(
            format!("wf_{label}"),
            image,
            egui::TextureOptions::default(),
        ));
        *last_gen = gen;
    }

    if let Some(tex) = texture.as_ref() {
        let size = egui::vec2(ui.available_width(), WATERFALL_H);
        ui.add(egui::Image::new(egui::load::SizedTexture::new(
            tex.id(),
            size,
        )));
    } else {
        ui.allocate_space(egui::vec2(ui.available_width(), WATERFALL_H));
    }
}

fn build_waterfall_image(
    wf: &openpulse_channel::dsp::WaterfallBuffer,
    _min_db: f32,
    _max_db: f32,
) -> egui::ColorImage {
    let rows = wf.rows();
    let n_rows = rows.len();
    let mut pixels = Vec::with_capacity(FREQ_BINS * WATERFALL_ROWS);

    // Texture top = oldest; texture bottom = newest.
    // rows[0] = newest, rows[n_rows-1] = oldest.
    for tex_row in 0..WATERFALL_ROWS {
        // data_age=0 is newest (tex_row=WATERFALL_ROWS-1)
        let data_age = WATERFALL_ROWS - 1 - tex_row;
        if data_age < n_rows {
            for &intensity in &rows[data_age] {
                pixels.push(plasma(intensity));
            }
        } else {
            for _ in 0..FREQ_BINS {
                pixels.push(egui::Color32::BLACK);
            }
        }
    }

    egui::ColorImage {
        size: [FREQ_BINS, WATERFALL_ROWS],
        pixels,
    }
}
