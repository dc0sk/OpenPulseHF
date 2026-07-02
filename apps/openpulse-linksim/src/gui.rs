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
use egui_plot::{Line, Plot, PlotPoint, PlotPoints, Points, Text};
use openpulse_audio::CpalBackend;
use openpulse_channel::dsp::{PowerSpectrum, WaterfallBuffer, FREQ_BINS, WATERFALL_ROWS};
use openpulse_core::audio::{AudioBackend, AudioConfig, AudioOutputStream};
use openpulse_core::compression::{CompressionAlgorithm, ZSTD_DICT_ID};
use openpulse_core::fec::FecMode;
use openpulse_core::profile::SessionProfile;
use openpulse_linksim::{ChannelSpec, FrameStep, LinkParams, LinkSim};

const MIN_DB: f32 = -100.0;
const MAX_DB: f32 = 0.0;
/// Welch segments averaged for each panel's spectrum: enough to read the envelope, few enough that
/// the finite-sample variance keeps the trace visibly "breathing" over the actual modulated data.
const SPECTRUM_WELCH_SEGMENTS: usize = 6;
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
            notch: None,
        }
    }
}

/// Precomputed per-panel visualization (spectrum + I/Q scatter) for one signal column. Built on the
/// sim worker so the UI thread never runs an FFT or Hilbert transform. An empty `spectrum` means
/// "no update this frame" (the source waveform was empty), matching the old early-return.
struct PanelData {
    spectrum: Vec<f32>,
    iq: Vec<[f64; 2]>,
}

/// A completed sim frame together with its precomputed visualization DSP for the three panels.
struct FrameUpdate {
    step: FrameStep,
    panels: [PanelData; 3],
}

/// The I/Q scatter for `mode`'s waveform. Multicarrier (OFDM / SC-FDMA) runs the real receiver
/// front-end (FFT → channel-est → equalize) via the plugin to recover the true QAM constellation;
/// single-carrier modes use the passband Hilbert `baseband_iq`; FSK has no constellation. Returns
/// empty if the constellation is hidden, the mode has none, or a multicarrier frame failed to sync.
fn constellation_iq(mode: &str, samples: &[f32], sps: usize, want_iq: bool) -> Vec<[f64; 2]> {
    if !want_iq {
        return Vec::new();
    }
    let m = mode.to_ascii_uppercase();
    let to_f64 = |pts: Vec<(f32, f32)>| pts.iter().map(|&(i, q)| [i as f64, q as f64]).collect();
    if m.starts_with("FSK") {
        Vec::new()
    } else if m.starts_with("SCFDMA") {
        scfdma_plugin::demodulate::scfdma_constellation(samples, mode)
            .map(to_f64)
            .unwrap_or_default()
    } else if m.starts_with("OFDM") {
        ofdm_plugin::demodulate::ofdm_constellation(samples, mode)
            .map(to_f64)
            .unwrap_or_default()
    } else {
        baseband_iq(samples, sps)
    }
}

/// Compute one panel's spectrum + I/Q on the worker thread (reuses `ps`'s FFT planner), off the UI
/// thread. The spectrum FFT feeds the always-drawn spectrum/waterfall; the I/Q source is mode-aware
/// (see [`constellation_iq`]) and skipped entirely when `want_iq` is false.
fn compute_panel(
    ps: &mut PowerSpectrum,
    samples: &[f32],
    sps: usize,
    want_iq: bool,
    mode: &str,
) -> PanelData {
    if samples.is_empty() {
        return PanelData {
            spectrum: Vec::new(),
            iq: Vec::new(),
        };
    }
    PanelData {
        // Welch PSD over the whole burst (not just the fixed preamble window), so the trace
        // reflects the actual random modulated data and breathes naturally frame-to-frame.
        spectrum: ps.compute_welch(samples, SPECTRUM_WELCH_SEGMENTS),
        iq: constellation_iq(mode, samples, sps, want_iq),
    }
}

fn spawn_sim(
    controls: Arc<Mutex<Controls>>,
    running: Arc<AtomicBool>,
    show_constellation: Arc<AtomicBool>,
) -> (mpsc::Receiver<FrameUpdate>, JoinHandle<()>) {
    let (tx, rx) = mpsc::channel::<FrameUpdate>();
    let handle = std::thread::spawn(move || {
        // Persistent FFT planners for the three panels (TX / RX / ACK), reused across frames.
        let mut spectra = [
            PowerSpectrum::new(),
            PowerSpectrum::new(),
            PowerSpectrum::new(),
        ];
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
                        // Do the visualization DSP (FFT + I/Q) here, off the UI thread. The I/Q
                        // source is mode-aware (real equalized symbols for multicarrier, Hilbert for
                        // single-carrier, none for FSK) — see `constellation_iq`.
                        let want_iq = show_constellation.load(Ordering::Relaxed);
                        let sps = samples_per_symbol(&fs.mode).unwrap_or(0);
                        let panels = [
                            compute_panel(&mut spectra[0], &fs.forward_tx, sps, want_iq, &fs.mode),
                            compute_panel(&mut spectra[1], &fs.forward_rx, sps, want_iq, &fs.mode),
                            // Panel 2 is always the FSK4 ACK → no constellation (blanked by mode).
                            compute_panel(&mut spectra[2], &fs.ack_rx, 0, want_iq, "FSK4-ACK"),
                        ];
                        if tx.send(FrameUpdate { step: fs, panels }).is_err() {
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

/// One visualized signal column. The spectrum/I/Q DSP is done on the sim worker; this only holds
/// the latest results and the waterfall ring buffer (a cheap row push) for rendering.
struct PanelView {
    wf: WaterfallBuffer,
    spectrum: Vec<f32>,
    /// Normalized baseband I/Q scatter of the most recent waveform, for the constellation view.
    iq: Vec<[f64; 2]>,
    generation: u64,
    tex: Option<egui::TextureHandle>,
    last_gen: u64,
}

impl PanelView {
    fn new() -> Self {
        Self {
            wf: WaterfallBuffer::new(WATERFALL_ROWS),
            spectrum: vec![MIN_DB; FREQ_BINS],
            iq: Vec::new(),
            generation: 0,
            tex: None,
            last_gen: u64::MAX,
        }
    }
    /// Store precomputed panel DSP from the worker; empty spectrum = no update this frame.
    fn apply(&mut self, pd: PanelData) {
        if pd.spectrum.is_empty() {
            return;
        }
        self.wf.push(&pd.spectrum, MIN_DB, MAX_DB);
        self.spectrum = pd.spectrum;
        self.iq = pd.iq;
        self.generation += 1;
    }
}

/// Samples per symbol for a single-carrier PSK/QAM mode at 8 kHz, parsed from the mode name's
/// trailing baud number (e.g. `BPSK250` → 32, `8PSK1000` → 8, `64QAM2000-RRC` → 4). Returns `None`
/// for multicarrier/pilot/FSK modes, whose passband I/Q has no clean PSK symbol grid.
fn samples_per_symbol(mode: &str) -> Option<usize> {
    const FS: f32 = 8000.0;
    let m = mode.to_ascii_uppercase();
    if m.starts_with("OFDM")
        || m.starts_with("SCFDMA")
        || m.starts_with("PILOT")
        || m.starts_with("FSK")
    {
        return None;
    }
    // Baud = the final run of digits before any `-RRC`/`-P4` suffix (clearing on each letter keeps
    // only the trailing run, so the leading order digit of `8PSK`/`64QAM` is discarded).
    let base = m.split('-').next().unwrap_or(&m);
    let mut baud_str = String::new();
    for c in base.chars() {
        if c.is_ascii_digit() {
            baud_str.push(c);
        } else {
            baud_str.clear();
        }
    }
    let baud: f32 = baud_str.parse().ok()?;
    if baud <= 0.0 {
        return None;
    }
    let sps = (FS / baud).round() as usize;
    (sps >= 2).then_some(sps)
}

/// Normalized baseband I/Q scatter for the constellation view, via Hilbert downconversion of the
/// 1500 Hz passband at 8 kHz. The filter group-delay edges are trimmed. When `sps` (samples per
/// symbol) is ≥ 2 the cloud is sampled once per symbol at the best timing phase — discrete dots
/// (clean on TX, noise-smeared on RX); otherwise a fixed decimation of the full-rate I/Q cloud is
/// returned. Points are scaled so the RMS magnitude is ≈ 1.0.
fn baseband_iq(samples: &[f32], sps: usize) -> Vec<[f64; 2]> {
    const FC: f32 = 1500.0; // ModemEngine default center frequency
    const FS: f32 = 8000.0;
    const EDGE: usize = 31; // hilbert_iq group-delay artifact margin
    const MAX_POINTS: usize = 700;
    let (i_bb, q_bb) = openpulse_core::iq::hilbert_iq(samples, FC, FS);
    let n = i_bb.len();
    if n <= 2 * EDGE {
        return Vec::new();
    }
    let (lo, hi) = (EDGE, n - EDGE);
    let mag2 = |k: usize| (i_bb[k] as f64).powi(2) + (q_bb[k] as f64).powi(2);

    let indices: Vec<usize> = if sps >= 2 {
        // Symbol-spaced sampling at the phase whose samples carry the most energy: symbol centers
        // hold full amplitude while transitions dip, so peak mean magnitude ≈ best timing.
        let best_phase = (0..sps)
            .max_by(|&a, &b| {
                let e = |p: usize| {
                    let pts: Vec<usize> = (lo + p..hi).step_by(sps).collect();
                    let s: f64 = pts.iter().map(|&k| mag2(k)).sum();
                    if pts.is_empty() {
                        0.0
                    } else {
                        s / pts.len() as f64
                    }
                };
                e(a).total_cmp(&e(b))
            })
            .unwrap_or(0);
        (lo + best_phase..hi).step_by(sps).collect()
    } else {
        let step = ((hi - lo) / MAX_POINTS).max(1);
        (lo..hi).step_by(step).collect()
    };

    if indices.is_empty() {
        return Vec::new();
    }
    let rms = (indices.iter().map(|&k| mag2(k)).sum::<f64>() / indices.len() as f64)
        .sqrt()
        .max(1e-9);
    let step = (indices.len() / MAX_POINTS).max(1);
    indices
        .iter()
        .step_by(step)
        .map(|&k| [i_bb[k] as f64 / rms, q_bb[k] as f64 / rms])
        .collect()
}

/// A square I/Q constellation scatter (title above, fixed unit-ish bounds, no axes/grid).
fn constellation_plot(
    ui: &mut egui::Ui,
    id: &str,
    title: &str,
    points: &[[f64; 2]],
    side: f32,
    color: egui::Color32,
) {
    ui.allocate_ui(egui::vec2(side, side), |ui| {
        ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(title)
                    .size(13.0)
                    .strong()
                    .color(egui::Color32::from_gray(200)),
            );
            let pts: PlotPoints = points.iter().copied().collect();
            Plot::new(id)
                .width(side)
                .height((side - 22.0).max(40.0))
                .data_aspect(1.0)
                .show_axes([false, false])
                .show_grid(false)
                .allow_zoom(false)
                .allow_drag(false)
                .allow_scroll(false)
                .include_x(-1.8)
                .include_x(1.8)
                .include_y(-1.8)
                .include_y(1.8)
                .show(ui, |p| {
                    p.points(Points::new(pts).radius(1.2).color(color));
                });
        });
    });
}

struct LinkApp {
    controls: Arc<Mutex<Controls>>,
    running: Arc<AtomicBool>,
    /// Mirrors `ui_show_constellation` for the sim worker, so it can skip the I/Q Hilbert when the
    /// constellation view is hidden. Lock-free (read every worker frame).
    show_constellation: Arc<AtomicBool>,
    rx: Option<mpsc::Receiver<FrameUpdate>>,
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

    // Per-visualization toggles (toolbar): show the waterfalls / the I/Q constellations.
    ui_show_waterfall: bool,
    ui_show_constellation: bool,

    // Bundled OpenPulseHF QR code, decoded to a texture on first paint.
    qr_tex: Option<egui::TextureHandle>,

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
                snr_db: 5.0,
                payload: 512,
                fec: FecMode::Rs,
                compression: CompressionAlgorithm::Lz4,
                turnaround: 0.25,
                cessb: true,
            })),
            running: Arc::new(AtomicBool::new(false)),
            // Constellation off by default — its per-frame plotting (800 pts × 2 panels) and the
            // multicarrier equalize-per-frame extraction are the heaviest UI work; opt-in via toolbar.
            show_constellation: Arc::new(AtomicBool::new(false)),
            rx: None,
            handle: None,
            ui_profile: "hpx_hf".into(),
            ui_channel: ChannelKind::Awgn,
            ui_snr: 5.0,
            ui_payload: 512,
            ui_fec: FecMode::Rs,
            ui_compression: CompressionAlgorithm::Lz4,
            ui_turnaround: 0.25,
            ui_cessb: true,
            ui_randomize: true,
            random_snr: 5.0,
            next_snr_change: None,
            ui_audio_monitor: false,
            audio_out: None,
            audio_written: 0,
            audio_start: Instant::now(),
            ui_show_waterfall: true,
            ui_show_constellation: false,
            qr_tex: None,
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
        let (rx, handle) = spawn_sim(
            Arc::clone(&self.controls),
            Arc::clone(&self.running),
            Arc::clone(&self.show_constellation),
        );
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

    /// The bundled QR-code texture, decoded once on first use. NEAREST filtering keeps the
    /// QR modules crisp when scaled down. Returns `None` if the PNG fails to decode.
    fn qr_texture(&mut self, ctx: &egui::Context) -> Option<egui::TextureHandle> {
        if let Some(t) = &self.qr_tex {
            return Some(t.clone());
        }
        let bytes: &[u8] = include_bytes!("../../../docs/OpenPulseHF.png");
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .ok()?;
        let mut buf = vec![0u8; reader.output_buffer_size()?];
        let info = reader.next_frame(&mut buf).ok()?;
        let rgba = match info.color_type {
            png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
            png::ColorType::Rgb => buf[..info.buffer_size()]
                .chunks_exact(3)
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect(),
            _ => return None, // QR is RGB/RGBA; other formats aren't expected
        };
        let img = egui::ColorImage::from_rgba_unmultiplied(
            [info.width as usize, info.height as usize],
            &rgba,
        );
        let tex = ctx.load_texture("openpulse_qr", img, egui::TextureOptions::NEAREST);
        self.qr_tex = Some(tex.clone());
        Some(tex)
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
        let mut updates = Vec::new();
        if let Some(rx) = &self.rx {
            while let Ok(fu) = rx.try_recv() {
                updates.push(fu);
            }
        }
        for fu in updates {
            let FrameUpdate {
                step: fs,
                panels: [p0, p1, p2],
            } = fu;
            // Feed any connected openpulse-panel clients from the same live frame.
            #[cfg(feature = "serve")]
            if let Some(h) = &self.hub {
                h.publish(&fs);
            }

            // Panel DSP was already computed on the worker — just store it (no FFT/Hilbert here).
            self.panels[0].apply(p0);
            self.panels[1].apply(p1);
            self.panels[2].apply(p2);

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
    show_waterfall: bool,
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

        if !show_waterfall {
            return;
        }
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
        // Mirror the constellation toggle so the worker can skip the I/Q Hilbert when it's hidden.
        self.show_constellation
            .store(self.ui_show_constellation, Ordering::Relaxed);
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
                ui.checkbox(&mut self.ui_show_waterfall, "🌊 Waterfall")
                    .on_hover_text("Show the per-column waterfall under each spectrum.");
                ui.checkbox(&mut self.ui_show_constellation, "✦ Constellation")
                    .on_hover_text("Show the Station A / Station B I/Q constellation diagrams.");
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
                        ui.selectable_value(
                            &mut self.ui_fec,
                            FecMode::RsInterleaved,
                            "RS Interleaved",
                        );
                        ui.selectable_value(&mut self.ui_fec, FecMode::RsStrong, "RS Strong");
                        ui.selectable_value(
                            &mut self.ui_fec,
                            FecMode::Concatenated,
                            "Concatenated",
                        );
                        ui.selectable_value(
                            &mut self.ui_fec,
                            FecMode::SoftConcatenated,
                            "Soft (Conv+RS)",
                        );
                        ui.selectable_value(&mut self.ui_fec, FecMode::Ldpc, "LDPC r1/2");
                        ui.selectable_value(&mut self.ui_fec, FecMode::LdpcHighRate, "LDPC r8/9");
                        ui.selectable_value(&mut self.ui_fec, FecMode::Turbo, "Turbo r1/3");
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
                    let snr_label = self
                        .last
                        .as_ref()
                        .map(|fs| format!("{:.1} dB", fs.est_snr_db))
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
                            // Current SNR as a faint watermark behind the trace.
                            if !snr_label.is_empty() {
                                let b = p.plot_bounds();
                                let cx = (b.min()[0] + b.max()[0]) * 0.5;
                                let cy = (b.min()[1] + b.max()[1]) * 0.5;
                                p.text(
                                    Text::new(
                                        PlotPoint::new(cx, cy),
                                        egui::RichText::new(&snr_label)
                                            .size(34.0)
                                            .color(egui::Color32::from_gray(90)),
                                    )
                                    .name(""),
                                );
                            }
                            p.line(
                                Line::new(snr)
                                    .color(egui::Color32::from_rgb(120, 230, 120))
                                    .name("est SNR dB"),
                            );
                        });
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            let qr = self.qr_texture(ui.ctx());
            let qr_side = 160.0_f32; // 4× the old toolbar size
                                     // Reserve a band at the bottom for the centered QR so the columns don't eat it.
            let cols_h = (ui.available_height() - qr_side - 14.0).max(200.0);
            let col_w = ui.available_width() / 3.0;
            let spectrum_h = (cols_h * 0.45).clamp(110.0, 320.0);
            let waterfall_h = (cols_h * 0.40).clamp(90.0, 320.0);
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
            let show_wf = self.ui_show_waterfall;
            ui.horizontal(|ui| {
                ui.allocate_ui(egui::vec2(col_w, cols_h), |ui| {
                    draw_panel(
                        ui,
                        "Station A → data TX",
                        &format!("{} (clean)", subtitle.0),
                        &mut self.panels[0],
                        spectrum_h,
                        waterfall_h,
                        show_wf,
                    );
                });
                ui.allocate_ui(egui::vec2(col_w, cols_h), |ui| {
                    // Middle column: the ACK (B→A). Always FSK4-ACK regardless of the data mode (by
                    // design), so it is intentionally mode-invariant — labelled FSK4 to make that clear.
                    draw_panel(
                        ui,
                        "ACK (B→A, FSK4)",
                        &format!("ACK {:?} + noise", subtitle.2),
                        &mut self.panels[2],
                        spectrum_h,
                        waterfall_h,
                        show_wf,
                    );
                });
                ui.allocate_ui(egui::vec2(col_w, cols_h), |ui| {
                    // Far-right column: B's received data signal (mode-dependent + noise), which is
                    // what actually gets demodulated — so the "decoded" status belongs here. Grouped
                    // on the right with Station B's I/Q constellation below.
                    draw_panel(
                        ui,
                        "Station B ← data RX",
                        &format!("{} + noise · {decoded}", subtitle.0),
                        &mut self.panels[1],
                        spectrum_h,
                        waterfall_h,
                        show_wf,
                    );
                });
            });

            // Branding band below the waterfalls: Station A's I/Q constellation on the far left,
            // the wordmark, the QR centered, the tagline, and Station B's I/Q constellation on the
            // far right. The text stays closest to the QR; the constellations sit on the edges.
            if let Some(tex) = &qr {
                ui.add_space(6.0);
                let band_w = ui.available_width();
                // Every data mode now has a meaningful constellation (single-carrier via Hilbert,
                // multicarrier via the plugin's equalized symbols); a mode/frame that yields none
                // (e.g. a multicarrier frame that failed to sync) just self-blanks its scatter.
                let show_const = self.ui_show_constellation;
                // With both constellations shown the QR + 2 squares take 3×qr_side; otherwise just
                // the QR. The remainder splits between the two text blocks flanking the QR.
                let denom = if show_const { 3.0 } else { 1.0 };
                let text_w = ((band_w - denom * qr_side) / 2.0).max(0.0);
                ui.horizontal(|ui| {
                    // Far left: Station A (clean data TX) constellation.
                    if show_const {
                        constellation_plot(
                            ui,
                            "const_a",
                            "Station A — I/Q (TX)",
                            &self.panels[0].iq,
                            qr_side,
                            egui::Color32::from_rgb(120, 200, 255),
                        );
                    }
                    // Left text (closest to the QR): wordmark + sub-line, pushed toward the QR.
                    ui.allocate_ui(egui::vec2(text_w, qr_side), |ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(12.0);
                            ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                                ui.label(
                                    egui::RichText::new("OpenPulseHF")
                                        .size(56.0)
                                        .strong()
                                        .color(egui::Color32::from_gray(220)),
                                );
                                ui.label(
                                    egui::RichText::new("software-based data modem")
                                        .size(18.0)
                                        .color(egui::Color32::from_gray(180)),
                                );
                            });
                        });
                    });
                    // Center: the QR itself.
                    ui.add(egui::Image::new(egui::load::SizedTexture::new(
                        tex.id(),
                        egui::vec2(qr_side, qr_side),
                    )))
                    .on_hover_text("OpenPulseHF");
                    // Right text (closest to the QR): tagline, left-aligned and vertically centered.
                    ui.allocate_ui(egui::vec2(text_w, qr_side), |ui| {
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.add_space(12.0);
                            ui.vertical(|ui| {
                                ui.label(
                                    egui::RichText::new("Free & Opensource").size(24.0).strong(),
                                );
                                ui.label(
                                    egui::RichText::new(
                                        "advanced features over existing solutions",
                                    )
                                    .size(18.0),
                                );
                            });
                        });
                    });
                    // Far right: Station B (post-channel data RX) constellation, flush against the
                    // window's right edge. Anchoring it in a right-to-left region pins it to the
                    // border regardless of item-spacing/rounding in the row above (which otherwise
                    // leaves a gap on the right).
                    if show_const {
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), qr_side),
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                constellation_plot(
                                    ui,
                                    "const_b",
                                    "Station B — I/Q (RX)",
                                    &self.panels[1].iq,
                                    qr_side,
                                    egui::Color32::from_rgb(255, 170, 110),
                                );
                            },
                        );
                    }
                });
            }
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

#[cfg(test)]
mod tests {
    use super::{baseband_iq, samples_per_symbol};

    #[test]
    fn samples_per_symbol_uses_trailing_baud() {
        assert_eq!(samples_per_symbol("BPSK250"), Some(32)); // 8000/250
        assert_eq!(samples_per_symbol("QPSK500"), Some(16));
        assert_eq!(samples_per_symbol("8PSK1000"), Some(8)); // leading order digit ignored
        assert_eq!(samples_per_symbol("64QAM2000-RRC"), Some(4)); // suffix + order stripped
        assert_eq!(samples_per_symbol("QPSK2000-RRC"), Some(4));
    }

    #[test]
    fn samples_per_symbol_skips_multicarrier_and_fsk() {
        assert_eq!(samples_per_symbol("OFDM52-8PSK"), None);
        assert_eq!(samples_per_symbol("SCFDMA52-16QAM"), None);
        assert_eq!(samples_per_symbol("PILOT-QPSK500"), None);
        assert_eq!(samples_per_symbol("FSK4-ACK"), None);
    }

    #[test]
    fn symbol_spaced_constellation_is_tighter_than_the_cloud() {
        // A clean BPSK-like passband: 1500 Hz carrier, phase flipped every 32 samples (250 baud).
        let fs = 8000.0f32;
        let fc = 1500.0f32;
        let sps = 32usize;
        let mut samples = Vec::new();
        for sym in 0..60 {
            let phase = if sym % 2 == 0 {
                0.0
            } else {
                std::f32::consts::PI
            };
            for k in 0..sps {
                let t = (sym * sps + k) as f32;
                samples.push((2.0 * std::f32::consts::PI * fc / fs * t + phase).cos());
            }
        }
        let cloud = baseband_iq(&samples, 0);
        let dots = baseband_iq(&samples, sps);
        assert!(!dots.is_empty() && !cloud.is_empty());
        // Structural: one point per symbol, far fewer than the full-rate cloud.
        assert!(
            dots.len() * 4 < cloud.len(),
            "symbol-spaced {} points should be far fewer than the cloud's {}",
            dots.len(),
            cloud.len()
        );
        // BPSK symbol-spaced points sit on the ±I axis → small |Q| spread; the full-rate cloud
        // sweeps the transition arcs → larger |Q| spread.
        let q_spread = |pts: &[[f64; 2]]| {
            (pts.iter().map(|p| p[1] * p[1]).sum::<f64>() / pts.len() as f64).sqrt()
        };
        assert!(
            q_spread(&dots) < q_spread(&cloud),
            "symbol-spaced Q spread {:.3} should be under the cloud's {:.3}",
            q_spread(&dots),
            q_spread(&cloud)
        );
    }
}
