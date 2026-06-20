use egui::Ui;
use egui_plot::{Line, Plot, PlotPoints, Points, VLine};
use openpulse_channel::dsp::{FREQ_BINS, WATERFALL_ROWS};
use openpulse_core::compression::{CompressionAlgorithm, ZSTD_DICT_ID};
use openpulse_core::fec::FecMode;
use openpulse_core::profile::SessionProfile;

use crate::colormap::plasma;
use crate::state::{fec_locked, AppConfig, AppState, AudioSource, NoiseModel, Tap, ALL_MODES};

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
                        AudioSource::VirtualLoop,
                        "Virtual loop",
                    );
                    ui.selectable_value(
                        &mut state.config.audio_source,
                        AudioSource::TestMatrix,
                        "Test matrix",
                    );
                    ui.selectable_value(
                        &mut state.config.audio_source,
                        AudioSource::AdaptiveLadder,
                        "Adaptive ladder",
                    );
                    #[cfg(feature = "cpal")]
                    ui.selectable_value(
                        &mut state.config.audio_source,
                        AudioSource::LiveCapture,
                        "Live Audio",
                    );
                    #[cfg(feature = "cpal")]
                    ui.selectable_value(
                        &mut state.config.audio_source,
                        AudioSource::HardwareLoop,
                        "Hardware loop",
                    );
                })
                .response
                .on_hover_text(
                    "Synthetic: direct-plugin frame through a simulated channel\n\
                     Virtual loop: two real ModemEngines through ChannelSimHarness (testmatrix path)\n\
                     Live Audio: captured directly from one sound card\n\
                     Hardware loop: modulate out one card, capture from another (dual-card)",
                );
        });

        // Profile selector — drives the adaptive-ladder demo.
        if state.config.audio_source == AudioSource::AdaptiveLadder {
            ui.add_enabled_ui(!state.running, |ui| {
                ui.label("Profile:");
                egui::ComboBox::from_id_salt("profile_combo")
                    .selected_text(&state.config.profile)
                    .show_ui(ui, |ui| {
                        for &name in SessionProfile::PROFILE_NAMES {
                            ui.selectable_value(&mut state.config.profile, name.into(), name);
                        }
                    })
                    .response
                    .on_hover_text(
                        "Adaptive speed-level ladder to demonstrate — the mode steps up/down \
                         against the SNR slider using this profile's floor/ceiling thresholds",
                    );
            });
            ui.separator();
        }

        // Capture-device selector — for live audio and the hardware-loop RX side.
        #[cfg(feature = "cpal")]
        if matches!(
            state.config.audio_source,
            AudioSource::LiveCapture | AudioSource::HardwareLoop
        ) {
            ui.add_enabled_ui(!state.running, |ui| {
                ui.label("In:");
                let devices = state.input_devices.clone();
                let mut sel = state.config.input_device.clone();
                let sel_text = sel.clone().unwrap_or_else(|| "(default)".to_string());
                egui::ComboBox::from_id_salt("device_combo")
                    .selected_text(sel_text)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut sel, None, "(default)");
                        for dev in &devices {
                            ui.selectable_value(&mut sel, Some(dev.clone()), dev);
                        }
                    })
                    .response
                    .on_hover_text(
                        "Capture device — choose aloop_rx to scope the virtual loopback, \
                         the second card for the dual-card hardware loop, or your radio's input",
                    );
                state.config.input_device = sel;
                if ui
                    .small_button("⟳")
                    .on_hover_text("Re-scan input devices")
                    .clicked()
                {
                    state.refresh_input_devices();
                }
            });
        }

        // Playback-device selector — only the hardware loop transmits audio.
        #[cfg(feature = "cpal")]
        if state.config.audio_source == AudioSource::HardwareLoop {
            ui.add_enabled_ui(!state.running, |ui| {
                ui.label("Out:");
                let devices = state.output_devices.clone();
                let mut sel = state.config.output_device.clone();
                let sel_text = sel.clone().unwrap_or_else(|| "(default)".to_string());
                egui::ComboBox::from_id_salt("out_device_combo")
                    .selected_text(sel_text)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut sel, None, "(default)");
                        for dev in &devices {
                            ui.selectable_value(&mut sel, Some(dev.clone()), dev);
                        }
                    })
                    .response
                    .on_hover_text(
                        "Playback device for the TX side of the dual-card hardware loop",
                    );
                state.config.output_device = sel;
                if ui
                    .small_button("⟳")
                    .on_hover_text("Re-scan output devices")
                    .clicked()
                {
                    state.refresh_output_devices();
                }
            });
        }
        ui.separator();

        // Test matrix and adaptive ladder drive the mode/FEC themselves, so those pickers
        // are disabled. (The ladder still uses the SNR slider, so channel controls stay.)
        let mode_driven = matches!(
            state.config.audio_source,
            AudioSource::TestMatrix | AudioSource::AdaptiveLadder
        );

        ui.label("Mode:");
        ui.add_enabled_ui(!mode_driven, |ui| {
            egui::ComboBox::from_id_salt("mode_combo")
                .selected_text(&state.config.mode)
                .show_ui(ui, |ui| {
                    for &mode in ALL_MODES {
                        ui.selectable_value(&mut state.config.mode, mode.into(), mode);
                    }
                })
                .response
                .on_hover_text(
                    "Modulation mode and symbol rate (BPSK = binary PSK, QPSK = quadrature PSK)",
                );
        });

        // Real-audio sources (live capture / hardware loop) and the test matrix have no
        // user-set simulated channel, compression, or payload-size knob. The synthetic,
        // virtual-loop and adaptive-ladder paths do (the ladder is driven by the SNR slider).
        #[cfg(feature = "cpal")]
        let is_live = matches!(
            state.config.audio_source,
            AudioSource::LiveCapture | AudioSource::HardwareLoop
        );
        #[cfg(not(feature = "cpal"))]
        let is_live = false;
        let is_live = is_live || state.config.audio_source == AudioSource::TestMatrix;

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

        // OFDM / SC-FDMA carry FEC on the engine (virtual-loop) path but not the
        // direct-plugin synthetic / live / hardware path; FSK4-ACK never carries FEC.
        let engine_path = matches!(
            state.config.audio_source,
            AudioSource::VirtualLoop | AudioSource::TestMatrix | AudioSource::AdaptiveLadder
        );
        let fec_incompatible = fec_locked(&state.config.mode, engine_path);
        if fec_incompatible {
            state.config.fec_mode = FecMode::None;
        }
        ui.add_enabled_ui(!fec_incompatible && !mode_driven, |ui| {
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
                    "FEC unavailable for OFDM / SC-FDMA on the direct-plugin path — their\n\
                     padded byte counts are not multiples of 255 (required by the RS block\n\
                     decoder). Switch the source to Virtual loop, where the engine frames\n\
                     the payload and FEC works."
                } else {
                    "Forward error correction mode\n\
                     RS(255,223): corrects up to 16 byte errors/block (rate ≈ 87.5 %)\n\
                     RS(255,191) Strong: corrects up to 32 byte errors/block (rate ≈ 74.9 %)\n\
                     Soft-Conv+RS: K=7 soft Viterbi inner + RS outer (rate ≈ 43.7 %)"
                });
        });

        // Cap payload so one frame's modulated audio stays within a watchable duration —
        // otherwise slow modes (e.g. BPSK31) generate minutes of samples and stall.
        let mode_rate = state
            .rate_table
            .get(&state.config.mode)
            .copied()
            .unwrap_or(0.0);
        let payload_cap = max_payload_for_mode(mode_rate);
        state.config.payload_size = state.config.payload_size.min(payload_cap);

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
            ui.add(
                egui::Slider::new(&mut state.config.payload_size, 1..=payload_cap)
                    .suffix(" B")
                    .logarithmic(true),
            )
            .on_hover_text(format!(
                "Payload size in bytes per simulated frame\n\
                 Larger payloads are more compressible with LZ4/Zstd\n\
                 Maximum ({payload_cap} B) is capped per mode so one frame's audio stays\n\
                 short — slow modes generate far more samples per byte",
            ));
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

/// Largest payload (bytes) that keeps one frame's modulated audio within a watchable
/// duration, so slow modes (e.g. BPSK31) don't stall the signal thread at large payloads.
/// `rate_bps` is the mode's measured steady-state payload rate (0 = unknown → no cap).
fn max_payload_for_mode(rate_bps: f64) -> usize {
    const MAX_AUDIO_SEC: f64 = 8.0;
    if rate_bps <= 0.0 {
        return 2048;
    }
    let cap = (MAX_AUDIO_SEC * rate_bps / 8.0) as usize;
    cap.clamp(16, 2048)
}

fn mode_symbol_rate_hz(mode: &str) -> f64 {
    match mode {
        "BPSK31" => return 31.25,
        "BPSK63" => return 62.5,
        "FSK4-ACK" => return 100.0,
        _ => {}
    }
    // The baud is the last integer run in the name (constellation digits like 16/32/64
    // come first; the trailing -RRC/-HF/-P4 suffixes carry no further symbol-rate digits).
    // e.g. QPSK1000-HF-RRC → 1000, PILOT-16QAM500-RRC → 500, 64QAM2000-RRC → 2000.
    // OFDM/SC-FDMA modes use their own bandwidth special-case and never reach here.
    let mut last: Option<f64> = None;
    let mut cur = String::new();
    for ch in mode.chars() {
        if ch.is_ascii_digit() {
            cur.push(ch);
        } else if !cur.is_empty() {
            last = cur.parse().ok();
            cur.clear();
        }
    }
    if !cur.is_empty() {
        last = cur.parse().ok();
    }
    last.unwrap_or(250.0)
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
        FecMode::LdpcHighRate => "LDPC HR (8/9)",
        FecMode::Turbo => "Turbo (1/3)",
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
        FecMode::LdpcHighRate => gross * 1024.0 / 1152.0,
        FecMode::Turbo => gross / 3.0,
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
        FecMode::LdpcHighRate => {
            "Payload bit rate after high-rate LDPC overhead (code rate ≈ 88.9 %)"
        }
        FecMode::Turbo => "Payload bit rate after rate-1/3 Turbo overhead (code rate ≈ 33.3 %)",
    }
}

pub fn draw_stats(ui: &mut Ui, state: &AppState) {
    let stats = state.stats.read().unwrap();
    let fec_active = state.config.fec_mode != FecMode::None;
    if let Some(label) = &stats.matrix_current {
        ui.horizontal(|ui| {
            ui.strong("Status:");
            ui.label(egui::RichText::new(label).color(egui::Color32::from_rgb(120, 200, 255)));
        });
    }
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

        // Use the actually-running mode/FEC (set by the signal thread) so the rate is
        // correct even when it differs from the UI selection, e.g. during a matrix sweep.
        let rate_mode = stats
            .active_mode
            .clone()
            .unwrap_or_else(|| state.config.mode.clone());
        let rate_fec = if stats.active_mode.is_some() {
            stats.active_fec
        } else {
            state.config.fec_mode
        };
        let gross = state.rate_table.get(&rate_mode).copied().unwrap_or(0.0);
        let net = net_bps(gross, rate_fec);
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
            .on_hover_text(fec_net_tooltip(rate_fec));
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

// ── Signal-path cells (one column of the 2×4 grid) ────────────────────────────

/// Draw one spectrum cell at a fixed height (top grid row).
pub fn draw_spectrum_cell(ui: &mut Ui, label: &str, tap: &Tap, config: &AppConfig, height: f32) {
    let spectrum = tap.read().unwrap().latest_spectrum.clone();
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
        .height(height)
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
            let m = config.mode.as_str();
            let bw_half = if m == "FSK4-ACK" {
                150.0
            } else if m.starts_with("OFDM16") || m.starts_with("SCFDMA16") {
                312.5
            } else if m.starts_with("SCFDMA26") {
                507.8 // half-width SC-FDMA (26 subcarriers)
            } else if m.starts_with("OFDM52") || m.starts_with("SCFDMA52") {
                1015.6
            } else if m.starts_with("8PSK") || m.contains("-HF") {
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
}

/// Draw one waterfall cell at a fixed height (bottom grid row).
///
/// The texture is painted into an explicitly allocated rect, so it renders
/// deterministically inside the row's known-visible area.
pub fn draw_waterfall_cell(
    ui: &mut Ui,
    label: &str,
    tap: &Tap,
    texture: &mut Option<egui::TextureHandle>,
    last_gen: &mut u64,
    config: &AppConfig,
    height: f32,
) {
    let gen = tap.read().unwrap().generation;
    if gen != *last_gen || texture.is_none() {
        let t = tap.read().unwrap();
        let image = build_waterfall_image(&t.waterfall, config.min_db, config.max_db);
        match texture {
            Some(tex) => tex.set(image, egui::TextureOptions::LINEAR),
            None => {
                *texture = Some(ui.ctx().load_texture(
                    format!("wf_{label}"),
                    image,
                    egui::TextureOptions::LINEAR,
                ));
            }
        }
        *last_gen = gen;
    }

    let wf_size = egui::vec2(ui.available_width(), height);
    let (wf_rect, _) = ui.allocate_exact_size(wf_size, egui::Sense::hover());
    if let Some(tex) = texture.as_ref() {
        ui.painter().image(
            tex.id(),
            wf_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    } else {
        ui.painter().rect_filled(wf_rect, 0.0, egui::Color32::BLACK);
    }
}

/// Draw one IQ-scatter cell at a fixed height (used for the RX column only).
pub fn draw_scatter_cell(ui: &mut Ui, label: &str, tap: &Tap, height: f32) {
    let iq: Vec<[f64; 2]> = {
        let t = tap.read().unwrap();
        t.iq_symbols
            .iter()
            .map(|&(i, q)| [i as f64, q as f64])
            .collect()
    };
    Plot::new(format!("scatter_{label}"))
        .height(height)
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
