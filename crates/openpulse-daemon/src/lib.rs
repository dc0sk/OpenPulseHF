//! NDJSON-over-TCP control server for the OpenPulse daemon.
//!
//! [`ControlServer::spawn`] binds a TCP listener and accepts one or more
//! concurrent client connections.  Each client receives the full unsolicited
//! [`ControlEvent`] stream and may send [`ControlCommand`] lines which are
//! dispatched back to the caller via an `mpsc` channel.
//!
//! Clients that send [`ControlCommand::SubscribeSpectrum`] receive binary
//! spectrum frames interleaved with the NDJSON event stream on the same
//! connection.  See [`protocol::encode_spectrum_frame`] for the wire format.

pub mod protocol;
pub mod ws;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use openpulse_channel::dsp::PowerSpectrum;
use openpulse_modem::ModemEngine;
use protocol::{encode_spectrum_frame, CommandResponse, ControlCommand, ControlEvent};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc, Mutex, RwLock};

pub use protocol::ControlCommand as Command;
pub use protocol::ControlEvent as Event;

/// Shared mutable mode string, written by `set_mode` commands.
pub type SharedMode = Arc<Mutex<String>>;
/// Shared mutable TX attenuation (dB), written by `set_tx_attenuation` commands.
pub type SharedAttenuation = Arc<Mutex<f32>>;
/// Shared audio sample tap for spectrum computation (most-recent 1024 samples).
pub type SpectrumTap = Arc<RwLock<Vec<f32>>>;
/// Shared station identity: `(callsign, grid_square)`, set at startup.
pub type SharedStationId = Arc<Mutex<(String, String)>>;

/// Handle returned by [`ControlServer::spawn`].
///
/// Dropping this handle does *not* stop the server — use [`ControlServerHandle::shutdown`]
/// for a clean stop (or just let the process exit).
pub struct ControlServerHandle {
    /// Receives every [`ControlCommand`] dispatched from any connected client.
    pub commands: mpsc::Receiver<ControlCommand>,
    /// Sender for the shared event broadcast (pass to [`ws::spawn_ws`] to
    /// share state between the TCP and WebSocket control endpoints).
    pub event_tx: Arc<broadcast::Sender<ControlEvent>>,
    /// mpsc sender for injecting commands programmatically (used by WebSocket endpoint).
    pub command_tx: mpsc::Sender<ControlCommand>,
    /// Current active mode string (also updated by the command handler).
    pub active_mode: SharedMode,
    /// Current TX attenuation in dB (also updated by the command handler).
    pub tx_attenuation_db: SharedAttenuation,
    /// Audio sample tap; caller may write recent RX samples here.
    pub spectrum_tap: SpectrumTap,
    /// Station callsign and grid square loaded from config at startup.
    pub station_id: SharedStationId,
}

/// NDJSON-over-TCP control server.
pub struct ControlServer;

impl ControlServer {
    /// Spawn the control server on `addr`.
    ///
    /// `engine` is used to subscribe to the event broadcast channel.
    /// The bound address is written to `bound_addr` if provided (useful in
    /// tests that bind on port 0 and need the ephemeral port).
    pub async fn spawn(
        addr: SocketAddr,
        engine: &ModemEngine,
        initial_mode: String,
        initial_station_id: (String, String),
        bound_addr: Option<&mut SocketAddr>,
    ) -> Result<ControlServerHandle, std::io::Error> {
        let listener = TcpListener::bind(addr).await?;
        if let Some(out) = bound_addr {
            *out = listener.local_addr()?;
        }

        // Broadcast sender for ControlEvents; server tasks subscribe from it.
        let (ev_tx, _) = broadcast::channel::<ControlEvent>(256);
        let ev_tx = Arc::new(ev_tx);

        // mpsc for commands from all clients → caller.
        let (cmd_tx, cmd_rx) = mpsc::channel::<ControlCommand>(64);

        let active_mode = Arc::new(Mutex::new(initial_mode));
        let tx_attenuation_db: SharedAttenuation = Arc::new(Mutex::new(0.0f32));
        let spectrum_tap: SpectrumTap = Arc::new(RwLock::new(vec![0.0f32; 1024]));
        let station_id: SharedStationId = Arc::new(Mutex::new(initial_station_id));

        // Background task: forward EngineEvents into the ControlEvent broadcast.
        let mut eng_rx = engine.subscribe();
        let ev_tx_clone = Arc::clone(&ev_tx);
        tokio::spawn(async move {
            loop {
                match eng_rx.recv().await {
                    Ok(ev) => {
                        let _ = ev_tx_clone.send(ControlEvent::EngineEvent { event: ev });
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        // Background task: periodic Metrics events at 1 Hz.
        let ev_tx_metrics = Arc::clone(&ev_tx);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            loop {
                interval.tick().await;
                let ev = ControlEvent::Metrics {
                    effective_bps: 0.0,
                    ecc_rate: 0.0,
                    compress_ratio: 1.0,
                    afc_correction_hz: 0.0,
                    signal_strength_dbm: None,
                };
                let _ = ev_tx_metrics.send(ev);
            }
        });

        // Acceptor task.
        let ev_tx_accept = Arc::clone(&ev_tx);
        let cmd_tx_accept = cmd_tx.clone();
        let mode_accept = Arc::clone(&active_mode);
        let atten_accept = Arc::clone(&tx_attenuation_db);
        let tap_accept = Arc::clone(&spectrum_tap);
        let sid_accept = Arc::clone(&station_id);
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer)) => {
                        tracing::info!(%peer, "control port: client connected");
                        let rx = ev_tx_accept.subscribe();
                        let cmd_tx = cmd_tx_accept.clone();
                        let mode = Arc::clone(&mode_accept);
                        let atten = Arc::clone(&atten_accept);
                        let tap = Arc::clone(&tap_accept);
                        let sid = Arc::clone(&sid_accept);
                        tokio::spawn(handle_client(stream, rx, cmd_tx, mode, atten, tap, sid));
                    }
                    Err(e) => {
                        tracing::warn!("control port accept error: {e}");
                    }
                }
            }
        });

        Ok(ControlServerHandle {
            commands: cmd_rx,
            event_tx: ev_tx,
            command_tx: cmd_tx,
            active_mode,
            tx_attenuation_db,
            spectrum_tap,
            station_id,
        })
    }
}

/// Per-client handler: streams events out, reads commands in, optionally sends
/// binary spectrum frames when the client has subscribed.
async fn handle_client(
    stream: TcpStream,
    mut ev_rx: broadcast::Receiver<ControlEvent>,
    cmd_tx: mpsc::Sender<ControlCommand>,
    active_mode: SharedMode,
    tx_attenuation_db: SharedAttenuation,
    spectrum_tap: SpectrumTap,
    station_id: SharedStationId,
) {
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    // Per-client channel for binary spectrum frames produced by the spectrum task.
    let (spec_frame_tx, mut spec_frame_rx) = mpsc::channel::<Vec<u8>>(4);
    let mut spectrum_task: Option<tokio::task::JoinHandle<()>> = None;

    loop {
        tokio::select! {
            // Binary spectrum frames (only populated when client has subscribed).
            Some(frame) = spec_frame_rx.recv() => {
                if write_half.write_all(&frame).await.is_err() {
                    break;
                }
            }
            // Outbound: forward events to client.
            result = ev_rx.recv() => {
                match result {
                    Ok(ev) => {
                        let mut line = match serde_json::to_string(&ev) {
                            Ok(s) => s,
                            Err(_) => continue,
                        };
                        line.push('\n');
                        if write_half.write_all(line.as_bytes()).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            // Inbound: read one command line.
            result = lines.next_line() => {
                match result {
                    Ok(Some(line)) if !line.trim().is_empty() => {
                        // Parse command first to check for SubscribeSpectrum / GetConfig.
                        let cmd: ControlCommand = match serde_json::from_str(line.trim()) {
                            Ok(c) => c,
                            Err(e) => {
                                let resp = CommandResponse::err(format!("parse error: {e}"));
                                let _ = send_json(&mut write_half, &resp).await;
                                continue;
                            }
                        };

                        if let ControlCommand::SubscribeSpectrum { fps } = &cmd {
                            let fps = (*fps).clamp(1, 100);
                            // Cancel existing spectrum task for this client, if any.
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
                            let _ = send_json(&mut write_half, &resp).await;
                        } else if matches!(cmd, ControlCommand::GetConfig) {
                            let (cs, gs) = station_id.lock().await.clone();
                            let config = protocol::DaemonConfig {
                                callsign: cs,
                                grid_square: gs,
                                mode: active_mode.lock().await.clone(),
                                tx_attenuation_db: *tx_attenuation_db.lock().await,
                            };
                            let ev = ControlEvent::ConfigData { config };
                            if send_json(&mut write_half, &ev).await.is_err() {
                                break;
                            }
                            // Send ok after ConfigData so clients awaiting a CommandResponse
                            // do not hang.
                            let resp = CommandResponse::ok();
                            if send_json(&mut write_half, &resp).await.is_err() {
                                break;
                            }
                        } else {
                            let resp =
                                dispatch_command(&line, &cmd_tx, &active_mode, &tx_attenuation_db)
                                    .await;
                            if send_json(&mut write_half, &resp).await.is_err() {
                                break;
                            }
                        }
                    }
                    Ok(None) => break, // client disconnected
                    Ok(Some(_)) => {}  // empty line, ignore
                    Err(_) => break,
                }
            }
        }
    }

    if let Some(h) = spectrum_task {
        h.abort();
    }
}

/// Serialise `value` as JSON + newline and write it to `writer`.
async fn send_json<T: serde::Serialize>(
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    value: &T,
) -> Result<(), std::io::Error> {
    let mut s = serde_json::to_string(value).unwrap_or_default();
    s.push('\n');
    writer.write_all(s.as_bytes()).await
}

/// Parse and dispatch a single command line; returns the response.
pub(crate) async fn dispatch_command(
    line: &str,
    cmd_tx: &mpsc::Sender<ControlCommand>,
    active_mode: &SharedMode,
    tx_attenuation_db: &SharedAttenuation,
) -> CommandResponse {
    let cmd: ControlCommand = match serde_json::from_str(line) {
        Ok(c) => c,
        Err(e) => return CommandResponse::err(format!("parse error: {e}")),
    };

    // Apply commands that modify local state immediately; forward all commands to caller.
    if let ControlCommand::SetMode { ref mode } = cmd {
        *active_mode.lock().await = mode.clone();
    }
    if let ControlCommand::SetTxAttenuation { db, .. } = cmd {
        *tx_attenuation_db.lock().await = db;
    }
    // SetConfig: hold both locks simultaneously to update mode and attenuation atomically.
    if let ControlCommand::SetConfig { ref config } = cmd {
        let mut mode_guard = active_mode.lock().await;
        let mut atten_guard = tx_attenuation_db.lock().await;
        *mode_guard = config.mode.clone();
        *atten_guard = config.tx_attenuation_db;
    }

    if cmd_tx.send(cmd).await.is_err() {
        return CommandResponse::err("server shutting down");
    }

    CommandResponse::ok()
}
