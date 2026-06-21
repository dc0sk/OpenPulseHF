//! TCP server speaking the openpulse-daemon control protocol, driven by a `LinkSim`.
//!
//! Lets an unmodified `openpulse-panel` connect and visualize a simulated two-station link
//! — the speed-level ladder, HPX session state, metrics, and the on-air waterfall — without
//! a real daemon, modem, or audio hardware. The panel cannot tell it apart from a real
//! daemon: it speaks the same NDJSON `ControlEvent` stream interleaved with binary `OPSP`
//! spectrum frames.
//!
//! Two feeds are supported:
//! - [`serve`] / [`serve_on`] own a fresh `LinkSim` per client (headless CLI use).
//! - [`serve_hub`] streams frames published to a [`FrameHub`] by an *external* simulation
//!   (e.g. the linksim GUI), so the panel and the GUI window reflect the **same** live link.

use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use openpulse_channel::dsp::{PowerSpectrum, FFT_SIZE};
use openpulse_core::hpx::{HpxEvent, HpxState};
use openpulse_core::rate::{RateEvent, SpeedLevel};
use openpulse_daemon::protocol::{
    encode_spectrum_frame, ControlCommand, ControlEvent, DaemonConfig,
};
use openpulse_modem::event::EngineEvent;

use crate::{FrameStep, LinkParams, LinkSim};

const STATION_CALLSIGN: &str = "LINKSIM";
const STATION_GRID: &str = "JO00aa";
const SESSION_ID: &str = "LINKSIM0";
const SAMPLE_RATE: u32 = 8000;

// ── Public entry points ─────────────────────────────────────────────────────────

/// Serve the daemon control protocol on `addr`, driving one fresh `LinkSim` per client.
///
/// Blocks accepting connections; each client runs on its own thread with an independent
/// simulation. `fps` paces the waterfall scroll (frames/second).
pub fn serve(addr: &str, params: &LinkParams, fps: u32) -> io::Result<()> {
    let listener = TcpListener::bind(addr)?;
    eprintln!(
        "linksim: serving daemon control protocol on {addr}\n\
         connect openpulse-panel  (set Server: {addr} → Connect)"
    );
    serve_on(listener, params, fps)
}

/// Serve on an already-bound listener (used by tests that need an ephemeral port).
pub fn serve_on(listener: TcpListener, params: &LinkParams, fps: u32) -> io::Result<()> {
    for stream in listener.incoming().flatten() {
        let params = params.clone();
        thread::spawn(move || {
            let peer = stream
                .peer_addr()
                .map(|a| a.to_string())
                .unwrap_or_default();
            eprintln!("linksim: panel connected ({peer})");
            let _ = handle_owned_client(stream, params, fps);
            eprintln!("linksim: panel {peer} disconnected");
        });
    }
    Ok(())
}

/// Serve panel clients fed by a shared [`FrameHub`] — used when an external simulation
/// (e.g. the GUI) owns the `LinkSim`. Each client receives a copy of every published frame,
/// so the panel stays in lock-step with whatever drives the hub.
pub fn serve_hub(addr: &str, hub: FrameHub) -> io::Result<()> {
    let listener = TcpListener::bind(addr)?;
    eprintln!(
        "linksim: serving daemon control protocol on {addr}\n\
         connect openpulse-panel  (set Server: {addr} → Connect)"
    );
    serve_hub_on(listener, hub)
}

/// Serve hub clients on an already-bound listener (used by tests that need an ephemeral port).
pub fn serve_hub_on(listener: TcpListener, hub: FrameHub) -> io::Result<()> {
    for stream in listener.incoming().flatten() {
        let rx = hub.register();
        thread::spawn(move || {
            let peer = stream
                .peer_addr()
                .map(|a| a.to_string())
                .unwrap_or_default();
            eprintln!("linksim: panel connected ({peer})");
            let _ = handle_hub_client(stream, rx);
            eprintln!("linksim: panel {peer} disconnected");
        });
    }
    Ok(())
}

/// Fan-out hub: a simulation publishes [`FrameStep`]s; each connected panel client gets a copy.
#[derive(Clone, Default)]
pub struct FrameHub {
    clients: Arc<Mutex<Vec<mpsc::Sender<FrameStep>>>>,
}

impl FrameHub {
    /// Create an empty hub with no connected clients.
    pub fn new() -> Self {
        Self::default()
    }

    /// Send a copy of `step` to every connected client; drops clients that have disconnected.
    pub fn publish(&self, step: &FrameStep) {
        let mut clients = self.clients.lock().unwrap_or_else(|e| e.into_inner());
        clients.retain(|tx| tx.send(step.clone()).is_ok());
    }

    /// Number of currently connected panel clients.
    pub fn client_count(&self) -> usize {
        self.clients.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    fn register(&self) -> Receiver<FrameStep> {
        let (tx, rx) = mpsc::channel();
        self.clients
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(tx);
        rx
    }
}

// ── Client handlers ─────────────────────────────────────────────────────────────

/// Headless path: step a private `LinkSim` and pace the waterfall by sleeping per FFT window.
fn handle_owned_client(stream: TcpStream, params: LinkParams, fps: u32) -> io::Result<()> {
    let mut writer = stream.try_clone()?;
    let (getcfg_rx, stop_rx) = spawn_reader(stream);
    send_hpx_bringup(&mut writer)?;

    let mut sim = LinkSim::new(&params);
    let mut spectrum = PowerSpectrum::new();
    let mut st = ClientState::default();
    let row_dt = Duration::from_millis((1000 / fps.max(1)).clamp(20, 200) as u64);

    loop {
        if stop_rx.try_recv().is_ok() {
            return Ok(());
        }
        while getcfg_rx.try_recv().is_ok() {
            send_event(&mut writer, &config_event(&st.last_mode))?;
        }

        let step = match sim.step() {
            Some(s) => s,
            // total_frames is usize::MAX in serve mode, so this is just a safety restart.
            None => {
                sim = LinkSim::new(&params);
                continue;
            }
        };
        emit_frame_events(&mut writer, &step, &mut st)?;

        // On-air waterfall: window the received forward waveform, pacing one row per row_dt.
        let mut sent_any = false;
        for win in step.forward_rx.chunks(FFT_SIZE) {
            if win.len() < FFT_SIZE / 2 {
                continue;
            }
            send_spectrum(&mut writer, &spectrum.compute(win))?;
            sent_any = true;
            thread::sleep(row_dt);
            if stop_rx.try_recv().is_ok() {
                return Ok(());
            }
        }
        if !sent_any {
            // Keep the waterfall scrolling even on a transmit-failed frame.
            send_spectrum(&mut writer, &spectrum.compute(&[0.0f32; FFT_SIZE]))?;
            thread::sleep(row_dt);
        }
    }
}

/// Hub path: an external sim paces the frames; emit one spectrum row per received frame.
fn handle_hub_client(stream: TcpStream, frames: Receiver<FrameStep>) -> io::Result<()> {
    let mut writer = stream.try_clone()?;
    let (getcfg_rx, stop_rx) = spawn_reader(stream);
    send_hpx_bringup(&mut writer)?;

    let mut spectrum = PowerSpectrum::new();
    let mut st = ClientState::default();

    loop {
        if stop_rx.try_recv().is_ok() {
            return Ok(());
        }
        while getcfg_rx.try_recv().is_ok() {
            send_event(&mut writer, &config_event(&st.last_mode))?;
        }

        // Block for the next frame, waking periodically to re-check disconnect.
        let step = match frames.recv_timeout(Duration::from_millis(200)) {
            Ok(s) => s,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return Ok(()), // producer stopped
        };
        emit_frame_events(&mut writer, &step, &mut st)?;
        // One representative spectrum row per frame — matches the GUI waterfall cadence.
        send_spectrum(&mut writer, &spectrum.compute(&step.forward_rx))?;
    }
}

/// Per-client mutable state carried across frames.
#[derive(Default)]
struct ClientState {
    last_mode: String,
    last_level: u8,
}

/// Translate one [`FrameStep`] into the daemon `ControlEvent`s the panel renders.
fn emit_frame_events(
    writer: &mut TcpStream,
    step: &FrameStep,
    st: &mut ClientState,
) -> io::Result<()> {
    if step.mode != st.last_mode || step.level != st.last_level {
        send_event(
            writer,
            &engine(EngineEvent::RateChange {
                event: RateEvent::Maintained,
                speed_level: speed_level_from_u8(step.level),
                mode: step.mode.clone(),
                direction: None,
                trigger: None,
            }),
        )?;
        st.last_mode = step.mode.clone();
        st.last_level = step.level;
    }

    send_event(
        writer,
        &engine(EngineEvent::FrameTransmitted {
            mode: step.mode.clone(),
            bytes: step.payload_bytes,
        }),
    )?;
    if step.delivered {
        send_event(
            writer,
            &engine(EngineEvent::FrameReceived {
                mode: step.mode.clone(),
                bytes: step.delivered_bytes,
            }),
        )?;
    } else {
        // A dropped frame → brief recovery excursion for visual feedback.
        send_event(
            writer,
            &hpx_transition(
                HpxState::ActiveTransfer,
                HpxState::Recovery,
                HpxEvent::QualityDrop,
            ),
        )?;
        send_event(
            writer,
            &hpx_transition(
                HpxState::Recovery,
                HpxState::ActiveTransfer,
                HpxEvent::RecoveryOk,
            ),
        )?;
    }

    send_event(
        writer,
        &ControlEvent::Metrics {
            // Headline effective rate shared with the GUI display so the two windows agree.
            effective_bps: step.effective_bps as f32,
            // Windowed frame-failure rate — a stand-in for FEC stress that tracks conditions.
            ecc_rate: Some((1.0 - step.success_rate) as f32),
            compress_ratio: Some(step.compress_ratio as f32),
            afc_correction_hz: 0.0,
            signal_strength_dbm: Some((-120.0 + step.est_snr_db) as i32),
        },
    )
}

/// Spawn the per-client reader thread. Returns receivers signalled on a `GetConfig` request
/// and on disconnect (EOF or error), respectively.
fn spawn_reader(stream: TcpStream) -> (Receiver<()>, Receiver<()>) {
    let (getcfg_tx, getcfg_rx) = mpsc::channel();
    let (stop_tx, stop_rx) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break, // EOF or broken connection
                Ok(_) => {
                    if let Ok(ControlCommand::GetConfig) =
                        serde_json::from_str::<ControlCommand>(line.trim())
                    {
                        let _ = getcfg_tx.send(());
                    }
                }
            }
        }
        let _ = stop_tx.send(());
    });
    (getcfg_rx, stop_rx)
}

/// HPX bring-up sequence so the session-status pane shows a live link on connect.
fn send_hpx_bringup(writer: &mut TcpStream) -> io::Result<()> {
    let mut from = HpxState::Idle;
    for (to, ev) in [
        (HpxState::Discovery, HpxEvent::StartSession),
        (HpxState::Training, HpxEvent::DiscoveryOk),
        (HpxState::ActiveTransfer, HpxEvent::TrainingOk),
    ] {
        send_event(writer, &hpx_transition(from, to, ev))?;
        from = to;
    }
    Ok(())
}

// ── Wire helpers ────────────────────────────────────────────────────────────────

fn send_event(writer: &mut TcpStream, ev: &ControlEvent) -> io::Result<()> {
    let mut line = serde_json::to_string(ev).map_err(io::Error::other)?;
    line.push('\n');
    writer.write_all(line.as_bytes())?;
    writer.flush()
}

fn send_spectrum(writer: &mut TcpStream, bins: &[f32]) -> io::Result<()> {
    writer.write_all(&encode_spectrum_frame(SAMPLE_RATE, bins))?;
    writer.flush()
}

fn engine(event: EngineEvent) -> ControlEvent {
    ControlEvent::EngineEvent { event }
}

fn hpx_transition(from: HpxState, to: HpxState, event: HpxEvent) -> ControlEvent {
    engine(EngineEvent::HpxTransition {
        from,
        to,
        event,
        session_id: Some(SESSION_ID.to_string()),
    })
}

fn config_event(mode: &str) -> ControlEvent {
    ControlEvent::ConfigData {
        config: DaemonConfig {
            callsign: STATION_CALLSIGN.into(),
            grid_square: STATION_GRID.into(),
            mode: if mode.is_empty() {
                "—".into()
            } else {
                mode.into()
            },
            tx_attenuation_db: 0.0,
            qsy_enabled: false,
            bandplan_mode: "unrestricted".into(),
            allow_tuner_on_high_swr: false,
        },
    }
}

/// Map a numeric speed level back to its [`SpeedLevel`] enum (clamped to the SL1–SL20 range).
fn speed_level_from_u8(n: u8) -> SpeedLevel {
    match n {
        0 | 1 => SpeedLevel::Sl1,
        2 => SpeedLevel::Sl2,
        3 => SpeedLevel::Sl3,
        4 => SpeedLevel::Sl4,
        5 => SpeedLevel::Sl5,
        6 => SpeedLevel::Sl6,
        7 => SpeedLevel::Sl7,
        8 => SpeedLevel::Sl8,
        9 => SpeedLevel::Sl9,
        10 => SpeedLevel::Sl10,
        11 => SpeedLevel::Sl11,
        12 => SpeedLevel::Sl12,
        13 => SpeedLevel::Sl13,
        14 => SpeedLevel::Sl14,
        15 => SpeedLevel::Sl15,
        16 => SpeedLevel::Sl16,
        17 => SpeedLevel::Sl17,
        18 => SpeedLevel::Sl18,
        19 => SpeedLevel::Sl19,
        _ => SpeedLevel::Sl20,
    }
}
