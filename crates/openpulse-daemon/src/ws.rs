//! WebSocket control endpoint — same NDJSON protocol as the TCP port but
//! carried over WebSocket frames.
//!
//! Text frames carry JSON-encoded [`ControlEvent`] and [`CommandResponse`]
//! messages.  Binary frames carry power-spectrum data encoded by
//! [`encode_spectrum_frame`].  Clients that want spectrum must send a
//! `SubscribeSpectrum` command after connecting.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use openpulse_channel::dsp::PowerSpectrum;
use protocol::{encode_spectrum_frame, CommandResponse, ControlCommand, ControlEvent};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::{accept_async, tungstenite::Message};

use crate::protocol::{self, MessageSummary};
use crate::{SharedAttenuation, SharedMessageStore, SharedMode, SharedStationId, SpectrumTap};

/// Shared state passed from the TCP control server to the WebSocket endpoint.
pub struct WsShared {
    pub ev_tx: Arc<broadcast::Sender<ControlEvent>>,
    pub cmd_tx: mpsc::Sender<ControlCommand>,
    pub active_mode: SharedMode,
    pub tx_attenuation_db: SharedAttenuation,
    pub spectrum_tap: SpectrumTap,
    /// Station identity (callsign, grid_square) loaded from config at startup.
    pub station_id: SharedStationId,
    /// In-memory message store shared with the TCP endpoint.
    pub message_store: SharedMessageStore,
}

/// Spawn the WebSocket control endpoint on `addr`.
///
/// Shares the same event broadcast, command channel, and shared-state arcs as
/// the TCP control server so both frontends see identical state.  The TCP
/// control server already forwards engine events into `shared.ev_tx`; this
/// function does not duplicate that subscription.
pub async fn spawn_ws(
    addr: SocketAddr,
    shared: WsShared,
    bound_addr: Option<&mut SocketAddr>,
) -> Result<(), std::io::Error> {
    let WsShared {
        ev_tx,
        cmd_tx,
        active_mode,
        tx_attenuation_db,
        spectrum_tap,
        station_id,
        message_store,
    } = shared;
    let listener = TcpListener::bind(addr).await?;
    if let Some(out) = bound_addr {
        *out = listener.local_addr()?;
    }

    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, peer)) => {
                    tracing::info!(%peer, "WebSocket control: client connected");
                    let ctx = WsClientCtx {
                        ev_rx: ev_tx.subscribe(),
                        cmd_tx: cmd_tx.clone(),
                        active_mode: Arc::clone(&active_mode),
                        tx_attenuation_db: Arc::clone(&tx_attenuation_db),
                        spectrum_tap: Arc::clone(&spectrum_tap),
                        station_id: Arc::clone(&station_id),
                        message_store: Arc::clone(&message_store),
                    };
                    tokio::spawn(handle_ws_client(stream, ctx));
                }
                Err(e) => tracing::warn!("WebSocket accept error: {e}"),
            }
        }
    });

    Ok(())
}

struct WsClientCtx {
    ev_rx: broadcast::Receiver<ControlEvent>,
    cmd_tx: mpsc::Sender<ControlCommand>,
    active_mode: SharedMode,
    tx_attenuation_db: SharedAttenuation,
    spectrum_tap: SpectrumTap,
    station_id: SharedStationId,
    message_store: SharedMessageStore,
}

async fn handle_ws_client(stream: TcpStream, ctx: WsClientCtx) {
    let WsClientCtx {
        mut ev_rx,
        cmd_tx,
        active_mode,
        tx_attenuation_db,
        spectrum_tap,
        station_id,
        message_store,
    } = ctx;
    let ws = match accept_async(stream).await {
        Ok(w) => w,
        Err(e) => {
            tracing::warn!("WebSocket handshake failed: {e}");
            return;
        }
    };
    let (mut ws_tx, mut ws_rx) = ws.split();

    let (spec_frame_tx, mut spec_frame_rx) = mpsc::channel::<Vec<u8>>(4);
    let mut spectrum_task: Option<tokio::task::JoinHandle<()>> = None;

    loop {
        tokio::select! {
            // Binary spectrum frames.
            Some(frame) = spec_frame_rx.recv() => {
                if ws_tx.send(Message::Binary(frame)).await.is_err() {
                    break;
                }
            }
            // Outbound events.
            result = ev_rx.recv() => {
                match result {
                    Ok(ev) => {
                        if let Ok(s) = serde_json::to_string(&ev) {
                            if ws_tx.send(Message::Text(s)).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            // Inbound commands.
            result = ws_rx.next() => {
                match result {
                    None => break,
                    Some(Err(_)) => break,
                    Some(Ok(Message::Close(_))) => break,
                    Some(Ok(Message::Ping(payload))) => {
                        if ws_tx.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Text(text))) => {
                        let line = text.trim();
                        if line.is_empty() { continue; }

                        let cmd: ControlCommand = match serde_json::from_str(line) {
                            Ok(c) => c,
                            Err(e) => {
                                let resp = CommandResponse::err(format!("parse error: {e}"));
                                if let Ok(s) = serde_json::to_string(&resp) {
                                    let _ = ws_tx.send(Message::Text(s)).await;
                                }
                                continue;
                            }
                        };

                        if let ControlCommand::SubscribeSpectrum { fps } = &cmd {
                            let fps = (*fps).clamp(1, 100);
                            if let Some(h) = spectrum_task.take() {
                                h.abort();
                            }
                            let tap = Arc::clone(&spectrum_tap);
                            let tx = spec_frame_tx.clone();
                            let period = Duration::from_millis(1000 / fps as u64);
                            spectrum_task = Some(tokio::spawn(async move {
                                let mut interval = tokio::time::interval(period);
                                let mut ps = PowerSpectrum::new();
                                loop {
                                    interval.tick().await;
                                    let samples = tap.read().await.clone();
                                    let bins = ps.compute(&samples);
                                    let frame = encode_spectrum_frame(8000, &bins);
                                    if tx.send(frame).await.is_err() {
                                        break;
                                    }
                                }
                            }));
                            let resp = CommandResponse::ok();
                            if let Ok(s) = serde_json::to_string(&resp) {
                                if ws_tx.send(Message::Text(s)).await.is_err() {
                                    break;
                                }
                            }
                        } else if matches!(cmd, ControlCommand::GetConfig) {
                            let (cs, gs) = station_id.lock().await.clone();
                            let config = protocol::DaemonConfig {
                                callsign: cs,
                                grid_square: gs,
                                mode: active_mode.lock().await.clone(),
                                tx_attenuation_db: *tx_attenuation_db.lock().await,
                            };
                            let ev = ControlEvent::ConfigData { config };
                            if let Ok(s) = serde_json::to_string(&ev) {
                                if ws_tx.send(Message::Text(s)).await.is_err() {
                                    break;
                                }
                            }
                            let resp = CommandResponse::ok();
                            if let Ok(s) = serde_json::to_string(&resp) {
                                if ws_tx.send(Message::Text(s)).await.is_err() {
                                    break;
                                }
                            }
                        } else if matches!(cmd, ControlCommand::ListMessages) {
                            let messages: Vec<MessageSummary> = message_store.lock().await
                                .iter()
                                .map(|m| MessageSummary {
                                    id: m.id, from: m.from.clone(), to: m.to.clone(),
                                    subject: m.subject.clone(), timestamp_secs: m.timestamp_secs,
                                })
                                .collect();
                            let ev = ControlEvent::MessageList { messages };
                            if let Ok(s) = serde_json::to_string(&ev) {
                                if ws_tx.send(Message::Text(s)).await.is_err() { break; }
                            }
                            if let Ok(s) = serde_json::to_string(&CommandResponse::ok()) {
                                if ws_tx.send(Message::Text(s)).await.is_err() { break; }
                            }
                        } else if let ControlCommand::GetMessage { id } = &cmd {
                            let found = message_store.lock().await.iter().find(|m| m.id == *id).cloned();
                            match found {
                                None => {
                                    let resp = CommandResponse::err(format!("unknown id {id}"));
                                    if let Ok(s) = serde_json::to_string(&resp) {
                                        if ws_tx.send(Message::Text(s)).await.is_err() { break; }
                                    }
                                }
                                Some(m) => {
                                    let ev = ControlEvent::MessageData {
                                        id: m.id, from: m.from, to: m.to,
                                        subject: m.subject, body: m.body,
                                    };
                                    if let Ok(s) = serde_json::to_string(&ev) {
                                        if ws_tx.send(Message::Text(s)).await.is_err() { break; }
                                    }
                                    if let Ok(s) = serde_json::to_string(&CommandResponse::ok()) {
                                        if ws_tx.send(Message::Text(s)).await.is_err() { break; }
                                    }
                                }
                            }
                        } else if let ControlCommand::SendMessage { to, subject, body } = &cmd {
                            use std::time::{SystemTime, UNIX_EPOCH};
                            let from = station_id.lock().await.0.clone();
                            let timestamp_secs = SystemTime::now()
                                .duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
                            let id = {
                                let mut store = message_store.lock().await;
                                let id = store.len() as u64 + 1;
                                store.push(crate::StoredMessage {
                                    id, from: from.clone(), to: to.clone(),
                                    subject: subject.clone(), body: body.clone(), timestamp_secs,
                                });
                                id
                            };
                            let preview: String = body.chars().take(120).collect();
                            // Broadcast to all clients via shared ev_tx (not available in WS handler —
                            // forward via cmd_tx so the daemon main can broadcast if needed).
                            let _ = cmd_tx.send(cmd.clone()).await;
                            // Send MessageReceived only to this client for now.
                            let ev = ControlEvent::MessageReceived {
                                id, from, to: to.clone(), subject: subject.clone(), preview,
                            };
                            if let Ok(s) = serde_json::to_string(&ev) {
                                if ws_tx.send(Message::Text(s)).await.is_err() { break; }
                            }
                            if let Ok(s) = serde_json::to_string(&CommandResponse::ok()) {
                                if ws_tx.send(Message::Text(s)).await.is_err() { break; }
                            }
                        } else if let ControlCommand::DeleteMessage { id } = &cmd {
                            message_store.lock().await.retain(|m| m.id != *id);
                            if let Ok(s) = serde_json::to_string(&CommandResponse::ok()) {
                                if ws_tx.send(Message::Text(s)).await.is_err() { break; }
                            }
                        } else {
                            let resp = crate::dispatch_command(
                                &cmd,
                                &cmd_tx,
                                &active_mode,
                                &tx_attenuation_db,
                            )
                            .await;
                            if let Ok(s) = serde_json::to_string(&resp) {
                                if ws_tx.send(Message::Text(s)).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    Some(Ok(_)) => {} // pong/binary from client — ignore
                }
            }
        }
    }

    if let Some(h) = spectrum_task {
        h.abort();
    }
}
