//! Background TCP connection thread.
//!
//! Connects to `openpulse-server` control port, reads `ControlEvent` NDJSON lines,
//! applies them to [`PanelState`], and forwards `ControlCommand`s from the UI.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use openpulse_daemon::protocol::{ControlCommand, ControlEvent};

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
    let reader = BufReader::new(stream);

    for line in reader.lines() {
        if stop_rx.try_recv().is_ok() {
            break;
        }

        // Forward any pending commands.
        while let Ok(cmd) = cmd_rx.try_recv() {
            if let Ok(mut s) = serde_json::to_string(&cmd) {
                s.push('\n');
                let _ = write_half.write_all(s.as_bytes());
            }
        }

        match line {
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
            Err(_) => break,
            Ok(text) => {
                let text = text.trim().to_string();
                if text.is_empty() {
                    continue;
                }
                apply_event(&text, shared);
            }
        }
    }
}

fn apply_event(line: &str, shared: &Arc<Mutex<PanelState>>) {
    // Try CommandResponse first (has `ok` field, not `type`).
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
