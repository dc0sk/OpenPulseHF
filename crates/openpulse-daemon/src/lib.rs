//! NDJSON-over-TCP control server for the OpenPulse daemon.
//!
//! [`ControlServer::spawn`] binds a TCP listener and accepts one or more
//! concurrent client connections.  Each client receives the full unsolicited
//! [`ControlEvent`] stream and may send [`ControlCommand`] lines which are
//! dispatched back to the caller via an `mpsc` channel.

pub mod protocol;

use std::net::SocketAddr;
use std::sync::Arc;

use openpulse_modem::ModemEngine;
use protocol::{CommandResponse, ControlCommand, ControlEvent};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc, Mutex};

pub use protocol::ControlCommand as Command;
pub use protocol::ControlEvent as Event;

/// Shared mutable mode string, written by `set_mode` commands.
pub type SharedMode = Arc<Mutex<String>>;

/// Handle returned by [`ControlServer::spawn`].
///
/// Dropping this handle does *not* stop the server — use [`ControlServerHandle::shutdown`]
/// for a clean stop (or just let the process exit).
pub struct ControlServerHandle {
    /// Receives every [`ControlCommand`] dispatched from any connected client.
    pub commands: mpsc::Receiver<ControlCommand>,
    /// Current active mode string (also updated by the command handler).
    pub active_mode: SharedMode,
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
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
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
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer)) => {
                        tracing::info!(%peer, "control port: client connected");
                        let rx = ev_tx_accept.subscribe();
                        let cmd_tx = cmd_tx_accept.clone();
                        let mode = Arc::clone(&mode_accept);
                        tokio::spawn(handle_client(stream, rx, cmd_tx, mode));
                    }
                    Err(e) => {
                        tracing::warn!("control port accept error: {e}");
                    }
                }
            }
        });

        Ok(ControlServerHandle {
            commands: cmd_rx,
            active_mode,
        })
    }
}

/// Per-client handler: streams events out, reads commands in.
async fn handle_client(
    stream: TcpStream,
    mut ev_rx: broadcast::Receiver<ControlEvent>,
    cmd_tx: mpsc::Sender<ControlCommand>,
    active_mode: SharedMode,
) {
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    loop {
        tokio::select! {
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
                        let resp = dispatch_command(&line, &cmd_tx, &active_mode).await;
                        let mut out = match serde_json::to_string(&resp) {
                            Ok(s) => s,
                            Err(_) => continue,
                        };
                        out.push('\n');
                        if write_half.write_all(out.as_bytes()).await.is_err() {
                            break;
                        }
                    }
                    Ok(None) => break, // client disconnected
                    Ok(Some(_)) => {}  // empty line, ignore
                    Err(_) => break,
                }
            }
        }
    }
}

/// Parse and dispatch a single command line; returns the response.
async fn dispatch_command(
    line: &str,
    cmd_tx: &mpsc::Sender<ControlCommand>,
    active_mode: &SharedMode,
) -> CommandResponse {
    let cmd: ControlCommand = match serde_json::from_str(line) {
        Ok(c) => c,
        Err(e) => return CommandResponse::err(format!("parse error: {e}")),
    };

    // Apply commands that modify local state immediately; forward all commands to caller.
    if let ControlCommand::SetMode { ref mode } = cmd {
        *active_mode.lock().await = mode.clone();
    }

    if cmd_tx.send(cmd).await.is_err() {
        return CommandResponse::err("server shutting down");
    }

    CommandResponse::ok()
}
