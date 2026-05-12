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

/// WebSocket control endpoint — native server builds only.
#[cfg(not(target_arch = "wasm32"))]
pub mod ws;

#[cfg(not(target_arch = "wasm32"))]
use std::net::SocketAddr;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(not(target_arch = "wasm32"))]
use openpulse_channel::dsp::PowerSpectrum;
#[cfg(not(target_arch = "wasm32"))]
use openpulse_modem::ModemEngine;
#[cfg(not(target_arch = "wasm32"))]
use protocol::{
    encode_spectrum_frame, CommandResponse, ControlCommand, ControlEvent, MessageSummary,
};
#[cfg(not(target_arch = "wasm32"))]
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
#[cfg(not(target_arch = "wasm32"))]
use tokio::net::{TcpListener, TcpStream};
#[cfg(not(target_arch = "wasm32"))]
use tokio::sync::{broadcast, mpsc, Mutex, RwLock};

pub use protocol::ControlCommand as Command;
pub use protocol::ControlEvent as Event;

/// Shared mutable mode string, written by `set_mode` commands.
#[cfg(not(target_arch = "wasm32"))]
pub type SharedMode = Arc<Mutex<String>>;
/// Shared mutable TX attenuation (dB), written by `set_tx_attenuation` commands.
#[cfg(not(target_arch = "wasm32"))]
pub type SharedAttenuation = Arc<Mutex<f32>>;
/// Shared audio sample tap for spectrum computation (most-recent 1024 samples).
#[cfg(not(target_arch = "wasm32"))]
pub type SpectrumTap = Arc<RwLock<Vec<f32>>>;
/// Shared station identity strings (callsign + grid square), set at startup.
#[cfg(not(target_arch = "wasm32"))]
pub type SharedStationId = Arc<Mutex<(String, String)>>;
/// Shared in-memory message store (sent and received messages).
#[cfg(not(target_arch = "wasm32"))]
pub type SharedMessageStore = Arc<Mutex<MessageStore>>;

/// Maximum number of messages kept in memory; oldest are evicted when full.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const MAX_MESSAGES: usize = 500;

/// A single stored message (sent or received).
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone)]
pub struct StoredMessage {
    pub id: u64,
    pub from: String,
    pub to: String,
    pub subject: String,
    pub body: String,
    pub timestamp_secs: u64,
}

/// In-memory inbox with a monotonically increasing ID counter.
#[cfg(not(target_arch = "wasm32"))]
pub struct MessageStore {
    next_id: u64,
    pub messages: Vec<StoredMessage>,
}

#[cfg(not(target_arch = "wasm32"))]
impl MessageStore {
    fn new() -> Self {
        Self {
            next_id: 1,
            messages: Vec::new(),
        }
    }

    /// Allocate the next unique message ID.
    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}

/// All shared state passed to each per-client TCP handler.
#[cfg(not(target_arch = "wasm32"))]
struct ClientCtx {
    ev_tx: Arc<broadcast::Sender<ControlEvent>>,
    cmd_tx: mpsc::Sender<ControlCommand>,
    active_mode: SharedMode,
    tx_attenuation_db: SharedAttenuation,
    spectrum_tap: SpectrumTap,
    station_id: SharedStationId,
    message_store: SharedMessageStore,
}

/// Handle returned by [`ControlServer::spawn`].
///
/// Dropping this handle does *not* stop the server — use [`ControlServerHandle::shutdown`]
/// for a clean stop (or just let the process exit).
#[cfg(not(target_arch = "wasm32"))]
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
    /// In-memory message store shared across all control endpoints.
    pub message_store: SharedMessageStore,
}

/// NDJSON-over-TCP control server.
#[cfg(not(target_arch = "wasm32"))]
pub struct ControlServer;

#[cfg(not(target_arch = "wasm32"))]
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

        let (ev_tx, _) = broadcast::channel::<ControlEvent>(256);
        let ev_tx = Arc::new(ev_tx);
        let (cmd_tx, cmd_rx) = mpsc::channel::<ControlCommand>(64);

        let active_mode = Arc::new(Mutex::new(initial_mode));
        let tx_attenuation_db: SharedAttenuation = Arc::new(Mutex::new(0.0f32));
        let spectrum_tap: SpectrumTap = Arc::new(RwLock::new(vec![0.0f32; 1024]));
        let station_id: SharedStationId = Arc::new(Mutex::new(initial_station_id));
        let message_store: SharedMessageStore = Arc::new(Mutex::new(MessageStore::new()));

        // Background task: forward EngineEvents into the ControlEvent broadcast.
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

        // Background task: periodic Metrics events at 1 Hz.
        let ev_metrics = Arc::clone(&ev_tx);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            loop {
                interval.tick().await;
                let _ = ev_metrics.send(ControlEvent::Metrics {
                    effective_bps: 0.0,
                    ecc_rate: 0.0,
                    compress_ratio: 1.0,
                    afc_correction_hz: 0.0,
                    signal_strength_dbm: None,
                });
            }
        });

        // Acceptor task.
        let ev_tx_a = Arc::clone(&ev_tx);
        let cmd_tx_a = cmd_tx.clone();
        let mode_a = Arc::clone(&active_mode);
        let atten_a = Arc::clone(&tx_attenuation_db);
        let tap_a = Arc::clone(&spectrum_tap);
        let sid_a = Arc::clone(&station_id);
        let store_a = Arc::clone(&message_store);
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer)) => {
                        tracing::info!(%peer, "control port: client connected");
                        let ctx = ClientCtx {
                            ev_tx: Arc::clone(&ev_tx_a),
                            cmd_tx: cmd_tx_a.clone(),
                            active_mode: Arc::clone(&mode_a),
                            tx_attenuation_db: Arc::clone(&atten_a),
                            spectrum_tap: Arc::clone(&tap_a),
                            station_id: Arc::clone(&sid_a),
                            message_store: Arc::clone(&store_a),
                        };
                        let rx = ev_tx_a.subscribe();
                        tokio::spawn(handle_client(stream, rx, ctx));
                    }
                    Err(e) => tracing::warn!("control port accept error: {e}"),
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
            message_store,
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn handle_client(
    stream: TcpStream,
    mut ev_rx: broadcast::Receiver<ControlEvent>,
    ctx: ClientCtx,
) {
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    let (spec_frame_tx, mut spec_frame_rx) = mpsc::channel::<Vec<u8>>(4);
    let mut spectrum_task: Option<tokio::task::JoinHandle<()>> = None;

    loop {
        tokio::select! {
            Some(frame) = spec_frame_rx.recv() => {
                if write_half.write_all(&frame).await.is_err() { break; }
            }
            result = ev_rx.recv() => {
                match result {
                    Ok(ev) => {
                        let mut line = match serde_json::to_string(&ev) {
                            Ok(s) => s,
                            Err(_) => continue,
                        };
                        line.push('\n');
                        if write_half.write_all(line.as_bytes()).await.is_err() { break; }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            result = lines.next_line() => {
                match result {
                    Ok(Some(line)) if !line.trim().is_empty() => {
                        let cmd: ControlCommand = match serde_json::from_str(line.trim()) {
                            Ok(c) => c,
                            Err(e) => {
                                let resp = CommandResponse::err(format!("parse error: {e}"));
                                let _ = send_json(&mut write_half, &resp).await;
                                continue;
                            }
                        };
                        if handle_command(cmd, &mut write_half, &spec_frame_tx, &mut spectrum_task, &ctx).await {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Ok(Some(_)) => {}
                    Err(_) => break,
                }
            }
        }
    }

    if let Some(h) = spectrum_task {
        h.abort();
    }
}

/// Dispatch one command; returns `true` when the write failed and the loop should exit.
#[cfg(not(target_arch = "wasm32"))]
async fn handle_command(
    cmd: ControlCommand,
    write_half: &mut tokio::net::tcp::OwnedWriteHalf,
    spec_frame_tx: &mpsc::Sender<Vec<u8>>,
    spectrum_task: &mut Option<tokio::task::JoinHandle<()>>,
    ctx: &ClientCtx,
) -> bool {
    match &cmd {
        ControlCommand::SubscribeSpectrum { fps } => {
            let fps = (*fps).clamp(1, 100);
            if let Some(h) = spectrum_task.take() {
                h.abort();
            }
            let tap = Arc::clone(&ctx.spectrum_tap);
            let tx = spec_frame_tx.clone();
            let period = Duration::from_millis(1000 / fps as u64);
            *spectrum_task = Some(tokio::spawn(async move {
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
            send_json(write_half, &CommandResponse::ok()).await.is_err()
        }

        ControlCommand::GetConfig => {
            let (cs, gs) = ctx.station_id.lock().await.clone();
            // Hold both locks simultaneously so the snapshot is consistent with SetConfig.
            let mode_guard = ctx.active_mode.lock().await;
            let atten_guard = ctx.tx_attenuation_db.lock().await;
            let config = protocol::DaemonConfig {
                callsign: cs,
                grid_square: gs,
                mode: mode_guard.clone(),
                tx_attenuation_db: *atten_guard,
            };
            drop(mode_guard);
            drop(atten_guard);
            if send_json(write_half, &ControlEvent::ConfigData { config })
                .await
                .is_err()
            {
                return true;
            }
            send_json(write_half, &CommandResponse::ok()).await.is_err()
        }

        ControlCommand::ListMessages => {
            let messages: Vec<MessageSummary> = ctx
                .message_store
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
            if send_json(write_half, &ControlEvent::MessageList { messages })
                .await
                .is_err()
            {
                return true;
            }
            send_json(write_half, &CommandResponse::ok()).await.is_err()
        }

        ControlCommand::GetMessage { id } => {
            let found = ctx
                .message_store
                .lock()
                .await
                .messages
                .iter()
                .find(|m| m.id == *id)
                .cloned();
            match found {
                None => send_json(
                    write_half,
                    &CommandResponse::err(format!("unknown id {id}")),
                )
                .await
                .is_err(),
                Some(m) => {
                    let ev = ControlEvent::MessageData {
                        id: m.id,
                        from: m.from,
                        to: m.to,
                        subject: m.subject,
                        body: m.body,
                    };
                    if send_json(write_half, &ev).await.is_err() {
                        return true;
                    }
                    send_json(write_half, &CommandResponse::ok()).await.is_err()
                }
            }
        }

        ControlCommand::SendMessage { to, subject, body } => {
            let from = ctx.station_id.lock().await.0.clone();
            let timestamp_secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let id = {
                let mut store = ctx.message_store.lock().await;
                let id = store.alloc_id();
                store.messages.push(StoredMessage {
                    id,
                    from: from.clone(),
                    to: to.clone(),
                    subject: subject.clone(),
                    body: body.clone(),
                    timestamp_secs,
                });
                if store.messages.len() > MAX_MESSAGES {
                    store.messages.remove(0);
                }
                id
            };
            let preview: String = body.chars().take(120).collect();
            let ev = ControlEvent::MessageReceived {
                id,
                from,
                to: to.clone(),
                subject: subject.clone(),
                preview,
                timestamp_secs,
            };
            // Broadcast to all connected clients.
            let _ = ctx.ev_tx.send(ev);
            // Forward to daemon main for RF dispatch.
            let _ = ctx.cmd_tx.send(cmd.clone()).await;
            send_json(write_half, &CommandResponse::ok()).await.is_err()
        }

        ControlCommand::DeleteMessage { id } => {
            ctx.message_store
                .lock()
                .await
                .messages
                .retain(|m| m.id != *id);
            send_json(write_half, &CommandResponse::ok()).await.is_err()
        }

        _ => {
            let resp =
                dispatch_command(&cmd, &ctx.cmd_tx, &ctx.active_mode, &ctx.tx_attenuation_db).await;
            send_json(write_half, &resp).await.is_err()
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn send_json<T: serde::Serialize>(
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    value: &T,
) -> Result<(), std::io::Error> {
    let mut s = serde_json::to_string(value).unwrap_or_default();
    s.push('\n');
    writer.write_all(s.as_bytes()).await
}

/// Apply state-mutating commands and forward all commands to the caller.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn dispatch_command(
    cmd: &ControlCommand,
    cmd_tx: &mpsc::Sender<ControlCommand>,
    active_mode: &SharedMode,
    tx_attenuation_db: &SharedAttenuation,
) -> CommandResponse {
    if let ControlCommand::SetMode { ref mode } = cmd {
        *active_mode.lock().await = mode.clone();
    }
    if let ControlCommand::SetTxAttenuation { db, .. } = cmd {
        *tx_attenuation_db.lock().await = *db;
    }
    if let ControlCommand::SetConfig { ref config } = cmd {
        // Hold both locks simultaneously so GetConfig cannot observe a mixed state.
        let mut mode = active_mode.lock().await;
        let mut atten = tx_attenuation_db.lock().await;
        *mode = config.mode.clone();
        *atten = config.tx_attenuation_db;
    }

    if cmd_tx.send(cmd.clone()).await.is_err() {
        return CommandResponse::err("server shutting down");
    }

    CommandResponse::ok()
}
