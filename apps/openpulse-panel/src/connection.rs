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

use crate::state::{
    ActiveTransfer, IncomingOffer, PanelState, ReceivedFile, RigSnapshot, ECC_HISTORY_LEN,
    WATERFALL_ROWS,
};
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
                    st.record_frame(true);
                    format!("RX {bytes}B [{mode}]")
                }
                EngineEvent::FrameTransmitted { mode, bytes } => {
                    st.record_frame(false);
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
                    st.reset_frame_stats();
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
            st.ecc_rate = ecc_rate.unwrap_or(0.0);
            st.compress_ratio = compress_ratio.unwrap_or(1.0);
            if afc_correction_hz != 0.0 {
                st.afc_hz = afc_correction_hz;
            }
            st.signal_strength_dbm = signal_strength_dbm;
            st.ecc_history.push_front(ecc_rate.unwrap_or(0.0));
            if st.ecc_history.len() > ECC_HISTORY_LEN {
                st.ecc_history.pop_back();
            }
        }
        ControlEvent::SystemMetrics {
            cpu_percent,
            ram_mb,
            ram_percent,
            gpu_percent,
            decode_latency_ms,
        } => {
            st.cpu_percent = cpu_percent;
            st.ram_mb = ram_mb;
            st.ram_percent = ram_percent;
            st.gpu_percent = gpu_percent;
            st.decode_latency_ms = decode_latency_ms;
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
            // The primary rig (single-rig CAT identifies itself as "rigctld") maps to
            // slot A; only an explicit "b" (cross-band secondary) goes to slot B.
            if rig == "b" {
                st.rig_b = Some(snap);
            } else {
                st.rig_a = Some(snap);
            }
        }
        ControlEvent::PttChanged { active } => {
            st.ptt_active = active;
        }
        ControlEvent::RfConnectionChanged { connected, peer } => {
            st.rf_connected = connected;
            st.rf_peer = peer;
        }
        ControlEvent::PeerVerified { callsign, grid } => {
            if grid.is_empty() {
                st.push_log(format!(
                    "Peer {callsign} identity verified (signed handshake)"
                ));
            } else {
                st.push_log(format!(
                    "Peer {callsign} verified (signed handshake), grid {grid}"
                ));
            }
        }
        ControlEvent::RepeaterChanged { enabled } => {
            st.repeater_enabled = enabled;
            st.push_log(format!(
                "Repeater {}",
                if enabled { "enabled" } else { "disabled" }
            ));
        }
        ControlEvent::QsyPending { token } => {
            st.pending_qsy_token = Some(token.clone());
            st.push_log(format!("QSY pending token {token}"));
        }
        ControlEvent::QsyDecision { token, accepted } => {
            if st.pending_qsy_token.as_deref() == Some(token.as_str()) {
                st.pending_qsy_token = None;
            }
            st.push_log(format!(
                "QSY {} for token {token}",
                if accepted { "accepted" } else { "rejected" }
            ));
        }
        ControlEvent::QsyIncoming {
            token,
            n_candidates,
        } => {
            st.push_log(format!(
                "QSY incoming from remote: token {token}, {n_candidates} candidates requested"
            ));
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
        ControlEvent::CommandError { command, reason } => {
            st.push_log(format!("CMD ERROR {command}: {reason}"));
        }
        ControlEvent::OtaStatus {
            active,
            tx_mode,
            tx_level,
            tx_fec,
            rx_recommended_level,
            rx_confirmed_level,
            is_locked,
        } => {
            st.ota_active = active;
            st.ota_tx_mode = tx_mode;
            st.ota_tx_level = tx_level;
            st.ota_tx_fec = tx_fec;
            st.ota_rx_recommended_level = rx_recommended_level;
            st.ota_rx_confirmed_level = rx_confirmed_level;
            st.ota_is_locked = is_locked;
        }
        // ── File transfer (FF-16 Phase D) ──
        ControlEvent::FileOffered {
            transfer_id,
            from,
            name,
            size,
            auto_accepted,
            signature_valid,
            ..
        } => {
            if !auto_accepted {
                st.incoming_offer = Some(IncomingOffer {
                    transfer_id,
                    from: from.clone(),
                    name: name.clone(),
                    size,
                    signature_valid,
                });
            }
            st.push_log(format!(
                "FILE OFFER {name} ({size} B) from {from}{}",
                if auto_accepted { " [auto]" } else { "" }
            ));
        }
        ControlEvent::FileProgress {
            transfer_id,
            direction,
            name,
            blocks_done,
            blocks_total,
            bytes_done,
            bytes_total,
        } => {
            st.active_transfer = Some(ActiveTransfer {
                transfer_id,
                direction,
                name,
                blocks_done,
                blocks_total,
                bytes_done,
                bytes_total,
            });
        }
        ControlEvent::FileReceived {
            transfer_id,
            from,
            name,
            size,
            path,
            verified,
        } => {
            if st.incoming_offer.as_ref().map(|o| o.transfer_id) == Some(transfer_id) {
                st.incoming_offer = None;
            }
            if st.active_transfer.as_ref().map(|t| t.transfer_id) == Some(transfer_id) {
                st.active_transfer = None;
            }
            st.received_files.insert(
                0,
                ReceivedFile {
                    name: name.clone(),
                    from: from.clone(),
                    size,
                    path,
                    verified,
                },
            );
            st.file_status = format!(
                "Received {name} from {from} — {}",
                if verified {
                    "verified ✓"
                } else {
                    "UNVERIFIED"
                }
            );
        }
        ControlEvent::FileSent {
            name,
            to,
            receipt_valid,
            ..
        } => {
            st.active_transfer = None;
            st.file_status = format!(
                "Sent {name} to {to} — {}",
                match receipt_valid {
                    Some(true) => "receipt ✓",
                    Some(false) => "receipt UNVERIFIED",
                    None => "no receipt",
                }
            );
        }
        ControlEvent::FileFailed {
            transfer_id,
            direction,
            reason,
        } => {
            if st.incoming_offer.as_ref().map(|o| o.transfer_id) == Some(transfer_id) {
                st.incoming_offer = None;
            }
            if st.active_transfer.as_ref().map(|t| t.transfer_id) == Some(transfer_id) {
                st.active_transfer = None;
            }
            st.file_status = format!("Transfer {direction} failed: {reason}");
        }
        _ => {}
    }
}

#[cfg(test)]
mod file_event_tests {
    use super::apply_event;
    use crate::state::PanelState;
    use openpulse_daemon::protocol::ControlEvent;
    use std::sync::{Arc, Mutex};

    fn feed(shared: &Arc<Mutex<PanelState>>, ev: ControlEvent) {
        apply_event(&serde_json::to_string(&ev).unwrap(), shared);
    }

    fn offered(transfer_id: u32, auto: bool) -> ControlEvent {
        ControlEvent::FileOffered {
            transfer_id,
            from: "W1AW".into(),
            name: "report.txt".into(),
            size: 100,
            sha256_hex: "ab".into(),
            mime: "text/plain".into(),
            auto_accepted: auto,
            signature_valid: true,
        }
    }

    #[test]
    fn file_offered_prompts_unless_auto_accepted() {
        let s = Arc::new(Mutex::new(PanelState::default()));
        feed(&s, offered(1, false));
        assert_eq!(
            s.lock()
                .unwrap()
                .incoming_offer
                .as_ref()
                .unwrap()
                .transfer_id,
            1
        );

        let s2 = Arc::new(Mutex::new(PanelState::default()));
        feed(&s2, offered(2, true));
        assert!(s2.lock().unwrap().incoming_offer.is_none());
    }

    #[test]
    fn file_received_records_and_clears_the_offer() {
        let s = Arc::new(Mutex::new(PanelState::default()));
        feed(&s, offered(3, false));
        feed(
            &s,
            ControlEvent::FileReceived {
                transfer_id: 3,
                from: "W1AW".into(),
                name: "report.txt".into(),
                size: 100,
                path: "/dl/report.txt".into(),
                verified: true,
            },
        );
        let st = s.lock().unwrap();
        assert!(st.incoming_offer.is_none());
        assert_eq!(st.received_files.len(), 1);
        assert!(st.received_files[0].verified);
        assert!(st.file_status.contains("verified"));
    }

    #[test]
    fn file_progress_then_failed_clears_active() {
        let s = Arc::new(Mutex::new(PanelState::default()));
        feed(
            &s,
            ControlEvent::FileProgress {
                transfer_id: 5,
                direction: "tx".into(),
                name: "a".into(),
                blocks_done: 1,
                blocks_total: 3,
                bytes_done: 10,
                bytes_total: 30,
            },
        );
        assert!(s.lock().unwrap().active_transfer.is_some());
        feed(
            &s,
            ControlEvent::FileFailed {
                transfer_id: 5,
                direction: "tx".into(),
                reason: "stall".into(),
            },
        );
        let st = s.lock().unwrap();
        assert!(st.active_transfer.is_none());
        assert!(st.file_status.contains("failed"));
    }
}
