//! `openpulse-twinview` — a single window showing **both directions** of a
//! twin-station link by connecting to two real `openpulse-server` daemons at once.
//!
//! Two columns, one per station, each rendering that daemon's live spectrum +
//! waterfall and its rate/OTA/HPX readouts (from the real control protocol). The
//! left station's TX level is the A→B rate; the right station's is B→A — so both
//! directions of the bridged link are visible side by side, instead of running two
//! separate `openpulse-panel` windows.
//!
//! Usage:
//! ```text
//! openpulse-twinview                              # 127.0.0.1:9000 + 127.0.0.1:9002
//! openpulse-twinview 127.0.0.1:9000 127.0.0.1:9002
//! ```

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use egui::{Color32, ColorImage, RichText, TextureHandle};
use egui_plot::{Line, Plot, PlotPoints};
use openpulse_daemon::protocol::ControlEvent;

const WATERFALL_ROWS: usize = 64;
const WATERFALL_COLS: usize = 512;

/// Live state for one station, updated by its connection thread.
#[derive(Default)]
struct StationState {
    label: String,
    addr: String,
    connected: bool,
    spectrum_bins: Vec<f32>,
    history: VecDeque<Vec<f32>>,
    generation: u64,
    mode: String,
    speed_level: String,
    ota_active: bool,
    ota_tx_level: Option<String>,
    ota_rx_recommended: Option<String>,
    ota_rx_confirmed: Option<String>,
    hpx_state: String,
    dcd_busy: bool,
    dcd_energy: f32,
    afc_hz: f32,
    effective_bps: f32,
    ptt_active: bool,
    last: String,
}

fn spawn_connection(label: String, addr: String, shared: Arc<Mutex<StationState>>) {
    {
        let mut st = shared.lock().unwrap();
        st.label = label;
        st.addr = addr.clone();
    }
    std::thread::spawn(move || loop {
        match TcpStream::connect(&addr) {
            Ok(stream) => {
                let _ = stream.set_read_timeout(Some(Duration::from_millis(50)));
                if let Ok(mut writer) = stream.try_clone() {
                    // Ask the daemon to start streaming spectrum frames.
                    let _ = writeln!(writer, r#"{{"cmd":"subscribe_spectrum","fps":20}}"#);
                    let _ = writer.flush();
                    shared.lock().unwrap().connected = true;
                    run_session(BufReader::new(stream), &shared);
                    shared.lock().unwrap().connected = false;
                    // keep `writer` alive for the whole session above.
                    drop(writer);
                }
            }
            Err(_) => {
                shared.lock().unwrap().connected = false;
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    });
}

/// Read the interleaved binary-spectrum / NDJSON-event stream until it closes.
fn run_session(mut reader: BufReader<TcpStream>, shared: &Arc<Mutex<StationState>>) {
    use std::io::ErrorKind::{TimedOut, WouldBlock};
    loop {
        let first = match reader.fill_buf() {
            Err(ref e) if e.kind() == WouldBlock || e.kind() == TimedOut => continue,
            Err(_) | Ok(&[]) => return,
            Ok(buf) => buf[0],
        };
        if first == b'O' {
            // Binary spectrum frame: "OPSP" + fft_size(u16 LE) + sample_rate(u32 LE) + bins.
            let mut header = [0u8; 10];
            if reader.read_exact(&mut header).is_err() || &header[0..4] != b"OPSP" {
                return;
            }
            let n = u16::from_le_bytes([header[4], header[5]]) as usize;
            if n > 8192 {
                return;
            }
            let mut payload = vec![0u8; n * 4];
            if reader.read_exact(&mut payload).is_err() {
                return;
            }
            let bins: Vec<f32> = payload
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            let mut st = shared.lock().unwrap();
            st.spectrum_bins = bins.clone();
            st.history.push_front(bins);
            if st.history.len() > WATERFALL_ROWS {
                st.history.pop_back();
            }
            st.generation = st.generation.wrapping_add(1);
        } else {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Err(ref e) if e.kind() == WouldBlock || e.kind() == TimedOut => continue,
                Err(_) | Ok(0) => return,
                Ok(_) => {
                    let t = line.trim();
                    if !t.is_empty() {
                        apply_event(t, shared);
                    }
                }
            }
        }
    }
}

fn apply_event(line: &str, shared: &Arc<Mutex<StationState>>) {
    // CommandResponse lines carry an "ok" field; skip them.
    if line.contains("\"ok\"") {
        return;
    }
    let ev: ControlEvent = match serde_json::from_str(line) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut st = shared.lock().unwrap();
    match ev {
        ControlEvent::OtaStatus {
            active,
            tx_mode,
            tx_level,
            rx_recommended_level,
            rx_confirmed_level,
            ..
        } => {
            st.ota_active = active;
            if let Some(m) = tx_mode {
                st.mode = m;
            }
            if let Some(l) = tx_level.clone() {
                st.speed_level = l;
            }
            st.ota_tx_level = tx_level;
            st.ota_rx_recommended = rx_recommended_level;
            st.ota_rx_confirmed = rx_confirmed_level;
        }
        ControlEvent::Metrics { effective_bps, .. } => st.effective_bps = effective_bps,
        ControlEvent::PttChanged { active } => st.ptt_active = active,
        ControlEvent::EngineEvent { event } => {
            use openpulse_modem::EngineEvent;
            match event {
                EngineEvent::RateChange {
                    speed_level, mode, ..
                } => {
                    st.speed_level = format!("{speed_level:?}");
                    st.mode = mode;
                }
                EngineEvent::HpxTransition { to, .. } => st.hpx_state = format!("{to:?}"),
                EngineEvent::DcdChange { busy, energy } => {
                    st.dcd_busy = busy;
                    st.dcd_energy = energy;
                }
                EngineEvent::AfcUpdate { offset_hz, .. } => st.afc_hz = offset_hz,
                EngineEvent::FrameReceived { mode, bytes } => {
                    st.last = format!("RX {bytes}B [{mode}]")
                }
                EngineEvent::FrameTransmitted { mode, bytes } => {
                    st.last = format!("TX {bytes}B [{mode}]")
                }
                _ => {}
            }
        }
        _ => {}
    }
}

// ── Waterfall colormap (plasma), matching openpulse-panel ──────────────────────

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

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

fn build_waterfall_image(history: &VecDeque<Vec<f32>>) -> ColorImage {
    let mut pixels = vec![Color32::BLACK; WATERFALL_COLS * WATERFALL_ROWS];
    for (row, bins) in history.iter().enumerate() {
        for col in 0..WATERFALL_COLS {
            let dbfs = bins.get(col).copied().unwrap_or(-120.0);
            let t = ((dbfs + 120.0) / 120.0).clamp(0.0, 1.0);
            pixels[row * WATERFALL_COLS + col] = plasma(t);
        }
    }
    ColorImage {
        size: [WATERFALL_COLS, WATERFALL_ROWS],
        pixels,
    }
}

// ── App ────────────────────────────────────────────────────────────────────────

struct TwinApp {
    a: Arc<Mutex<StationState>>,
    b: Arc<Mutex<StationState>>,
    a_tex: Option<TextureHandle>,
    a_gen: u64,
    b_tex: Option<TextureHandle>,
    b_gen: u64,
}

impl TwinApp {
    fn new(addr_a: String, addr_b: String) -> Self {
        let a = Arc::new(Mutex::new(StationState::default()));
        let b = Arc::new(Mutex::new(StationState::default()));
        spawn_connection("Station A".into(), addr_a, Arc::clone(&a));
        spawn_connection("Station B".into(), addr_b, Arc::clone(&b));
        Self {
            a,
            b,
            a_tex: None,
            a_gen: u64::MAX,
            b_tex: None,
            b_gen: u64::MAX,
        }
    }
}

fn refresh_texture(
    ctx: &egui::Context,
    shared: &Arc<Mutex<StationState>>,
    tex: &mut Option<TextureHandle>,
    seen_gen: &mut u64,
    name: &str,
) {
    let (gen, image) = {
        let st = shared.lock().unwrap();
        if st.generation == *seen_gen || st.history.is_empty() {
            return;
        }
        (st.generation, build_waterfall_image(&st.history))
    };
    *seen_gen = gen;
    match tex {
        Some(t) => t.set(image, egui::TextureOptions::default()),
        None => *tex = Some(ctx.load_texture(name, image, egui::TextureOptions::default())),
    }
}

fn draw_station(ui: &mut egui::Ui, shared: &Arc<Mutex<StationState>>, tex: Option<&TextureHandle>) {
    let st = shared.lock().unwrap();
    ui.horizontal(|ui| {
        ui.heading(&st.label);
        let (dot, color) = if st.connected {
            ("● connected", Color32::from_rgb(80, 200, 120))
        } else {
            ("● offline", Color32::from_rgb(200, 90, 90))
        };
        ui.label(RichText::new(dot).color(color));
    });
    ui.label(RichText::new(&st.addr).color(Color32::GRAY).small());
    ui.separator();

    // Spectrum.
    ui.label(RichText::new("Spectrum").strong());
    if st.spectrum_bins.is_empty() {
        ui.label(RichText::new("waiting for spectrum…").color(Color32::GRAY));
    } else {
        let points: PlotPoints = st
            .spectrum_bins
            .iter()
            .enumerate()
            .map(|(i, &v)| [i as f64, v as f64])
            .collect();
        Plot::new(format!("spec-{}", st.label))
            .height(110.0)
            .include_y(-120.0)
            .include_y(0.0)
            .show(ui, |p| {
                p.line(Line::new(points).color(Color32::from_rgb(100, 200, 100)));
            });
    }

    // Waterfall.
    ui.label(RichText::new("Waterfall").strong());
    match tex {
        Some(t) => {
            let size = egui::vec2(ui.available_width().min(512.0), 96.0);
            ui.image((t.id(), size));
        }
        None => {
            ui.label(RichText::new("waiting for waterfall…").color(Color32::GRAY));
        }
    }

    ui.separator();
    // Readouts.
    egui::Grid::new(format!("grid-{}", st.label))
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            let dash = "—".to_string();
            ui.label("Mode");
            ui.label(if st.mode.is_empty() { &dash } else { &st.mode });
            ui.end_row();
            ui.label("TX level");
            ui.label(st.ota_tx_level.as_ref().unwrap_or(&dash));
            ui.end_row();
            ui.label("RX confirmed");
            ui.label(st.ota_rx_confirmed.as_ref().unwrap_or(&dash));
            ui.end_row();
            ui.label("RX recommend");
            ui.label(st.ota_rx_recommended.as_ref().unwrap_or(&dash));
            ui.end_row();
            ui.label("OTA");
            ui.label(if st.ota_active { "active" } else { "off" });
            ui.end_row();
            ui.label("HPX");
            ui.label(if st.hpx_state.is_empty() {
                &dash
            } else {
                &st.hpx_state
            });
            ui.end_row();
            ui.label("AFC");
            ui.label(format!("{:+.1} Hz", st.afc_hz));
            ui.end_row();
            ui.label("Throughput");
            ui.label(format!("{:.0} bps", st.effective_bps));
            ui.end_row();
            ui.label("PTT");
            ui.label(
                RichText::new(if st.ptt_active { "TX" } else { "rx" }).color(if st.ptt_active {
                    Color32::from_rgb(230, 120, 60)
                } else {
                    Color32::GRAY
                }),
            );
            ui.end_row();
        });

    // DCD energy bar.
    ui.horizontal(|ui| {
        ui.label("DCD");
        let energy_norm = (st.dcd_energy * 10.0).min(1.0);
        let color = if st.dcd_busy {
            Color32::RED
        } else {
            Color32::from_rgb(80, 200, 120)
        };
        let (rect, _) = ui.allocate_exact_size(egui::vec2(140.0, 14.0), egui::Sense::hover());
        if ui.is_rect_visible(rect) {
            ui.painter().rect_filled(rect, 2.0, Color32::DARK_GRAY);
            let filled =
                egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * energy_norm, 14.0));
            ui.painter().rect_filled(filled, 2.0, color);
        }
    });
    if !st.last.is_empty() {
        ui.label(RichText::new(&st.last).color(Color32::GRAY).small());
    }
}

impl eframe::App for TwinApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(100));
        refresh_texture(ctx, &self.a, &mut self.a_tex, &mut self.a_gen, "wf_a");
        refresh_texture(ctx, &self.b, &mut self.b_tex, &mut self.b_gen, "wf_b");

        egui::TopBottomPanel::top("hdr").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("OpenPulse Twin-Station View");
                ui.label(
                    RichText::new("both directions of one bridged link")
                        .color(Color32::GRAY)
                        .small(),
                );
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.columns(2, |cols| {
                draw_station(&mut cols[0], &self.a, self.a_tex.as_ref());
                draw_station(&mut cols[1], &self.b, self.b_tex.as_ref());
            });
        });
    }
}

fn main() -> eframe::Result<()> {
    let mut args = std::env::args().skip(1);
    let addr_a = args.next().unwrap_or_else(|| "127.0.0.1:9000".to_string());
    let addr_b = args.next().unwrap_or_else(|| "127.0.0.1:9002".to_string());

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("OpenPulse Twin-Station View")
            .with_inner_size([1100.0, 720.0]),
        ..Default::default()
    };
    eframe::run_native(
        "openpulse-twinview",
        options,
        Box::new(move |_cc| Ok(Box::new(TwinApp::new(addr_a, addr_b)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use openpulse_modem::EngineEvent;

    fn state() -> Arc<Mutex<StationState>> {
        Arc::new(Mutex::new(StationState::default()))
    }

    #[test]
    fn ota_status_updates_levels_and_mode() {
        let shared = state();
        let ev = ControlEvent::OtaStatus {
            active: true,
            tx_mode: Some("QPSK500".into()),
            tx_level: Some("SL5".into()),
            tx_fec: "rs".into(),
            rx_recommended_level: Some("SL4".into()),
            rx_confirmed_level: Some("SL3".into()),
            is_locked: false,
        };
        apply_event(&serde_json::to_string(&ev).unwrap(), &shared);
        let st = shared.lock().unwrap();
        assert!(st.ota_active);
        assert_eq!(st.mode, "QPSK500");
        assert_eq!(st.speed_level, "SL5");
        assert_eq!(st.ota_tx_level.as_deref(), Some("SL5"));
        assert_eq!(st.ota_rx_confirmed.as_deref(), Some("SL3"));
    }

    #[test]
    fn engine_dcd_event_updates_dcd() {
        let shared = state();
        let ev = ControlEvent::EngineEvent {
            event: EngineEvent::DcdChange {
                busy: true,
                energy: 0.42,
            },
        };
        apply_event(&serde_json::to_string(&ev).unwrap(), &shared);
        let st = shared.lock().unwrap();
        assert!(st.dcd_busy);
        assert!((st.dcd_energy - 0.42).abs() < 1e-6);
    }

    #[test]
    fn command_response_and_garbage_are_ignored() {
        let shared = state();
        apply_event(r#"{"ok":true}"#, &shared);
        apply_event("not json", &shared);
        let st = shared.lock().unwrap();
        assert!(!st.ota_active);
        assert!(st.mode.is_empty());
    }

    #[test]
    fn waterfall_image_has_expected_dimensions() {
        let mut history = VecDeque::new();
        history.push_front(vec![-30.0f32; WATERFALL_COLS]);
        let img = build_waterfall_image(&history);
        assert_eq!(img.size, [WATERFALL_COLS, WATERFALL_ROWS]);
    }
}
