//! Live two-station link visualizer.
//!
//! Side-by-side **Station A | Channel | Station B**: A's clean data TX on the left, the
//! noisy on-air signal in the middle, and B's FSK4 ACK response on the right. A background
//! thread runs the [`openpulse_linksim`] engine frame-by-frame; the SNR slider adjusts the
//! channel live, and the bottom plot tracks the effective two-way transfer rate over time.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;

use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints};
use openpulse_channel::dsp::{PowerSpectrum, WaterfallBuffer, FREQ_BINS, WATERFALL_ROWS};
use openpulse_core::fec::FecMode;
use openpulse_core::profile::SessionProfile;
use openpulse_linksim::{ChannelSpec, FrameStep, LinkParams, LinkSim};

const MIN_DB: f32 = -100.0;
const MAX_DB: f32 = 0.0;
const HIST: usize = 600; // rolling plot history (frames)
const WINDOW: usize = 24; // windowed-throughput averaging window (frames)

#[derive(Clone, Copy, PartialEq)]
enum ChannelKind {
    Clean,
    Awgn,
    WattersonGood,
    WattersonModerate,
    WattersonPoor,
    GilbertElliott,
}

impl ChannelKind {
    const ALL: [ChannelKind; 6] = [
        ChannelKind::Clean,
        ChannelKind::Awgn,
        ChannelKind::WattersonGood,
        ChannelKind::WattersonModerate,
        ChannelKind::WattersonPoor,
        ChannelKind::GilbertElliott,
    ];
    fn label(self) -> &'static str {
        match self {
            ChannelKind::Clean => "Clean",
            ChannelKind::Awgn => "AWGN",
            ChannelKind::WattersonGood => "Watterson Good-F1",
            ChannelKind::WattersonModerate => "Watterson Moderate-F1",
            ChannelKind::WattersonPoor => "Watterson Poor-F1",
            ChannelKind::GilbertElliott => "Gilbert-Elliott",
        }
    }
    fn spec(self, snr: f32) -> ChannelSpec {
        match self {
            ChannelKind::Clean => ChannelSpec::Clean,
            ChannelKind::Awgn => ChannelSpec::Awgn(snr),
            ChannelKind::WattersonGood => ChannelSpec::WattersonGoodF1(snr),
            ChannelKind::WattersonModerate => ChannelSpec::WattersonModerateF1(snr),
            ChannelKind::WattersonPoor => ChannelSpec::WattersonPoorF1(snr),
            ChannelKind::GilbertElliott => ChannelSpec::GilbertElliott(snr),
        }
    }
}

/// Shared controls written by the UI, read by the sim thread.
struct Controls {
    /// Bumped when a structural parameter changes → thread rebuilds the sim.
    generation: u64,
    profile: String,
    channel: ChannelKind,
    snr_db: f32,
    payload: usize,
    fec: FecMode,
    turnaround: f64,
}

impl Controls {
    fn params(&self) -> LinkParams {
        LinkParams {
            profile_name: self.profile.clone(),
            forward: self.channel.spec(self.snr_db),
            reverse: self.channel.spec(self.snr_db + 5.0),
            payload_bytes_per_frame: self.payload,
            total_frames: usize::MAX, // run continuously; the UI keeps a rolling window
            fec: self.fec,
            turnaround_s: self.turnaround,
            max_attempts: 6,
            seed: 0x1234_5678,
        }
    }
}

fn spawn_sim(
    controls: Arc<Mutex<Controls>>,
    running: Arc<AtomicBool>,
) -> (mpsc::Receiver<FrameStep>, JoinHandle<()>) {
    let (tx, rx) = mpsc::channel::<FrameStep>();
    let handle = std::thread::spawn(move || {
        while running.load(Ordering::Relaxed) {
            // Snapshot params (and the generation we built against) for this run.
            let (mut sim, gen, mut chan, mut snr) = {
                let c = controls.lock().unwrap();
                (LinkSim::new(&c.params()), c.generation, c.channel, c.snr_db)
            };
            loop {
                if !running.load(Ordering::Relaxed) {
                    return;
                }
                // Apply live changes (or rebuild on a structural change).
                {
                    let c = controls.lock().unwrap();
                    if c.generation != gen {
                        break; // structural change → rebuild on the next outer iteration
                    }
                    if c.channel != chan || (c.snr_db - snr).abs() > f32::EPSILON {
                        chan = c.channel;
                        snr = c.snr_db;
                        sim.set_conditions(chan.spec(snr), chan.spec(snr + 5.0));
                    }
                }
                match sim.step() {
                    Some(fs) => {
                        if tx.send(fs).is_err() {
                            return;
                        }
                    }
                    None => break, // continuous run shouldn't hit this (total_frames = MAX)
                }
                std::thread::sleep(std::time::Duration::from_millis(40));
            }
        }
    });
    (rx, handle)
}

/// One visualized signal column.
struct PanelView {
    ps: PowerSpectrum,
    wf: WaterfallBuffer,
    spectrum: Vec<f32>,
    generation: u64,
    tex: Option<egui::TextureHandle>,
    last_gen: u64,
}

impl PanelView {
    fn new() -> Self {
        Self {
            ps: PowerSpectrum::new(),
            wf: WaterfallBuffer::new(WATERFALL_ROWS),
            spectrum: vec![MIN_DB; FREQ_BINS],
            generation: 0,
            tex: None,
            last_gen: u64::MAX,
        }
    }
    fn push(&mut self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        let spec = self.ps.compute(samples);
        self.wf.push(&spec, MIN_DB, MAX_DB);
        self.spectrum = spec;
        self.generation += 1;
    }
}

struct LinkApp {
    controls: Arc<Mutex<Controls>>,
    running: Arc<AtomicBool>,
    rx: Option<mpsc::Receiver<FrameStep>>,
    handle: Option<JoinHandle<()>>,

    // UI-editable mirror of the controls.
    ui_profile: String,
    ui_channel: ChannelKind,
    ui_snr: f32,
    ui_payload: usize,
    ui_fec: FecMode,
    ui_turnaround: f64,

    panels: [PanelView; 3], // 0 = A TX, 1 = Channel, 2 = B ACK
    last: Option<FrameStep>,
    // rolling history (frame index, windowed eff bps, snr, level)
    eff_hist: VecDeque<[f64; 2]>,
    snr_hist: VecDeque<[f64; 2]>,
    level_hist: VecDeque<[f64; 2]>,
    window: VecDeque<(usize, f64)>, // (delivered_bits, air_s) for windowed rate
    frame_counter: usize,
    delivered: usize,
    attempted: usize,
}

impl LinkApp {
    fn new() -> Self {
        Self {
            controls: Arc::new(Mutex::new(Controls {
                generation: 0,
                profile: "hpx_hf".into(),
                channel: ChannelKind::Awgn,
                snr_db: 15.0,
                payload: 64,
                fec: FecMode::Rs,
                turnaround: 0.25,
            })),
            running: Arc::new(AtomicBool::new(false)),
            rx: None,
            handle: None,
            ui_profile: "hpx_hf".into(),
            ui_channel: ChannelKind::Awgn,
            ui_snr: 15.0,
            ui_payload: 64,
            ui_fec: FecMode::Rs,
            ui_turnaround: 0.25,
            panels: [PanelView::new(), PanelView::new(), PanelView::new()],
            last: None,
            eff_hist: VecDeque::new(),
            snr_hist: VecDeque::new(),
            level_hist: VecDeque::new(),
            window: VecDeque::new(),
            frame_counter: 0,
            delivered: 0,
            attempted: 0,
        }
    }

    fn start(&mut self) {
        self.stop();
        self.reset_history();
        {
            let mut c = self.controls.lock().unwrap();
            c.generation = c.generation.wrapping_add(1);
            c.profile = self.ui_profile.clone();
            c.channel = self.ui_channel;
            c.snr_db = self.ui_snr;
            c.payload = self.ui_payload;
            c.fec = self.ui_fec;
            c.turnaround = self.ui_turnaround;
        }
        self.running.store(true, Ordering::Relaxed);
        let (rx, handle) = spawn_sim(Arc::clone(&self.controls), Arc::clone(&self.running));
        self.rx = Some(rx);
        self.handle = Some(handle);
    }

    fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        self.rx = None;
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }

    fn reset_history(&mut self) {
        self.eff_hist.clear();
        self.snr_hist.clear();
        self.level_hist.clear();
        self.window.clear();
        self.frame_counter = 0;
        self.delivered = 0;
        self.attempted = 0;
        self.last = None;
    }

    /// Push UI changes to the shared controls. SNR is live; structural fields bump generation.
    fn sync_controls(&mut self) {
        let mut c = self.controls.lock().unwrap();
        c.snr_db = self.ui_snr;
        c.channel = self.ui_channel;
        let structural_changed = c.profile != self.ui_profile
            || c.payload != self.ui_payload
            || c.fec != self.ui_fec
            || (c.turnaround - self.ui_turnaround).abs() > f64::EPSILON;
        if structural_changed {
            c.profile = self.ui_profile.clone();
            c.payload = self.ui_payload;
            c.fec = self.ui_fec;
            c.turnaround = self.ui_turnaround;
            c.generation = c.generation.wrapping_add(1);
        }
    }

    fn drain(&mut self) {
        let mut steps = Vec::new();
        if let Some(rx) = &self.rx {
            while let Ok(fs) = rx.try_recv() {
                steps.push(fs);
            }
        }
        for fs in steps {
            self.panels[0].push(&fs.forward_tx);
            self.panels[1].push(&fs.forward_rx);
            self.panels[2].push(&fs.ack_tx);

            self.frame_counter += 1;
            self.attempted += 1;
            if fs.delivered {
                self.delivered += 1;
            }
            // Windowed effective rate.
            self.window
                .push_back((fs.delivered_bytes * 8, fs.frame_air_s));
            while self.window.len() > WINDOW {
                self.window.pop_front();
            }
            let (bits, air): (usize, f64) = self
                .window
                .iter()
                .fold((0, 0.0), |(b, a), &(fb, fa)| (b + fb, a + fa));
            let eff = if air > 0.0 { bits as f64 / air } else { 0.0 };
            let x = self.frame_counter as f64;
            push_hist(&mut self.eff_hist, [x, eff]);
            push_hist(&mut self.snr_hist, [x, fs.est_snr_db as f64]);
            push_hist(&mut self.level_hist, [x, fs.level as f64]);
            self.last = Some(fs);
        }
    }
}

fn push_hist(buf: &mut VecDeque<[f64; 2]>, p: [f64; 2]) {
    buf.push_back(p);
    while buf.len() > HIST {
        buf.pop_front();
    }
}

fn fmt_bps(bps: f64) -> String {
    if bps >= 1000.0 {
        format!("{:.2} kbps", bps / 1000.0)
    } else {
        format!("{bps:.0} bps")
    }
}

fn plasma(t: u8) -> egui::Color32 {
    const STOPS: &[(f32, f32, f32, f32)] = &[
        (0.000, 0.050, 0.030, 0.527),
        (0.143, 0.459, 0.017, 0.655),
        (0.286, 0.679, 0.008, 0.736),
        (0.429, 0.839, 0.152, 0.706),
        (0.571, 0.953, 0.325, 0.592),
        (0.714, 0.989, 0.553, 0.349),
        (0.857, 0.992, 0.761, 0.141),
        (1.000, 0.940, 0.975, 0.131),
    ];
    let v = t as f32 / 255.0;
    let i = ((v * (STOPS.len() - 1) as f32) as usize).min(STOPS.len() - 2);
    let (x0, r0, g0, b0) = STOPS[i];
    let (x1, r1, g1, b1) = STOPS[i + 1];
    let f = ((v - x0) / (x1 - x0).max(1e-9)).clamp(0.0, 1.0);
    egui::Color32::from_rgb(
        ((r0 + (r1 - r0) * f) * 255.0) as u8,
        ((g0 + (g1 - g0) * f) * 255.0) as u8,
        ((b0 + (b1 - b0) * f) * 255.0) as u8,
    )
}

fn waterfall_image(wf: &WaterfallBuffer) -> egui::ColorImage {
    let rows = wf.rows();
    let n = rows.len();
    let mut pixels = Vec::with_capacity(FREQ_BINS * WATERFALL_ROWS);
    for tex_row in 0..WATERFALL_ROWS {
        let age = WATERFALL_ROWS - 1 - tex_row;
        if age < n {
            for &v in &rows[age] {
                pixels.push(plasma(v));
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

fn draw_panel(
    ui: &mut egui::Ui,
    title: &str,
    subtitle: &str,
    panel: &mut PanelView,
    spectrum_h: f32,
    waterfall_h: f32,
) {
    ui.vertical(|ui| {
        ui.strong(title);
        ui.label(egui::RichText::new(subtitle).weak());
        let pts: PlotPoints = panel
            .spectrum
            .iter()
            .enumerate()
            .map(|(i, &db)| [i as f64 * 4000.0 / FREQ_BINS as f64, db as f64])
            .collect();
        Plot::new(format!("spec_{title}"))
            .height(spectrum_h)
            .allow_zoom(false)
            .allow_drag(false)
            .include_x(0.0)
            .include_x(4000.0)
            .include_y(MIN_DB as f64)
            .include_y(MAX_DB as f64)
            .show(ui, |p| {
                p.line(Line::new(pts).color(egui::Color32::from_rgb(100, 200, 100)));
            });

        if panel.generation != panel.last_gen || panel.tex.is_none() {
            let img = waterfall_image(&panel.wf);
            match &mut panel.tex {
                Some(t) => t.set(img, egui::TextureOptions::LINEAR),
                None => {
                    panel.tex = Some(ui.ctx().load_texture(
                        format!("wf_{title}"),
                        img,
                        egui::TextureOptions::LINEAR,
                    ))
                }
            }
            panel.last_gen = panel.generation;
        }
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), waterfall_h),
            egui::Sense::hover(),
        );
        if let Some(t) = &panel.tex {
            ui.painter().image(
                t.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
    });
}

impl eframe::App for LinkApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.running.load(Ordering::Relaxed) {
            self.sync_controls();
            self.drain();
            ctx.request_repaint();
        }

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                let is_running = self.running.load(Ordering::Relaxed);
                if is_running {
                    if ui.button("■ Stop").clicked() {
                        self.stop();
                    }
                } else if ui.button("▶ Run").clicked() {
                    self.start();
                }
                ui.separator();
                ui.label("Profile:");
                egui::ComboBox::from_id_salt("profile")
                    .selected_text(&self.ui_profile)
                    .show_ui(ui, |ui| {
                        for &p in SessionProfile::PROFILE_NAMES {
                            ui.selectable_value(&mut self.ui_profile, p.into(), p);
                        }
                    });
                ui.separator();
                ui.label("Channel:");
                egui::ComboBox::from_id_salt("channel")
                    .selected_text(self.ui_channel.label())
                    .show_ui(ui, |ui| {
                        for k in ChannelKind::ALL {
                            ui.selectable_value(&mut self.ui_channel, k, k.label());
                        }
                    });
                ui.separator();
                ui.label("SNR:");
                ui.add(egui::Slider::new(&mut self.ui_snr, -10.0..=35.0).suffix(" dB"));
                ui.separator();
                ui.label("FEC:");
                egui::ComboBox::from_id_salt("fec")
                    .selected_text(format!("{:?}", self.ui_fec))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.ui_fec, FecMode::None, "None");
                        ui.selectable_value(&mut self.ui_fec, FecMode::Rs, "RS");
                        ui.selectable_value(&mut self.ui_fec, FecMode::RsStrong, "RS Strong");
                        ui.selectable_value(&mut self.ui_fec, FecMode::SoftConcatenated, "Soft");
                    });
                ui.separator();
                ui.label("Payload:");
                ui.add(egui::Slider::new(&mut self.ui_payload, 8..=512).suffix(" B"));
                ui.separator();
                ui.label("Turnaround:");
                ui.add(egui::Slider::new(&mut self.ui_turnaround, 0.0..=1.0).suffix(" s"));
            });
        });

        egui::TopBottomPanel::bottom("stats")
            .min_height(180.0)
            .show(ctx, |ui| {
                if let Some(fs) = &self.last {
                    ui.horizontal(|ui| {
                        let (bits, air): (usize, f64) = self
                            .window
                            .iter()
                            .fold((0, 0.0), |(b, a), &(fb, fa)| (b + fb, a + fa));
                        let eff = if air > 0.0 { bits as f64 / air } else { 0.0 };
                        ui.strong(format!("Effective: {}", fmt_bps(eff)));
                        ui.separator();
                        ui.label(format!("Frame {}", self.frame_counter));
                        ui.separator();
                        let dr = if self.attempted > 0 {
                            100.0 * self.delivered as f64 / self.attempted as f64
                        } else {
                            0.0
                        };
                        ui.label(format!("Delivered: {dr:.0}%"));
                        ui.separator();
                        ui.label(format!("SL{} {}", fs.level, fs.mode));
                        ui.separator();
                        ui.label(format!("est SNR {:.1} dB", fs.est_snr_db));
                        ui.separator();
                        ui.label(format!(
                            "ACK: {:?} (heard {:?})",
                            fs.ack_sent, fs.ack_received
                        ));
                        if fs.attempts > 1 {
                            ui.separator();
                            ui.colored_label(
                                egui::Color32::LIGHT_RED,
                                format!("{} attempts", fs.attempts),
                            );
                        }
                    });
                } else {
                    ui.label("Press ▶ Run to start the link.");
                }

                let eff: PlotPoints = self.eff_hist.iter().copied().collect();
                let lvl: PlotPoints = self
                    .level_hist
                    .iter()
                    .map(|&[x, y]| [x, y * 200.0])
                    .collect();
                Plot::new("eff_plot")
                    .height(120.0)
                    .allow_zoom(false)
                    .allow_drag(false)
                    .y_axis_label("bps")
                    .show(ui, |p| {
                        p.line(
                            Line::new(eff)
                                .color(egui::Color32::from_rgb(80, 180, 255))
                                .name("effective bps"),
                        );
                        p.line(
                            Line::new(lvl)
                                .color(egui::Color32::from_rgba_unmultiplied(255, 180, 50, 120))
                                .name("speed level ×200"),
                        );
                    });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            let col_w = ui.available_width() / 3.0;
            let spectrum_h = (ui.available_height() * 0.42).clamp(120.0, 320.0);
            let waterfall_h = (ui.available_height() * 0.42).clamp(100.0, 320.0);
            let subtitle = self
                .last
                .as_ref()
                .map(|fs| (fs.mode.clone(), fs.delivered, fs.ack_sent))
                .unwrap_or_else(|| ("—".into(), false, openpulse_core::ack::AckType::AckOk));
            ui.horizontal(|ui| {
                ui.allocate_ui(egui::vec2(col_w, ui.available_height()), |ui| {
                    draw_panel(
                        ui,
                        "Station A → data TX",
                        &format!("{} (clean)", subtitle.0),
                        &mut self.panels[0],
                        spectrum_h,
                        waterfall_h,
                    );
                });
                ui.allocate_ui(egui::vec2(col_w, ui.available_height()), |ui| {
                    draw_panel(
                        ui,
                        "Channel (on air)",
                        "forward signal + noise",
                        &mut self.panels[1],
                        spectrum_h,
                        waterfall_h,
                    );
                });
                ui.allocate_ui(egui::vec2(col_w, ui.available_height()), |ui| {
                    let decoded = if subtitle.1 {
                        "decoded OK"
                    } else {
                        "decode FAIL"
                    };
                    draw_panel(
                        ui,
                        "Station B ← ACK TX",
                        &format!("{decoded} → ACK {:?}", subtitle.2),
                        &mut self.panels[2],
                        spectrum_h,
                        waterfall_h,
                    );
                });
            });
        });
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("OpenPulse Two-Station Link Simulator")
            .with_inner_size([1500.0, 900.0]),
        ..Default::default()
    };
    eframe::run_native(
        "openpulse-linksim-gui",
        options,
        Box::new(|_cc| Ok(Box::new(LinkApp::new()))),
    )
}
