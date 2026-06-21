//! TCP server speaking the openpulse-daemon control protocol, driven by a live `LinkSim`.
//!
//! Lets an unmodified `openpulse-panel` connect and visualize the simulated two-station
//! link — the speed-level ladder, HPX session state, metrics, and the on-air waterfall —
//! without a real daemon, modem, or audio hardware. The panel cannot tell it apart from a
//! real daemon: it speaks the same NDJSON `ControlEvent` stream interleaved with binary
//! `OPSP` spectrum frames.

use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use openpulse_channel::dsp::{PowerSpectrum, FFT_SIZE};
use openpulse_core::hpx::{HpxEvent, HpxState};
use openpulse_core::rate::{RateEvent, SpeedLevel};
use openpulse_daemon::protocol::{
    encode_spectrum_frame, ControlCommand, ControlEvent, DaemonConfig,
};
use openpulse_modem::event::EngineEvent;

use crate::{LinkParams, LinkSim};

const STATION_CALLSIGN: &str = "LINKSIM";
const STATION_GRID: &str = "JO00aa";
const SESSION_ID: &str = "LINKSIM0";
const SAMPLE_RATE: u32 = 8000;

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
    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                let params = params.clone();
                thread::spawn(move || {
                    let peer = s.peer_addr().map(|a| a.to_string()).unwrap_or_default();
                    eprintln!("linksim: panel connected ({peer})");
                    if let Err(e) = handle_client(s, params, fps) {
                        eprintln!("linksim: panel {peer} disconnected: {e}");
                    } else {
                        eprintln!("linksim: panel {peer} disconnected");
                    }
                });
            }
            Err(e) => eprintln!("linksim: accept error: {e}"),
        }
    }
    Ok(())
}

fn handle_client(stream: TcpStream, params: LinkParams, fps: u32) -> io::Result<()> {
    let mut writer = stream.try_clone()?;
    let reader = BufReader::new(stream);

    // Reader thread: drain client commands, surface GetConfig requests, detect disconnect.
    let (getcfg_tx, getcfg_rx) = mpsc::channel::<()>();
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    thread::spawn(move || {
        let mut reader = reader;
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

    let mut sim = LinkSim::new(&params);
    let mut spectrum = PowerSpectrum::new();
    let row_dt = Duration::from_millis((1000 / fps.max(1)).clamp(20, 200) as u64);

    let mut last_mode = String::new();
    let mut last_level = 0u8;

    // HPX bring-up sequence so the session-status pane shows a live link.
    let mut hpx = HpxState::Idle;
    for (to, ev) in [
        (HpxState::Discovery, HpxEvent::StartSession),
        (HpxState::Training, HpxEvent::DiscoveryOk),
        (HpxState::ActiveTransfer, HpxEvent::TrainingOk),
    ] {
        send_event(&mut writer, &hpx_transition(hpx, to, ev))?;
        hpx = to;
    }

    loop {
        if stop_rx.try_recv().is_ok() {
            return Ok(());
        }
        while getcfg_rx.try_recv().is_ok() {
            send_event(&mut writer, &config_event(&last_mode))?;
        }

        let step = match sim.step() {
            Some(s) => s,
            // total_frames is usize::MAX in serve mode, so this is just a safety restart.
            None => {
                sim = LinkSim::new(&params);
                continue;
            }
        };

        if step.mode != last_mode || step.level != last_level {
            send_event(
                &mut writer,
                &engine(EngineEvent::RateChange {
                    event: RateEvent::Maintained,
                    speed_level: speed_level_from_u8(step.level),
                    mode: step.mode.clone(),
                    direction: None,
                    trigger: None,
                }),
            )?;
            last_mode = step.mode.clone();
            last_level = step.level;
        }

        send_event(
            &mut writer,
            &engine(EngineEvent::FrameTransmitted {
                mode: step.mode.clone(),
                bytes: params.payload_bytes_per_frame,
            }),
        )?;
        if step.delivered {
            send_event(
                &mut writer,
                &engine(EngineEvent::FrameReceived {
                    mode: step.mode.clone(),
                    bytes: step.delivered_bytes,
                }),
            )?;
        } else {
            // A dropped frame → brief recovery excursion for visual feedback.
            send_event(
                &mut writer,
                &hpx_transition(
                    HpxState::ActiveTransfer,
                    HpxState::Recovery,
                    HpxEvent::QualityDrop,
                ),
            )?;
            send_event(
                &mut writer,
                &hpx_transition(
                    HpxState::Recovery,
                    HpxState::ActiveTransfer,
                    HpxEvent::RecoveryOk,
                ),
            )?;
        }

        let attempts = step.attempts.max(1);
        send_event(
            &mut writer,
            &ControlEvent::Metrics {
                effective_bps: step.effective_bps_so_far as f32,
                ecc_rate: Some((attempts - 1) as f32 / attempts as f32),
                compress_ratio: Some(step.compress_ratio as f32),
                afc_correction_hz: 0.0,
                signal_strength_dbm: Some((-120.0 + step.est_snr_db) as i32),
            },
        )?;

        // On-air waterfall: window the received forward waveform into FFT frames.
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
