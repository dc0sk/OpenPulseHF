//! Background TCP connection thread.
//!
//! Connects to `openpulse-server` control port, reads `ControlEvent` NDJSON lines
//! and binary spectrum frames, applies them to [`PanelState`], and forwards
//! `ControlCommand`s from the UI.
//!
//! After connecting, the thread immediately sends `SubscribeSpectrum { fps: 20 }`.
//! The read loop distinguishes binary spectrum frames from NDJSON lines by peeking
//! at the first byte: `O` (0x4F = `SPECTRUM_MAGIC[0]`) → binary frame, otherwise
//! → NDJSON line.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use openpulse_daemon::protocol::{ControlCommand, ControlEvent, SPECTRUM_MAGIC};

use crate::state::{PanelState, RigSnapshot};

/// Spawn the connection thread.  Returns a sender for outbound commands.
pub fn spawn(
    addr: String,
    shared: Arc<Mutex<PanelState>>,
    stop_rx: Receiver<()>,
) -> Sender<ControlCommand> {
    let (cmd_tx, cmd_rx) = crossbeam_channel::bounded::<ControlCommand>(32);
    std::thread::spawn(move || run_loop(addr, shared, stop_rx, cmd_rx));
    cmd_tx
}

fn run_loop(
    addr: String,
    shared: Arc<Mutex<PanelState>>,
    stop_rx: Receiver<()>,
    cmd_rx: Receiver<ControlCommand>,
) {
    loop {
        if stop_rx.try_recv().is_ok() {
            break;
        }

        match TcpStream::connect(&addr) {
            Err(e) => {
                {
                    let mut st = shared.lock().unwrap();
                    st.connected = false;
                    st.push_log(format!("connect error: {e}"));
                }
                std::thread::sleep(Duration::from_secs(2));
            }
            Ok(stream) => {
                {
                    let mut st = shared.lock().unwrap();
                    st.connected = true;
                    st.push_log(format!("connected to {addr}"));
                }
                let _ = stream.set_read_timeout(Some(Duration::from_millis(50)));
                run_session(stream, &shared, &stop_rx, &cmd_rx);
                {
                    let mut st = shared.lock().unwrap();
                    st.connected = false;
                    if stop_rx.try_recv().is_ok() {
                        break;
                    }
                    st.push_log("disconnected — retrying in 2 s".into());
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        }
    }
}

fn run_session(
    stream: TcpStream,
    shared: &Arc<Mutex<PanelState>>,
    stop_rx: &Receiver<()>,
    cmd_rx: &Receiver<ControlCommand>,
) {
    let mut write_half = stream.try_clone().unwrap();
    let mut reader = BufReader::new(stream);

    // Subscribe to spectrum immediately on connect.
    if let Ok(mut s) = serde_json::to_string(&ControlCommand::SubscribeSpectrum { fps: 20 }) {
        s.push('\n');
        let _ = write_half.write_all(s.as_bytes());
    }

    let mut line = String::new();

    loop {
        if stop_rx.try_recv().is_ok() {
            break;
        }

        // Forward any pending commands from the UI.
        while let Ok(cmd) = cmd_rx.try_recv() {
            if let Ok(mut s) = serde_json::to_string(&cmd) {
                s.push('\n');
                let _ = write_half.write_all(s.as_bytes());
            }
        }

        // Peek at the first buffered byte to decide how to read.
        let first_byte = match reader.fill_buf() {
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue
            }
            Err(_) => break,
            Ok(&[]) => break, // EOF
            Ok(buf) => buf[0],
        };

        if first_byte == SPECTRUM_MAGIC[0] {
            // Binary spectrum frame: 4 magic + 2 fft_size LE + 4 sample_rate LE + bins.
            let mut header = [0u8; 10];
            if reader.read_exact(&mut header).is_err() {
                break;
            }
            if &header[0..4] != SPECTRUM_MAGIC {
                break;
            }
            let fft_size = u16::from_le_bytes([header[4], header[5]]) as usize;
            let mut bin_bytes = vec![0u8; fft_size * 4];
            if reader.read_exact(&mut bin_bytes).is_err() {
                break;
            }
            let bins: Vec<f32> = bin_bytes
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            shared.lock().unwrap().spectrum_bins = bins;
        } else {
            // NDJSON line.
            line.clear();
            match reader.read_line(&mut line) {
                Err(ref e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    continue
                }
                Err(_) | Ok(0) => break,
                Ok(_) => {
                    let text = line.trim().to_string();
                    if !text.is_empty() {
                        apply_event(&text, shared);
                    }
                }
            }
        }
    }
}

fn apply_event(line: &str, shared: &Arc<Mutex<PanelState>>) {
    // CommandResponse has `ok` field; skip it.
    if line.contains("\"ok\"") {
        return;
    }

    let ev: ControlEvent = match serde_json::from_str(line) {
        Ok(e) => e,
        Err(_) => return,
    };

    let mut st = shared.lock().unwrap();

    match ev {
        ControlEvent::EngineEvent { event } => {
            use openpulse_modem::EngineEvent;
            let desc = match &event {
                EngineEvent::FrameReceived { mode, bytes } => {
                    format!("RX {bytes}B [{mode}]")
                }
                EngineEvent::FrameTransmitted { mode, bytes } => {
                    format!("TX {bytes}B [{mode}]")
                }
                EngineEvent::DcdChange { busy, energy } => {
                    st.dcd_busy = *busy;
                    st.dcd_energy = *energy;
                    format!("DCD {} e={energy:.3}", if *busy { "BUSY" } else { "CLEAR" })
                }
                EngineEvent::AfcUpdate { offset_hz, .. } => {
                    st.afc_hz = *offset_hz;
                    format!("AFC {offset_hz:+.1} Hz")
                }
                EngineEvent::RateChange {
                    speed_level, mode, ..
                } => {
                    st.speed_level = format!("{speed_level:?}");
                    st.mode = mode.clone();
                    format!("RATE {speed_level:?} [{mode}]")
                }
                EngineEvent::HpxTransition { from, to, .. } => {
                    format!("HPX {from:?}→{to:?}")
                }
                EngineEvent::SessionStarted { session_id, .. } => {
                    let id = session_id.as_deref().unwrap_or("?");
                    format!("SESSION START {id}")
                }
                EngineEvent::SessionEnded { reason, .. } => {
                    format!("SESSION END: {reason}")
                }
            };
            st.push_log(desc);
        }
        ControlEvent::Metrics {
            effective_bps,
            ecc_rate,
            compress_ratio,
            afc_correction_hz,
            signal_strength_dbm,
        } => {
            st.effective_bps = effective_bps;
            st.ecc_rate = ecc_rate;
            st.compress_ratio = compress_ratio;
            if afc_correction_hz != 0.0 {
                st.afc_hz = afc_correction_hz;
            }
            st.signal_strength_dbm = signal_strength_dbm;
        }
        ControlEvent::RigStatus {
            rig,
            freq_hz,
            mode,
            power_w,
            alc,
            swr,
        } => {
            let snap = RigSnapshot {
                freq_hz,
                mode,
                power_w,
                alc,
                swr,
            };
            if rig == "a" {
                st.rig_a = Some(snap);
            } else {
                st.rig_b = Some(snap);
            }
        }
    }
}
