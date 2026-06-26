//! Live two-station link visualizer.
//!
//! Side-by-side **Station A | Channel | Station B**: A's clean data TX on the left, the
//! noisy on-air signal in the middle, and B's FSK4 ACK response on the right. A background
//! thread runs the [`openpulse_linksim`] engine frame-by-frame; the SNR slider adjusts the
//! channel live, and the bottom strip plots the effective transfer rate and the estimated
//! SNR over time, side by side.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use eframe::egui;
use egui_plot::{Line, Plot, PlotPoint, PlotPoints, Text};
use openpulse_audio::CpalBackend;
use openpulse_channel::dsp::{PowerSpectrum, WaterfallBuffer, FREQ_BINS, WATERFALL_ROWS};
use openpulse_core::audio::{AudioBackend, AudioConfig, AudioOutputStream};
use openpulse_core::compression::{CompressionAlgorithm, ZSTD_DICT_ID};
use openpulse_core::fec::FecMode;
use openpulse_core::profile::SessionProfile;
use openpulse_linksim::{ChannelSpec, FrameStep, LinkParams, LinkSim};

const MIN_DB: f32 = -100.0;
const MAX_DB: f32 = 0.0;
const HIST: usize = 600; // rolling plot history (frames)
const WINDOW: usize = 24; // windowed-throughput averaging window (frames)
/// SNR slider bounds (dB) — shared by the slider and the randomize feature.
const SNR_MIN: f32 = -10.0;
const SNR_MAX: f32 = 35.0;
/// TX-audio monitor: cap on the buffered lead (s). Each frame writes only enough to top
/// the buffer up to this, so the audio stays within this much of the spectrum instead of
/// falling seconds behind on long (slow-mode) bursts.
const AUDIO_BUFFER_TARGET_S: f64 = 0.2;
/// TX-audio monitor playback gain (the modem waveform is near full-scale; keep it kind).
const AUDIO_MONITOR_GAIN: f32 = 0.5;

#[derive(Clone, Copy, PartialEq)]
enum ChannelKind {
    Clean,
    Awgn,
    WattersonGood,
    WattersonModerate,
    WattersonPoor,
    GilbertElliott,
    FlatFading,
}

impl ChannelKind {
    const ALL: [ChannelKind; 7] = [
        ChannelKind::Clean,
        ChannelKind::Awgn,
        ChannelKind::WattersonGood,
        ChannelKind::WattersonModerate,
        ChannelKind::WattersonPoor,
        ChannelKind::GilbertElliott,
        ChannelKind::FlatFading,
    ];
    fn label(self) -> &'static str {
        match self {
            ChannelKind::Clean => "Clean",
            ChannelKind::Awgn => "AWGN",
            ChannelKind::WattersonGood => "Watterson Good-F1",
            ChannelKind::WattersonModerate => "Watterson Moderate-F1",
            ChannelKind::WattersonPoor => "Watterson Poor-F1",
            ChannelKind::GilbertElliott => "Gilbert-Elliott",
            ChannelKind::FlatFading => "Flat Fading (1 Hz)",
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
            ChannelKind::FlatFading => ChannelSpec::FlatFading(snr),
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
    compression: CompressionAlgorithm,
    turnaround: f64,
    cessb: bool,
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
            compression: self.compression,
            turnaround_s: self.turnaround,
            max_attempts: 6,
            seed: 0x1234_5678,
            cessb_enabled: self.cessb,
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
            let (mut sim, gen, mut chan, mut snr, mut cessb) = {
                let c = controls.lock().unwrap_or_else(|e| e.into_inner());
                (
                    LinkSim::new(&c.params()),
                    c.generation,
                    c.channel,
                    c.snr_db,
                    c.cessb,
                )
            };
            loop {
                if !running.load(Ordering::Relaxed) {
                    return;
                }
                // Apply live changes (or rebuild on a structural change).
                {
                    let c = controls.lock().unwrap_or_else(|e| e.into_inner());
                    if c.generation != gen {
                        break; // structural change → rebuild on the next outer iteration
                    }
                    if c.channel != chan || (c.snr_db - snr).abs() > f32::EPSILON {
                        chan = c.channel;
                        snr = c.snr_db;
                        sim.set_conditions(chan.spec(snr), chan.spec(snr + 5.0));
                    }
                    if c.cessb != cessb {
                        cessb = c.cessb;
                        sim.set_cessb(cessb);
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
    ui_compression: CompressionAlgorithm,
    ui_turnaround: f64,
    ui_cessb: bool,

    // Randomize-SNR feature: when on, the applied SNR jumps to a new random value in
    // [slider, SNR_MAX] every 1–5 s. The slider stays put and acts as the floor.
    ui_randomize: bool,
    random_snr: f32,
    next_snr_change: Option<Instant>,
    rng_state: u64,

    // TX-audio monitor: copy the forward (data TX) waveform to the default playback device.
    ui_audio_monitor: bool,
    audio_out: Option<Box<dyn AudioOutputStream>>,
    audio_written: u64,
    audio_start: Instant,

    panels: [PanelView; 3], // 0 = A TX, 1 = B RX (decoded), 2 = ACK
    last: Option<FrameStep>,
    // rolling history (frame index, windowed eff bps, snr, level)
    eff_hist: VecDeque<[f64; 2]>,
    snr_hist: VecDeque<[f64; 2]>,
    level_hist: VecDeque<[f64; 2]>,
    window: VecDeque<(f64, f64)>, // (forward_air_s, total_air_s) → half-duplex duty cycle
    frame_counter: usize,
    delivered: usize,
    attempted: usize,
    // Current bitrate readout (testbench-style, computed by the sim): Gross = mode rate,
    // Net = Gross × code rate, Effective = Net × compression advantage × success. Goodput =
    // measured two-way rate.
    disp_gross: f64,
    disp_net: f64,
    disp_eff: f64,
    disp_goodput: f64,

    /// Fan-out hub feeding connected `openpulse-panel` clients (when launched with `--serve`).
    #[cfg(feature = "serve")]
    hub: Option<openpulse_linksim::serve::FrameHub>,
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
                compression: CompressionAlgorithm::None,
                turnaround: 0.25,
                cessb: true,
            })),
            running: Arc::new(AtomicBool::new(false)),
            rx: None,
            handle: None,
            ui_profile: "hpx_hf".into(),
            ui_channel: ChannelKind::Awgn,
            ui_snr: 15.0,
            ui_payload: 64,
            ui_fec: FecMode::Rs,
            ui_compression: CompressionAlgorithm::None,
            ui_turnaround: 0.25,
            ui_cessb: true,
            ui_randomize: false,
            random_snr: 15.0,
            next_snr_change: None,
            ui_audio_monitor: false,
            audio_out: None,
            audio_written: 0,
            audio_start: Instant::now(),
            // Seed the PRNG from wall-clock nanos so successive launches differ.
            rng_state: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x9E37_79B9_7F4A_7C15)
                | 1,
            panels: [PanelView::new(), PanelView::new(), PanelView::new()],
            last: None,
            eff_hist: VecDeque::new(),
            snr_hist: VecDeque::new(),
            level_hist: VecDeque::new(),
            window: VecDeque::new(),
            frame_counter: 0,
            delivered: 0,
            attempted: 0,
            disp_gross: 0.0,
            disp_net: 0.0,
            disp_eff: 0.0,
            disp_goodput: 0.0,
            #[cfg(feature = "serve")]
            hub: None,
        }
    }

    fn start(&mut self) {
        self.stop();
        self.reset_history();
        {
            let mut c = self.controls.lock().unwrap_or_else(|e| e.into_inner());
            c.generation = c.generation.wrapping_add(1);
            c.profile = self.ui_profile.clone();
            c.channel = self.ui_channel;
            c.snr_db = self.ui_snr;
            c.payload = self.ui_payload;
            c.fec = self.ui_fec;
            c.compression = self.ui_compression;
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
        self.audio_out = None; // stop monitoring TX audio when the sim stops
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

    /// Copy a frame's forward (data TX) waveform to the default playback device when the
    /// monitor is on. Opens the cpal output lazily and drops frames when more than
    /// [`AUDIO_BUFFER_TARGET_S`] is already buffered, so it stays near real time.
    fn feed_audio_monitor(&mut self, fs: &FrameStep) {
        if !self.ui_audio_monitor {
            self.audio_out = None; // closed while off
            return;
        }
        if self.audio_out.is_none() {
            match CpalBackend::new().open_output(None, &AudioConfig::default()) {
                Ok(stream) => {
                    self.audio_out = Some(stream);
                    self.audio_written = 0;
                    self.audio_start = Instant::now();
                }
                Err(_) => {
                    // No usable output device — turn the toggle back off rather than retry.
                    self.ui_audio_monitor = false;
                    return;
                }
            }
        }
        if fs.forward_tx.is_empty() {
            return;
        }
        // Keep audio in step with the spectrum: cap the buffered lead at AUDIO_BUFFER_TARGET_S.
        // A single TX burst can be seconds long, so write only enough to top the buffer up to
        // the target and drop the rest — otherwise the audio falls seconds behind the visuals.
        let sr = AudioConfig::default().sample_rate as f64;
        let lead_s = self.audio_written as f64 / sr - self.audio_start.elapsed().as_secs_f64();
        let room_s = AUDIO_BUFFER_TARGET_S - lead_s;
        if room_s <= 0.0 {
            return;
        }
        let n = fs.forward_tx.len().min((room_s * sr) as usize);
        if n == 0 {
            return;
        }
        let scaled: Vec<f32> = fs.forward_tx[..n]
            .iter()
            .map(|&s| (s * AUDIO_MONITOR_GAIN).clamp(-1.0, 1.0))
            .collect();
        if let Some(out) = &mut self.audio_out {
            if out.write(&scaled).is_ok() {
                self.audio_written += n as u64;
            }
        }
    }

    /// A uniform random number in [0, 1) from an internal SplitMix64 (no external dep).
    fn rand_unit(&mut self) -> f32 {
        self.rng_state = self.rng_state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.rng_state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        // Top 24 bits → a float in [0, 1).
        ((z >> 40) as f32) / ((1u32 << 24) as f32)
    }

    /// When "Randomize" is on, jump the applied SNR to a fresh random value every 1–5 s,
    /// never below the slider's current value (the slider acts as the floor and stays put).
    fn tick_randomize(&mut self, now: Instant) {
        if !self.ui_randomize {
            self.next_snr_change = None;
            self.random_snr = self.ui_snr; // mirror the floor while idle
            return;
        }
        if self.next_snr_change.is_none_or(|t| now >= t) {
            let snr = self.ui_snr + self.rand_unit() * (SNR_MAX - self.ui_snr);
            self.random_snr = (snr * 2.0).round() / 2.0; // 0.5 dB steps
            let secs = 1.0 + self.rand_unit() as f64 * 4.0; // 1–5 s
            self.next_snr_change = Some(now + Duration::from_secs_f64(secs));
        } else if self.random_snr < self.ui_snr {
            // Slider raised above the current draw between ticks → honor the floor now.
            self.random_snr = self.ui_snr;
        }
    }

    /// Push UI changes to the shared controls. SNR is live; structural fields bump generation.
    fn sync_controls(&mut self) {
        let mut c = self.controls.lock().unwrap_or_else(|e| e.into_inner());
        // When randomizing, apply the random draw (≥ the slider floor); otherwise the slider.
        c.snr_db = if self.ui_randomize {
            self.random_snr
        } else {
            self.ui_snr
        };
        c.channel = self.ui_channel;
        c.cessb = self.ui_cessb; // live — applied without a rebuild (like SNR)
        let structural_changed = c.profile != self.ui_profile
            || c.payload != self.ui_payload
            || c.fec != self.ui_fec
            || c.compression != self.ui_compression
            || (c.turnaround - self.ui_turnaround).abs() > f64::EPSILON;
        if structural_changed {
            c.profile = self.ui_profile.clone();
            c.payload = self.ui_payload;
            c.fec = self.ui_fec;
            c.compression = self.ui_compression;
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
            // Feed any connected openpulse-panel clients from the same live frame.
            #[cfg(feature = "serve")]
            if let Some(h) = &self.hub {
                h.publish(&fs);
            }

            self.panels[0].push(&fs.forward_tx);
            self.panels[1].push(&fs.forward_rx);
            // The return ACK as heard back at A (post reverse channel) — using the noisy RX
            // (not the clean TX) so the waterfall actually moves frame-to-frame.
            self.panels[2].push(&fs.ack_rx);

            self.frame_counter += 1;
            self.attempted += 1;
            if fs.delivered {
                self.delivered += 1;
            }
            self.window.push_back((fs.forward_air_s, fs.frame_air_s));
            while self.window.len() > WINDOW {
                self.window.pop_front();
            }
            // Rates come from the sim (the same values fed to the panel, so the two windows
            // display identical Gross / Net / Effective figures).
            self.disp_gross = fs.gross_bps;
            self.disp_net = fs.net_bps;
            self.disp_eff = fs.effective_bps;
            // Two-way goodput is DERIVED from the Effective (forward) rate, derated by the
            // half-duplex duty cycle: forward air time / total air time (total includes the
            // ACK frame and turnaround). Windowed so it tracks the recent ACK/turnaround mix.
            let (win_fwd, win_total): (f64, f64) = self
                .window
                .iter()
                .fold((0.0, 0.0), |(f, t), &(ff, ft)| (f + ff, t + ft));
            let duty = if win_total > 0.0 {
                win_fwd / win_total
            } else {
                0.0
            };
            self.disp_goodput = self.disp_eff * duty;
            let x = self.frame_counter as f64;
            push_hist(&mut self.eff_hist, [x, self.disp_eff]);
            push_hist(&mut self.snr_hist, [x, fs.est_snr_db as f64]);
            push_hist(&mut self.level_hist, [x, fs.level as f64]);
            self.feed_audio_monitor(&fs);
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
        // Newest row (rows[0]) at the top so the waterfall scrolls top-to-bottom; older rows
        // (and the empty tail before the buffer fills) fall below.
        let age = tex_row;
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
        // Randomize the SNR (if enabled) before syncing, so a new value reaches the sim this frame.
        self.tick_randomize(Instant::now());
        if self.running.load(Ordering::Relaxed) {
            self.sync_controls();
            self.drain();
            ctx.request_repaint();
        } else if self.ui_randomize {
            // Keep the timer alive while idle so the randomized SNR still updates every 1–5 s.
            ctx.request_repaint_after(Duration::from_millis(200));
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
                ui.checkbox(&mut self.ui_randomize, "🎲 Randomize")
                    .on_hover_text(
                        "Jump the SNR to a new random value every 1–5 seconds, never below \
                         the SNR slider (which acts as the floor).",
                    );
                ui.checkbox(&mut self.ui_audio_monitor, "🔊 TX audio")
                    .on_hover_text(
                        "Copy the forward (data TX) waveform to the default playback device. \
                         Off by default.",
                    );
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
                ui.label(if self.ui_randomize {
                    "SNR floor:"
                } else {
                    "SNR:"
                });
                ui.add(egui::Slider::new(&mut self.ui_snr, SNR_MIN..=SNR_MAX).suffix(" dB"));
                if self.ui_randomize {
                    ui.label(
                        egui::RichText::new(format!("🎲 {:.1} dB", self.random_snr))
                            .color(egui::Color32::from_rgb(0x4c, 0xaf, 0x50)),
                    )
                    .on_hover_text("Current randomized SNR applied to the channel.");
                }
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
                ui.label("Compress:");
                egui::ComboBox::from_id_salt("compress")
                    .selected_text(match self.ui_compression {
                        CompressionAlgorithm::None => "None",
                        CompressionAlgorithm::Lz4 => "LZ4",
                        CompressionAlgorithm::Zstd(_) => "Zstd",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.ui_compression,
                            CompressionAlgorithm::None,
                            "None",
                        );
                        ui.selectable_value(
                            &mut self.ui_compression,
                            CompressionAlgorithm::Lz4,
                            "LZ4",
                        );
                        ui.selectable_value(
                            &mut self.ui_compression,
                            CompressionAlgorithm::Zstd(ZSTD_DICT_ID),
                            "Zstd",
                        );
                    });
                ui.separator();
                ui.label("Payload:");
                ui.add(egui::Slider::new(&mut self.ui_payload, 8..=512).suffix(" B"));
                ui.separator();
                ui.label("Turnaround:");
                ui.add(egui::Slider::new(&mut self.ui_turnaround, 0.0..=1.0).suffix(" s"));
                ui.separator();
                // CE-SSB TX conditioning toggle (live, like SNR). Only acts on the modes
                // ModemEngine::cessb_benefits enables (OFDM QPSK/8PSK) — a no-op elsewhere.
                let (cessb_label, cessb_color) = if self.ui_cessb {
                    ("CE-SSB: ON", egui::Color32::from_rgb(0x4c, 0xaf, 0x50))
                } else {
                    ("CE-SSB: OFF", egui::Color32::DARK_GRAY)
                };
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(cessb_label).color(egui::Color32::WHITE),
                        )
                        .fill(cessb_color),
                    )
                    .on_hover_text(
                        "CE-SSB TX envelope conditioning (average-power gain at fixed peak).\n\
                         Only affects OFDM QPSK/8PSK; a no-op on other modes.",
                    )
                    .clicked()
                {
                    self.ui_cessb = !self.ui_cessb;
                }

                #[cfg(feature = "serve")]
                if let Some(h) = &self.hub {
                    let n = h.client_count();
                    let (color, text) = if n > 0 {
                        (egui::Color32::GREEN, format!("● panel ×{n}"))
                    } else {
                        (egui::Color32::DARK_GRAY, "○ panel".to_string())
                    };
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(text).color(color))
                            .on_hover_text("Connected openpulse-panel clients");
                    });
                }
            });
        });

        egui::TopBottomPanel::bottom("stats")
            .min_height(180.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!("Gross: {}", fmt_bps(self.disp_gross)))
                        .on_hover_text(
                            "Current mode's raw payload bit rate (symbol rate × bits/symbol)",
                        );
                    ui.separator();
                    ui.label(format!("Net: {}", fmt_bps(self.disp_net)))
                        .on_hover_text("Gross minus FEC overhead (gross × code rate)");
                    ui.separator();
                    // Effective (testbench-style): Net × compression advantage × frame success.
                    ui.label(
                        egui::RichText::new(format!("Effective: {}", fmt_bps(self.disp_eff)))
                            .strong()
                            .size(16.0)
                            .color(egui::Color32::from_rgb(120, 220, 255)),
                    )
                    .on_hover_text(
                        "Net × compression advantage × frame success rate.\n\
                         (Same definition as the testbench's Effective.)",
                    );
                    ui.separator();
                    ui.label(format!("2-way: {}", fmt_bps(self.disp_goodput)))
                        .on_hover_text(
                            "Two-way goodput, derived from Effective × half-duplex duty cycle \
                             (forward air time / total air time, where total includes the ACK \
                             frame and turnaround).",
                        );
                    ui.separator();
                    if let Some(fs) = &self.last {
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
                    } else {
                        ui.label("Press ▶ Run to start the link.");
                    }
                });

                // Bitrate (left) and SNR (right) share the bottom strip. SNR is in dB on a very
                // different scale than bps, so it gets its own plot/axis rather than being
                // squashed onto the bitrate axis.
                ui.columns(2, |cols| {
                    let eff: PlotPoints = self.eff_hist.iter().copied().collect();
                    let lvl: PlotPoints = self
                        .level_hist
                        .iter()
                        .map(|&[x, y]| [x, y * 200.0])
                        .collect();
                    let mode = self
                        .last
                        .as_ref()
                        .map(|fs| fs.mode.clone())
                        .unwrap_or_default();
                    Plot::new("eff_plot")
                        .height(120.0)
                        .allow_zoom(false)
                        .allow_drag(false)
                        .y_axis_label("bps")
                        .show(&mut cols[0], |p| {
                            // Current mode as a faint watermark behind the traces.
                            if !mode.is_empty() {
                                let b = p.plot_bounds();
                                let cx = (b.min()[0] + b.max()[0]) * 0.5;
                                let cy = (b.min()[1] + b.max()[1]) * 0.5;
                                p.text(
                                    Text::new(
                                        PlotPoint::new(cx, cy),
                                        egui::RichText::new(&mode)
                                            .size(34.0)
                                            .color(egui::Color32::from_gray(90)),
                                    )
                                    .name(""),
                                );
                            }
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

                    let snr: PlotPoints = self.snr_hist.iter().copied().collect();
                    Plot::new("snr_plot")
                        .height(120.0)
                        .allow_zoom(false)
                        .allow_drag(false)
                        .y_axis_label("SNR (dB)")
                        .include_y(0.0)
                        .show(&mut cols[1], |p| {
                            p.line(
                                Line::new(snr)
                                    .color(egui::Color32::from_rgb(120, 230, 120))
                                    .name("est SNR dB"),
                            );
                        });
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
            let decoded = if subtitle.1 {
                "decoded OK"
            } else {
                "decode FAIL"
            };
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
                    // The decoded-data view: B's received signal (mode-dependent + noise), which
                    // is what actually gets demodulated — so the "decoded" status belongs here.
                    draw_panel(
                        ui,
                        "Station B ← data RX",
                        &format!("{} + noise · {decoded}", subtitle.0),
                        &mut self.panels[1],
                        spectrum_h,
                        waterfall_h,
                    );
                });
                ui.allocate_ui(egui::vec2(col_w, ui.available_height()), |ui| {
                    // The ACK is always FSK4-ACK regardless of the data mode (by design), so this
                    // column is intentionally mode-invariant — labelled FSK4 to make that clear.
                    draw_panel(
                        ui,
                        "ACK (B→A, FSK4)",
                        &format!("ACK {:?} + noise", subtitle.2),
                        &mut self.panels[2],
                        spectrum_h,
                        waterfall_h,
                    );
                });
            });
        });
    }
}

/// Parse `--serve <ADDR>` (or `--serve=<ADDR>`) and, if present, start the panel server
/// thread. Returns the [`FrameHub`] the GUI publishes into.
#[cfg(feature = "serve")]
fn start_panel_server() -> Option<openpulse_linksim::serve::FrameHub> {
    use openpulse_linksim::serve::{serve_hub, FrameHub};
    let mut args = std::env::args().skip(1);
    let mut addr = None;
    while let Some(a) = args.next() {
        if a == "--serve" {
            addr = args.next();
        } else if let Some(rest) = a.strip_prefix("--serve=") {
            addr = Some(rest.to_string());
        }
    }
    let addr = addr?;
    eprintln!("linksim-gui: serving panel protocol on {addr} — connect openpulse-panel there");
    let hub = FrameHub::new();
    let h = hub.clone();
    std::thread::spawn(move || {
        if let Err(e) = serve_hub(&addr, h) {
            eprintln!("linksim-gui: panel server error: {e}");
        }
    });
    Some(hub)
}

fn main() -> eframe::Result<()> {
    #[cfg(feature = "serve")]
    let hub = start_panel_server();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("OpenPulse Two-Station Link Simulator")
            .with_inner_size([1500.0, 900.0]),
        ..Default::default()
    };
    eframe::run_native(
        "openpulse-linksim-gui",
        options,
        Box::new(move |_cc| {
            #[allow(unused_mut)]
            let mut app = LinkApp::new();
            #[cfg(feature = "serve")]
            {
                app.hub = hub;
            }
            Ok(Box::new(app))
        }),
    )
}
