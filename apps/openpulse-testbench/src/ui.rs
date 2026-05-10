use egui::Ui;
use egui_plot::{Line, Plot, PlotPoints, Points, VLine};
use openpulse_channel::dsp::{FREQ_BINS, WATERFALL_ROWS};
use openpulse_core::compression::{CompressionAlgorithm, ZSTD_DICT_ID};
use openpulse_core::fec::FecMode;

use crate::colormap::plasma;
#[cfg(feature = "cpal")]
use crate::state::AudioSource;
use crate::state::{
    fec_payload_limit, mode_fec_incompatible, AppConfig, AppState, NoiseModel, Tap, ALL_MODES,
};

const SPECTRUM_H: f32 = 170.0;
const WATERFALL_H: f32 = 200.0;
const SCATTER_H: f32 = 170.0;
const SNR_PLOT_H: f32 = 80.0;

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

        let fec_incompatible = mode_fec_incompatible(&state.config.mode);
        if fec_incompatible {
            state.config.fec_mode = FecMode::None;
        }
        ui.add_enabled_ui(!fec_incompatible, |ui| {
            ui.label("FEC:");
            egui::ComboBox::from_id_salt("fec_combo")
                .selected_text(fec_mode_label(state.config.fec_mode))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut state.config.fec_mode, FecMode::None, "None");
                    ui.selectable_value(&mut state.config.fec_mode, FecMode::Rs, "RS(255,223)");
                    ui.selectable_value(
                        &mut state.config.fec_mode,
                        FecMode::RsStrong,
                        "RS(255,191) Strong",
                    );
                    ui.selectable_value(
                        &mut state.config.fec_mode,
                        FecMode::SoftConcatenated,
                        "Soft-Conv+RS",
                    );
                })
                .response
                .on_hover_text(if state.config.mode == "FSK4-ACK" {
                    "FEC unavailable for FSK4-ACK — fixed 5-byte ACK payload has no room\n\
                     for RS parity bytes"
                } else if fec_incompatible {
                    "FEC unavailable for OFDM / SC-FDMA — their internal 2-byte length\n\
                     prefix causes byte counts that are not multiples of 255, which the\n\
                     RS block decoder requires"
                } else {
                    "Forward error correction mode\n\
                     RS(255,223): corrects up to 16 byte errors/block (rate ≈ 87.5 %)\n\
                     RS(255,191) Strong: corrects up to 32 byte errors/block (rate ≈ 74.9 %)\n\
                     Soft-Conv+RS: K=7 soft Viterbi inner + RS outer (rate ≈ 43.7 %)"
                });
        });

        // Clamp payload size when FEC is active: must fit within one RS block.
        if let Some(limit) = fec_payload_limit(state.config.fec_mode) {
            state.config.payload_size = state.config.payload_size.min(limit);
        }

        if !is_live {
            ui.separator();

            ui.label("Compress:");
            egui::ComboBox::from_id_salt("compress_combo")
                .selected_text(match state.config.compression {
                    CompressionAlgorithm::None => "None",
                    CompressionAlgorithm::Lz4 => "LZ4",
                    CompressionAlgorithm::Zstd(_) => "Zstd",
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
                    ui.selectable_value(
                        &mut state.config.compression,
                        CompressionAlgorithm::Zstd(ZSTD_DICT_ID),
                        "Zstd",
                    );
                })
                .response
                .on_hover_text(
                    "Payload compression applied before FEC encoding and modulation\n\
                     LZ4: block format, fast; Zstd: higher ratio, slightly slower\n\
                     Both only help with compressible data",
                );

            ui.separator();

            ui.label("Payload:");
            let payload_max = fec_payload_limit(state.config.fec_mode).unwrap_or(2048);
            ui.add(
                egui::Slider::new(&mut state.config.payload_size, 1..=payload_max)
                    .suffix(" B")
                    .logarithmic(true),
            )
            .on_hover_text(
                "Payload size in bytes per simulated frame\n\
                 Larger payloads are more compressible with LZ4/Zstd\n\
                 Maximum capped at 219 B (RS / Soft-Conv+RS) or 187 B (RS Strong) when\n\
                 FEC is active — RS block data capacity minus 4-byte length prefix",
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
        "QPSK125" => 250.0,      // 125 baud × 2 bits/symbol
        "QPSK250" => 500.0,      // 250 baud × 2 bits/symbol
        "QPSK500" => 1000.0,     // 500 baud × 2 bits/symbol
        "QPSK1000" => 2000.0,    // 1000 baud × 2 bits/symbol
        "QPSK1000-HF" => 2000.0, // same rate, narrower BW via cosine overlap
        "8PSK500" => 1500.0,     // 500 baud × 3 bits/symbol
        "8PSK1000" => 3000.0,    // 1000 baud × 3 bits/symbol
        "8PSK1000-HF" => 3000.0, // same rate, narrower BW via cosine overlap
        "BPSK250-RRC" => 250.0,
        "QPSK500-RRC" => 1000.0,
        "QPSK1000-RRC" => 2000.0,
        "8PSK500-RRC" => 1500.0,
        "8PSK1000-RRC" => 3000.0,
        "FSK4-ACK" => 200.0, // 100 baud × 2 bits/symbol (4-tone)
        // OFDM/SC-FDMA: n_data × 2 bits × (8000 / 288) symbol/s
        "OFDM16" | "SCFDMA16" => 889.0, // 16 × 2 × 8000/288 ≈ 889 bps
        "OFDM52" | "SCFDMA52" => 2889.0, // 52 × 2 × 8000/288 ≈ 2889 bps
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
        "QPSK1000" | "QPSK1000-HF" => 1000.0,
        "8PSK500" => 500.0,
        "8PSK1000" | "8PSK1000-HF" => 1000.0,
        "BPSK250-RRC" => 250.0,
        "QPSK500-RRC" => 500.0,
        "QPSK1000-RRC" => 1000.0,
        "8PSK500-RRC" => 500.0,
        "8PSK1000-RRC" => 1000.0,
        "FSK4-ACK" => 100.0,
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

fn fec_mode_label(mode: FecMode) -> &'static str {
    match mode {
        FecMode::None => "None",
        FecMode::Rs => "RS(255,223)",
        FecMode::RsStrong => "RS(255,191) Strong",
        FecMode::SoftConcatenated => "Soft-Conv+RS",
        FecMode::RsInterleaved => "RS+Interleave",
        FecMode::Concatenated => "Conv+RS",
        FecMode::ShortRs => "Short RS",
        FecMode::Ldpc => "LDPC",
    }
}

/// Net bitrate after FEC overhead (gross × code rate).
fn net_bps(gross: f64, fec_mode: FecMode) -> f64 {
    match fec_mode {
        FecMode::None => gross,
        FecMode::Rs | FecMode::RsInterleaved => gross * 223.0 / 255.0,
        FecMode::RsStrong => gross * 191.0 / 255.0,
        // Conv rate-1/2 inner + RS(255,223) outer
        FecMode::Concatenated | FecMode::SoftConcatenated => gross * 223.0 / 255.0 * 0.5,
        FecMode::ShortRs => gross,
        FecMode::Ldpc => gross,
    }
}

fn fec_net_tooltip(fec_mode: FecMode) -> &'static str {
    match fec_mode {
        FecMode::None => "Payload bit rate — equal to gross when FEC is off",
        FecMode::Rs | FecMode::RsInterleaved => {
            "Payload bit rate after RS(255,223) overhead (code rate ≈ 87.5 %)"
        }
        FecMode::RsStrong => "Payload bit rate after RS(255,191) overhead (code rate ≈ 74.9 %)",
        FecMode::Concatenated => {
            "Payload bit rate after Conv(1/2)+RS(255,223) overhead (code rate ≈ 43.7 %)"
        }
        FecMode::SoftConcatenated => {
            "Payload bit rate after Soft-Conv(1/2)+RS(255,223) overhead (code rate ≈ 43.7 %)"
        }
        FecMode::ShortRs | FecMode::Ldpc => "Payload bit rate",
    }
}

pub fn draw_stats(ui: &mut Ui, state: &AppState) {
    let stats = state.stats.read().unwrap();
    let fec_active = state.config.fec_mode != FecMode::None;
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
        let net = net_bps(gross, state.config.fec_mode);
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
            .on_hover_text(fec_net_tooltip(state.config.fec_mode));
        ui.separator();
        ui.label(format!("Effective: {}", fmt_bps(effective_bps)))
            .on_hover_text(format!(
                "Net rate × compression advantage × frame success rate (last 1.5 s)\n\
                 Compression advantage last run: {:.2}×",
                compress_adv
            ));
        ui.separator();

        if let Some(snr) = stats.current_snr_db {
            ui.label(format!("SNR: {snr:.1} dB"))
                .on_hover_text("Estimated signal-to-noise ratio (TX power / noise power)");
            ui.separator();
        }

        if let Some(last) = stats.event_log.back() {
            ui.label(format!("Last event: {last}"));
        }

        // ECC at end — only shown when FEC is active.
        if fec_active {
            ui.separator();
            match stats.fec_correction_rate() {
                Some(rate) => {
                    ui.label(format!("ECC: {:.1}%", rate * 100.0))
                        .on_hover_text(format!(
                            "Last TX: {} of {} channel bit errors corrected by FEC",
                            stats.last_fec_corrected_bits, stats.last_fec_channel_error_bits
                        ));
                }
                None => {
                    ui.label("ECC: —");
                }
            }
        }
    });

    // SNR trend plot — only shown when history is available.
    if !stats.snr_history.is_empty() {
        let now = std::time::Instant::now();
        let points: PlotPoints = stats
            .snr_history
            .iter()
            .map(|(t, snr)| {
                let age_s = now.duration_since(*t).as_secs_f64();
                [-age_s, *snr as f64]
            })
            .collect();
        Plot::new("snr_trend")
            .height(SNR_PLOT_H)
            .allow_zoom(false)
            .allow_drag(false)
            .include_y(-10.0)
            .include_y(35.0)
            .include_x(-180.0)
            .include_x(0.0)
            .x_axis_label("s ago")
            .y_axis_label("dB")
            .label_formatter(|_, v| format!("{:.0} s ago\n{:.1} dB", -v.x, v.y))
            .show(ui, |plot_ui| {
                plot_ui.line(Line::new(points).color(egui::Color32::from_rgb(80, 180, 255)));
            });
    }
}

// ── Signal path panel ─────────────────────────────────────────────────────────

pub fn draw_signal_panel(
    ui: &mut Ui,
    label: &str,
    tap: &Tap,
    texture: &mut Option<egui::TextureHandle>,
    last_gen: &mut u64,
    config: &AppConfig,
    show_scatter: bool,
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

            // Bandwidth markers (half-width from centre = 1500 Hz).
            // FSK4-ACK: 4 tones at ±50/±150 Hz → outermost edge at ±150 Hz.
            // OFDM16: SCs 38–57, edge-to-edge = 20 × 31.25 Hz = 625 Hz → half = 312.5 Hz.
            // OFDM52: SCs 16–80, edge-to-edge = 65 × 31.25 Hz = 2031 Hz → half ≈ 1016 Hz.
            // -HF / 8PSK: cosine overlap shaping, null-to-null BW ≈ 2×Rs → half = Rs.
            // Everything else: Hann windowing, null-to-null BW ≈ 4×Rs → half = 2×Rs.
            let sr = mode_symbol_rate_hz(&config.mode);
            let bw_half = if config.mode == "FSK4-ACK" {
                150.0
            } else if config.mode == "OFDM16" || config.mode == "SCFDMA16" {
                312.5
            } else if config.mode == "OFDM52" || config.mode == "SCFDMA52" {
                1015.6
            } else if config.mode.starts_with("8PSK") || config.mode.ends_with("-HF") {
                sr
            } else {
                2.0 * sr
            };
            let bw_color = egui::Color32::from_rgba_unmultiplied(255, 180, 50, 160);
            let left = (1500.0 - bw_half).max(0.0);
            let right = (1500.0 + bw_half).min(4000.0);
            plot_ui.vline(VLine::new(left).color(bw_color).name("BW"));
            plot_ui.vline(VLine::new(right).color(bw_color).name("BW"));
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

    // IQ scatter plot — shown only for the RX tap when enabled.
    if show_scatter {
        let iq: Vec<[f64; 2]> = {
            let t = tap.read().unwrap();
            t.iq_symbols
                .iter()
                .map(|&(i, q)| [i as f64, q as f64])
                .collect()
        };
        Plot::new(format!("scatter_{label}"))
            .height(SCATTER_H)
            .allow_zoom(false)
            .allow_drag(false)
            .data_aspect(1.0)
            .include_x(-2.0)
            .include_x(2.0)
            .include_y(-2.0)
            .include_y(2.0)
            .x_axis_label("I")
            .y_axis_label("Q")
            .label_formatter(|_, v| format!("I={:.2} Q={:.2}", v.x, v.y))
            .show(ui, |plot_ui| {
                if !iq.is_empty() {
                    let pts: PlotPoints = iq.into_iter().collect();
                    plot_ui.points(
                        Points::new(pts)
                            .color(egui::Color32::from_rgba_unmultiplied(255, 220, 50, 180))
                            .radius(1.5),
                    );
                }
            });
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
