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
use crate::{
    SharedAttenuation, SharedBandplanMode, SharedMessageStore, SharedMode, SharedQsyEnabled,
    SharedStationId, SharedTunerOnHighSWR, SpectrumTap, MAX_MESSAGES,
};

/// Shared state passed from the TCP control server to the WebSocket endpoint.
pub struct WsShared {
    pub ev_tx: Arc<broadcast::Sender<ControlEvent>>,
    pub cmd_tx: mpsc::Sender<ControlCommand>,
    pub active_mode: SharedMode,
    pub tx_attenuation_db: SharedAttenuation,
    pub qsy_enabled: SharedQsyEnabled,
    pub bandplan_mode: SharedBandplanMode,
    pub allow_tuner_on_high_swr: SharedTunerOnHighSWR,
    pub spectrum_tap: SpectrumTap,
    /// Station identity (callsign, grid_square) loaded from config at startup.
    pub station_id: SharedStationId,
    /// In-memory message store shared with the TCP endpoint.
    pub message_store: SharedMessageStore,
    /// Registered mode names for pre-write `SetMode`/`SetConfig` validation (shared with the TCP path).
    pub valid_modes: crate::ValidModes,
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
        qsy_enabled,
        bandplan_mode,
        allow_tuner_on_high_swr,
        spectrum_tap,
        station_id,
        message_store,
        valid_modes,
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
                        ev_tx: Arc::clone(&ev_tx),
                        ev_rx: ev_tx.subscribe(),
                        cmd_tx: cmd_tx.clone(),
                        active_mode: Arc::clone(&active_mode),
                        tx_attenuation_db: Arc::clone(&tx_attenuation_db),
                        qsy_enabled: Arc::clone(&qsy_enabled),
                        bandplan_mode: Arc::clone(&bandplan_mode),
                        allow_tuner_on_high_swr: Arc::clone(&allow_tuner_on_high_swr),
                        spectrum_tap: Arc::clone(&spectrum_tap),
                        station_id: Arc::clone(&station_id),
                        message_store: Arc::clone(&message_store),
                        valid_modes: Arc::clone(&valid_modes),
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
    ev_tx: Arc<broadcast::Sender<ControlEvent>>,
    ev_rx: broadcast::Receiver<ControlEvent>,
    cmd_tx: mpsc::Sender<ControlCommand>,
    active_mode: SharedMode,
    tx_attenuation_db: SharedAttenuation,
    qsy_enabled: SharedQsyEnabled,
    bandplan_mode: SharedBandplanMode,
    allow_tuner_on_high_swr: SharedTunerOnHighSWR,
    spectrum_tap: SpectrumTap,
    station_id: SharedStationId,
    message_store: SharedMessageStore,
    valid_modes: crate::ValidModes,
}

async fn handle_ws_client(stream: TcpStream, ctx: WsClientCtx) {
    let WsClientCtx {
        ev_tx,
        mut ev_rx,
        cmd_tx,
        active_mode,
        tx_attenuation_db,
        qsy_enabled,
        bandplan_mode,
        allow_tuner_on_high_swr,
        spectrum_tap,
        station_id,
        message_store,
        valid_modes,
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
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(lost = n, "WebSocket event receiver lagged; events dropped");
                        continue;
                    }
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

                        // Request-response commands handled inline; everything else falls to
                        // `dispatch_command`. This inline set MUST stay in sync with the TCP path
                        // (`lib.rs::handle_command`) — a request-response command added to one
                        // transport but not the other returns no data on the other. (Parity
                        // audited 2026-06-27.)
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
                                    let bins = ps.compute(&tap.read().await);
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
                            // Hold all locks simultaneously for a consistent snapshot.
                            let mode_guard = active_mode.lock().await;
                            let atten_guard = tx_attenuation_db.lock().await;
                            let qsy_guard = qsy_enabled.lock().await;
                            let bp_guard = bandplan_mode.lock().await;
                            let tuner_guard = allow_tuner_on_high_swr.lock().await;
                            let config = protocol::DaemonConfig {
                                callsign: cs,
                                grid_square: gs,
                                mode: mode_guard.clone(),
                                tx_attenuation_db: *atten_guard,
                                qsy_enabled: *qsy_guard,
                                bandplan_mode: bp_guard.clone(),
                                allow_tuner_on_high_swr: *tuner_guard,
                            };
                            drop(mode_guard);
                            drop(atten_guard);
                            drop(qsy_guard);
                            drop(bp_guard);
                            drop(tuner_guard);
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
                            let messages: Vec<MessageSummary> = message_store
                                .lock()
                                .await
                                .messages
                                .iter()
                                .map(|m| MessageSummary {
                                    id: m.id,
                                    from: m.from.clone(),
                                    to: m.to.clone(),
                                    subject: m.subject.clone(),
                                    timestamp_secs: m.timestamp_secs,
                                })
                                .collect();
                            let ev = ControlEvent::MessageList { messages };
                            if let Ok(s) = serde_json::to_string(&ev) {
                                if ws_tx.send(Message::Text(s)).await.is_err() {
                                    break;
                                }
                            }
                            if let Ok(s) = serde_json::to_string(&CommandResponse::ok()) {
                                if ws_tx.send(Message::Text(s)).await.is_err() {
                                    break;
                                }
                            }
                        } else if let ControlCommand::GetMessage { id } = &cmd {
                            let found = message_store
                                .lock()
                                .await
                                .messages
                                .iter()
                                .find(|m| m.id == *id)
                                .cloned();
                            match found {
                                None => {
                                    let resp = CommandResponse::err(format!("unknown id {id}"));
                                    if let Ok(s) = serde_json::to_string(&resp) {
                                        if ws_tx.send(Message::Text(s)).await.is_err() {
                                            break;
                                        }
                                    }
                                }
                                Some(m) => {
                                    let ev = ControlEvent::MessageData {
                                        id: m.id,
                                        from: m.from,
                                        to: m.to,
                                        subject: m.subject,
                                        body: m.body,
                                    };
                                    if let Ok(s) = serde_json::to_string(&ev) {
                                        if ws_tx.send(Message::Text(s)).await.is_err() {
                                            break;
                                        }
                                    }
                                    if let Ok(s) = serde_json::to_string(&CommandResponse::ok()) {
                                        if ws_tx.send(Message::Text(s)).await.is_err() {
                                            break;
                                        }
                                    }
                                }
                            }
                        } else if let ControlCommand::SendMessage { to, subject, body } = &cmd {
                            use std::time::{SystemTime, UNIX_EPOCH};
                            let from = station_id.lock().await.0.clone();
                            let timestamp_secs = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs();
                            let id = {
                                let mut store = message_store.lock().await;
                                let id = store.alloc_id();
                                store.messages.push_back(crate::StoredMessage {
                                    id,
                                    from: from.clone(),
                                    to: to.clone(),
                                    subject: subject.clone(),
                                    body: body.clone(),
                                    timestamp_secs,
                                });
                                if store.messages.len() > MAX_MESSAGES {
                                    let _ = store.messages.pop_front();
                                }
                                id
                            };
                            let preview: String = body.chars().take(120).collect();
                            // Broadcast MessageReceived to all clients (TCP + WS) via shared ev_tx.
                            let ev = ControlEvent::MessageReceived {
                                id,
                                from,
                                to: to.clone(),
                                subject: subject.clone(),
                                preview,
                                timestamp_secs,
                            };
                            let _ = ev_tx.send(ev);
                            // Forward to daemon main for RF dispatch.
                            let _ = cmd_tx.send(cmd.clone()).await;
                            // ok is sent directly; ev_rx delivers MessageReceived in the next tick.
                            if let Ok(s) = serde_json::to_string(&CommandResponse::ok()) {
                                if ws_tx.send(Message::Text(s)).await.is_err() {
                                    break;
                                }
                            }
                        } else if let ControlCommand::DeleteMessage { id } = &cmd {
                            message_store
                                .lock()
                                .await
                                .messages
                                .retain(|m| m.id != *id);
                            if let Ok(s) = serde_json::to_string(&CommandResponse::ok()) {
                                if ws_tx.send(Message::Text(s)).await.is_err() {
                                    break;
                                }
                            }
                        } else {
                            let resp = crate::dispatch_command(
                                &cmd,
                                &cmd_tx,
                                &active_mode,
                                &tx_attenuation_db,
                                &qsy_enabled,
                                &bandplan_mode,
                                &allow_tuner_on_high_swr,
                                &valid_modes,
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
