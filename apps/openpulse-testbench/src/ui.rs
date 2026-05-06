use egui::Ui;
use egui_plot::{Line, Plot, PlotPoints, VLine};
use openpulse_channel::dsp::{FREQ_BINS, WATERFALL_ROWS};
use openpulse_core::compression::CompressionAlgorithm;

use crate::colormap::plasma;
#[cfg(feature = "cpal")]
use crate::state::AudioSource;
use crate::state::{AppConfig, AppState, NoiseModel, Tap, ALL_MODES};

const SPECTRUM_H: f32 = 170.0;
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
            if ui
                .button("■ Stop")
                .on_hover_text("Stop the running simulation")
                .clicked()
            {
                on_stop();
            }
        } else if ui
            .button("▶ Run")
            .on_hover_text("Start the signal simulation loop")
            .clicked()
        {
            on_run();
        }

        ui.separator();

        #[cfg(feature = "cpal")]
        {
            ui.add_enabled_ui(!state.running, |ui| {
                ui.label("Source:");
                egui::ComboBox::from_id_salt("source_combo")
                    .selected_text(state.config.audio_source.label())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut state.config.audio_source,
                            AudioSource::Synthetic,
                            "Synthetic",
                        );
                        ui.selectable_value(
                            &mut state.config.audio_source,
                            AudioSource::LiveCapture,
                            "Live Audio",
                        );
                    })
                    .response
                    .on_hover_text(
                        "Synthetic: generated test frame through a simulated channel\n\
                         Live Audio: captured directly from your sound card",
                    );
            });
            ui.separator();
        }

        ui.label("Mode:");
        egui::ComboBox::from_id_salt("mode_combo")
            .selected_text(&state.config.mode)
            .show_ui(ui, |ui| {
                for &mode in ALL_MODES {
                    ui.selectable_value(&mut state.config.mode, mode.into(), mode);
                }
            })
            .response
            .on_hover_text("Modulation mode and symbol rate (BPSK = binary PSK, QPSK = quadrature PSK)");

        #[cfg(feature = "cpal")]
        let is_live = state.config.audio_source == AudioSource::LiveCapture;
        #[cfg(not(feature = "cpal"))]
        let is_live = false;

        if !is_live {
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
                })
                .response
                .on_hover_text("Channel impairment model applied to the transmitted signal");

            ui.separator();

            ui.label("SNR:");
            ui.add(
                egui::Slider::new(&mut state.config.snr_db, -30.0..=30.0)
                    .suffix(" dB")
                    .step_by(0.5),
            )
            .on_hover_text("Signal-to-noise ratio applied by the channel model");

            ui.separator();

            ui.label("Seed:");
            ui.add(egui::TextEdit::singleline(&mut state.config.seed_str).desired_width(60.0))
                .on_hover_text(
                    "Random seed for deterministic noise — same seed always produces the same channel",
                );
        }

        ui.separator();

        ui.checkbox(&mut state.config.fec_enabled, "FEC")
            .on_hover_text("Enable Reed-Solomon forward error correction (rate 223/255)");

        if !is_live {
            ui.separator();

            ui.label("Compress:");
            egui::ComboBox::from_id_salt("compress_combo")
                .selected_text(match state.config.compression {
                    CompressionAlgorithm::None => "None",
                    CompressionAlgorithm::Lz4 => "LZ4",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut state.config.compression,
                        CompressionAlgorithm::None,
                        "None",
                    );
                    ui.selectable_value(
                        &mut state.config.compression,
                        CompressionAlgorithm::Lz4,
                        "LZ4",
                    );
                })
                .response
                .on_hover_text(
                    "Payload compression applied before FEC encoding and modulation\n\
                     LZ4 block format — only helps with compressible data",
                );

            ui.separator();

            ui.label("Payload:");
            ui.add(
                egui::Slider::new(&mut state.config.payload_size, 32..=2048)
                    .suffix(" B")
                    .logarithmic(true),
            )
            .on_hover_text(
                "Payload size in bytes per simulated frame\n\
                 Larger payloads are more compressible with LZ4",
            );
        }

        ui.separator();

        ui.label("dB range:");
        ui.add(
            egui::Slider::new(&mut state.config.min_db, -140.0..=-20.0)
                .suffix(" min")
                .step_by(5.0),
        )
        .on_hover_text("Bottom of the dBFS scale for spectrum plots and waterfall colour map");
        ui.add(
            egui::Slider::new(&mut state.config.max_db, -40.0..=10.0)
                .suffix(" max")
                .step_by(5.0),
        )
        .on_hover_text("Top of the dBFS scale for spectrum plots and waterfall colour map");
    });
}

// ── Statistics bar ────────────────────────────────────────────────────────────

fn gross_bps(mode: &str) -> f64 {
    match mode {
        "BPSK31" => 31.25,
        "BPSK63" => 62.5,
        "BPSK100" => 100.0,
        "BPSK250" => 250.0,
        "QPSK125" => 250.0,  // 125 baud × 2 bits/symbol
        "QPSK250" => 500.0,  // 250 baud × 2 bits/symbol
        "QPSK500" => 1000.0, // 500 baud × 2 bits/symbol
        _ => 0.0,
    }
}

fn mode_symbol_rate_hz(mode: &str) -> f64 {
    match mode {
        "BPSK31" => 31.25,
        "BPSK63" => 62.5,
        "BPSK100" => 100.0,
        "BPSK250" => 250.0,
        "QPSK125" => 125.0,
        "QPSK250" => 250.0,
        "QPSK500" => 500.0,
        _ => 250.0,
    }
}

fn fmt_bps(bps: f64) -> String {
    if bps >= 1000.0 {
        format!("{:.2} kbps", bps / 1000.0)
    } else {
        format!("{:.2} bps", bps)
    }
}

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
        if stats.total_bits == 0 {
            ui.label("BER: N/A");
        } else {
            ui.label(format!("BER: {:.4}", stats.ber()));
        }
        ui.separator();

        let gross = gross_bps(&state.config.mode);
        // RS(255,223): code rate = 223/255 ≈ 87.5 %
        let net = if state.config.fec_enabled {
            gross * 223.0 / 255.0
        } else {
            gross
        };
        // Effective: net × compression advantage × frame success rate over the last 1.5 s.
        // compress_adv > 1 when compression shrinks the payload, < 1 when it expands it.
        let window_total = stats.rate_window.len();
        let window_ok = stats.rate_window.iter().filter(|(_, b)| *b > 0).count();
        let success_rate = if window_total > 0 {
            window_ok as f64 / window_total as f64
        } else {
            0.0
        };
        let compress_adv = 1.0 / stats.last_compress_ratio.max(f64::MIN_POSITIVE);
        let effective_bps = net * compress_adv * success_rate;

        ui.label(format!("Gross: {}", fmt_bps(gross)))
            .on_hover_text("Raw air-link bit rate (symbol rate × bits per symbol)");
        ui.separator();
        ui.label(format!("Net: {}", fmt_bps(net)))
            .on_hover_text(if state.config.fec_enabled {
                "Payload bit rate after RS(255,223) FEC overhead (code rate ≈ 87.5 %)"
            } else {
                "Payload bit rate — equal to gross when FEC is off"
            });
        ui.separator();
        ui.label(format!("Effective: {}", fmt_bps(effective_bps)))
            .on_hover_text(format!(
                "Net rate × compression advantage × frame success rate (last 1.5 s)\n\
                 Compression advantage last run: {:.2}×",
                compress_adv
            ));
        ui.separator();

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

    // Spectrum line plot
    let plot_points: PlotPoints = spectrum
        .iter()
        .enumerate()
        .map(|(i, &db)| {
            // FREQ_BINS = FFT_SIZE/2 positive bins; max frequency = sample_rate/2 = 4000 Hz
            let freq = i as f64 * 4000.0 / FREQ_BINS as f64;
            [freq, db as f64]
        })
        .collect();

    Plot::new(format!("spectrum_{label}"))
        .height(SPECTRUM_H)
        .allow_zoom(false)
        .allow_drag(false)
        .include_x(0.0)
        .include_x(4000.0)
        .include_y(config.min_db as f64)
        .include_y(config.max_db as f64)
        .x_axis_label("Hz")
        .y_axis_label("dBFS")
        .label_formatter(|_name, value| format!("{:.0} Hz\n{:.1} dBFS", value.x, value.y))
        .show(ui, |plot_ui| {
            plot_ui.line(Line::new(plot_points).color(egui::Color32::from_rgb(100, 200, 100)));

            // Bandwidth markers at the Hann-window null-to-null main-lobe bandwidth
            // (±2 × baud_rate).  Hann sidelobes at ±3×, ±5× … Rs are visible on the
            // spectrum but are ≥ −31 dB below the main lobe peak.
            let sr = mode_symbol_rate_hz(&config.mode);
            let bw_color = egui::Color32::from_rgba_unmultiplied(255, 180, 50, 160);
            plot_ui.vline(VLine::new(1500.0 - 2.0 * sr).color(bw_color).name("BW"));
            plot_ui.vline(VLine::new(1500.0 + 2.0 * sr).color(bw_color).name("BW"));
        });

    // Waterfall texture
    if gen != *last_gen {
        let t = tap.read().unwrap();
        let image = build_waterfall_image(&t.waterfall, config.min_db, config.max_db);
        match texture {
            Some(tex) => tex.set(image, egui::TextureOptions::default()),
            None => {
                *texture = Some(ui.ctx().load_texture(
                    format!("wf_{label}"),
                    image,
                    egui::TextureOptions::default(),
                ));
            }
        }
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
