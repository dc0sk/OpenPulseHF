//! Background connection thread.
//!
//! Connects to the daemon control port (TCP or WebSocket), reads
//! [`ControlEvent`] messages, applies them to [`PanelState`], and forwards
//! [`ControlCommand`]s from the UI.
//!
//! After connecting the thread immediately sends `SubscribeSpectrum { fps: 20 }`.

use std::sync::{Arc, Mutex};
#[cfg(not(target_arch = "wasm32"))]
use std::thread;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
use crossbeam_channel::{Receiver, Sender};
#[cfg(not(target_arch = "wasm32"))]
use openpulse_daemon::protocol::ControlCommand;
use openpulse_daemon::protocol::{ControlEvent, SPECTRUM_MAGIC};

use crate::state::{PanelState, RigSnapshot, ECC_HISTORY_LEN, WATERFALL_ROWS};
#[cfg(not(target_arch = "wasm32"))]
use crate::transport::{RecvMsg, TcpTransport, Transport, WsTransport};

/// Whether to use raw TCP or WebSocket transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportKind {
    #[cfg(not(target_arch = "wasm32"))]
    Tcp,
    WebSocket,
}

/// Spawn the connection thread.  Returns a sender for outbound commands.
#[cfg(not(target_arch = "wasm32"))]
pub fn spawn(
    addr: String,
    kind: TransportKind,
    shared: Arc<Mutex<PanelState>>,
    stop_rx: Receiver<()>,
) -> Sender<ControlCommand> {
    let (cmd_tx, cmd_rx) = crossbeam_channel::bounded::<ControlCommand>(32);
    thread::spawn(move || run_loop(addr, kind, shared, stop_rx, cmd_rx));
    cmd_tx
}

#[cfg(not(target_arch = "wasm32"))]
fn run_loop(
    addr: String,
    kind: TransportKind,
    shared: Arc<Mutex<PanelState>>,
    stop_rx: Receiver<()>,
    cmd_rx: Receiver<ControlCommand>,
) {
    loop {
        if stop_rx.try_recv().is_ok() {
            break;
        }

        let transport: Option<Box<dyn Transport>> = match kind {
            TransportKind::Tcp => {
                TcpTransport::connect(&addr).map(|t| Box::new(t) as Box<dyn Transport>)
            }
            TransportKind::WebSocket => {
                WsTransport::connect(&addr).map(|t| Box::new(t) as Box<dyn Transport>)
            }
        };

        match transport {
            None => {
                let kind_str = match kind {
                    TransportKind::Tcp => "TCP",
                    TransportKind::WebSocket => "WS",
                };
                shared.lock().unwrap().push_log(format!(
                    "connect error: {kind_str} connection to {addr} failed"
                ));
                thread::sleep(Duration::from_secs(2));
            }
            Some(mut t) => {
                {
                    let mut st = shared.lock().unwrap();
                    st.connected = true;
                    st.push_log(format!("connected to {addr}"));
                }
                // Subscribe to spectrum immediately.
                if let Ok(s) = serde_json::to_string(&ControlCommand::SubscribeSpectrum { fps: 20 })
                {
                    t.send_text(&s);
                }

                run_session(t.as_mut(), &shared, &stop_rx, &cmd_rx);

                {
                    let mut st = shared.lock().unwrap();
                    st.connected = false;
                    if stop_rx.try_recv().is_ok() {
                        break;
                    }
                    st.push_log("disconnected — retrying in 2 s".into());
                }
                thread::sleep(Duration::from_secs(2));
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn run_session(
    transport: &mut dyn Transport,
    shared: &Arc<Mutex<PanelState>>,
    stop_rx: &Receiver<()>,
    cmd_rx: &Receiver<ControlCommand>,
) {
    loop {
        if stop_rx.try_recv().is_ok() {
            break;
        }

        // Forward pending UI commands.
        while let Ok(cmd) = cmd_rx.try_recv() {
            if let Ok(s) = serde_json::to_string(&cmd) {
                if !transport.send_text(&s) {
                    return;
                }
            }
        }

        // Poll for incoming data.
        match transport.try_recv() {
            None => {
                thread::sleep(Duration::from_millis(10));
            }
            Some(Err(())) => break,
            Some(Ok(RecvMsg::Binary(frame))) => {
                apply_spectrum(&frame, shared);
            }
            Some(Ok(RecvMsg::Text(line))) => {
                if !line.is_empty() {
                    apply_event(&line, shared);
                }
            }
        }
    }
}

pub(crate) fn apply_spectrum(frame: &[u8], shared: &Arc<Mutex<PanelState>>) {
    if frame.len() < 10 || &frame[0..4] != SPECTRUM_MAGIC {
        return;
    }
    let fft_size = u16::from_le_bytes([frame[4], frame[5]]) as usize;
    let expected = 10 + fft_size * 4;
    if frame.len() < expected {
        return;
    }
    let bins: Vec<f32> = frame[10..expected]
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    let mut st = shared.lock().unwrap();
    st.spectrum_history.push_front(bins.clone());
    if st.spectrum_history.len() > WATERFALL_ROWS {
        st.spectrum_history.pop_back();
    }
    st.spectrum_generation = st.spectrum_generation.wrapping_add(1);
    st.spectrum_bins = bins;
}

pub(crate) fn apply_event(line: &str, shared: &Arc<Mutex<PanelState>>) {
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
                    st.speed_level_num = *speed_level as u8;
                    st.mode = mode.clone();
                    format!("RATE {speed_level:?} [{mode}]")
                }
                EngineEvent::HpxTransition { from, to, .. } => {
                    st.hpx_state = format!("{to:?}");
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
            st.ecc_history.push_front(ecc_rate);
            if st.ecc_history.len() > ECC_HISTORY_LEN {
                st.ecc_history.pop_back();
            }
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
        ControlEvent::PttChanged { active } => {
            st.ptt_active = active;
        }
        ControlEvent::RfConnectionChanged { connected, peer } => {
            st.rf_connected = connected;
            st.rf_peer = peer;
        }
        ControlEvent::ConfigData { config } => {
            st.daemon_config = Some(config);
        }
        ControlEvent::MessageReceived {
            id,
            from,
            to,
            subject,
            timestamp_secs,
            ..
        } => {
            // Add to inbox if not already present.
            if !st.inbox.iter().any(|m| m.id == id) {
                use openpulse_daemon::protocol::MessageSummary;
                st.inbox.push(MessageSummary {
                    id,
                    from: from.clone(),
                    to: to.clone(),
                    subject: subject.clone(),
                    timestamp_secs,
                });
            }
            st.push_log(format!("MSG from {from} → {to}: {subject}"));
        }
        ControlEvent::MessageList { messages } => {
            st.inbox = messages;
        }
        ControlEvent::MessageData { id, body, .. } => {
            st.open_message_id = Some(id);
            st.open_message_body = Some(body);
        }
    }
}
