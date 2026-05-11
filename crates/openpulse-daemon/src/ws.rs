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
use openpulse_modem::ModemEngine;
use protocol::{encode_spectrum_frame, CommandResponse, ControlCommand, ControlEvent};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::{accept_async, tungstenite::Message};

use crate::protocol;
use crate::{SharedAttenuation, SharedCallsign, SharedMode, SpectrumTap};

/// Shared state passed from the TCP control server to the WebSocket endpoint.
pub struct WsShared {
    pub ev_tx: Arc<broadcast::Sender<ControlEvent>>,
    pub cmd_tx: mpsc::Sender<ControlCommand>,
    pub active_mode: SharedMode,
    pub tx_attenuation_db: SharedAttenuation,
    pub spectrum_tap: SpectrumTap,
    pub callsign: SharedCallsign,
}

/// Spawn the WebSocket control endpoint on `addr`.
///
/// Shares the same event broadcast, command channel, and shared-state arcs as
/// the TCP control server so both frontends see identical state.
pub async fn spawn_ws(
    addr: SocketAddr,
    engine: &ModemEngine,
    shared: WsShared,
    bound_addr: Option<&mut SocketAddr>,
) -> Result<(), std::io::Error> {
    let WsShared {
        ev_tx,
        cmd_tx,
        active_mode,
        tx_attenuation_db,
        spectrum_tap,
        callsign,
    } = shared;
    let listener = TcpListener::bind(addr).await?;
    if let Some(out) = bound_addr {
        *out = listener.local_addr()?;
    }

    // Forward EngineEvents into the shared broadcast (no-op if TCP server already does it;
    // the broadcast sender is shared so duplicate subscriptions are fine).
    let mut eng_rx = engine.subscribe();
    let ev_fwd = Arc::clone(&ev_tx);
    tokio::spawn(async move {
        loop {
            match eng_rx.recv().await {
                Ok(ev) => {
                    let _ = ev_fwd.send(ControlEvent::EngineEvent { event: ev });
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, peer)) => {
                    tracing::info!(%peer, "WebSocket control: client connected");
                    let rx = ev_tx.subscribe();
                    let cmd_tx = cmd_tx.clone();
                    let mode = Arc::clone(&active_mode);
                    let atten = Arc::clone(&tx_attenuation_db);
                    let tap = Arc::clone(&spectrum_tap);
                    let cs = Arc::clone(&callsign);
                    tokio::spawn(handle_ws_client(stream, rx, cmd_tx, mode, atten, tap, cs));
                }
                Err(e) => tracing::warn!("WebSocket accept error: {e}"),
            }
        }
    });

    Ok(())
}

async fn handle_ws_client(
    stream: TcpStream,
    mut ev_rx: broadcast::Receiver<ControlEvent>,
    cmd_tx: mpsc::Sender<ControlCommand>,
    active_mode: SharedMode,
    tx_attenuation_db: SharedAttenuation,
    spectrum_tap: SpectrumTap,
    callsign: SharedCallsign,
) {
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
                                let _ = ws_tx.send(Message::Text(s)).await;
                            }
                        } else if matches!(cmd, ControlCommand::GetConfig) {
                            let (cs, gs) = callsign.lock().await.clone();
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
                        } else {
                            let resp = crate::dispatch_command(
                                line,
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
                    Some(Ok(_)) => {} // ping/pong/binary from client — ignore
                }
            }
        }
    }

    if let Some(h) = spectrum_task {
        h.abort();
    }
}
