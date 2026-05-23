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
use std::collections::HashMap;
#[cfg(not(target_arch = "wasm32"))]
use std::net::SocketAddr;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(not(target_arch = "wasm32"))]
use openpulse_channel::dsp::PowerSpectrum;
#[cfg(not(target_arch = "wasm32"))]
use openpulse_core::handshake::{InMemoryTrustStore, TrustStore};
#[cfg(not(target_arch = "wasm32"))]
use openpulse_core::relay::RelayForwarder;
use openpulse_core::trust::{CertificateSource, PublicKeyTrustLevel, SigningMode};
#[cfg(not(target_arch = "wasm32"))]
use openpulse_modem::engine::SecureSessionParams;
#[cfg(not(target_arch = "wasm32"))]
use openpulse_modem::ModemEngine;
#[cfg(not(target_arch = "wasm32"))]
use openpulse_qsy::frame::{
    decode_unsigned as decode_qsy_frame, encode_unsigned as encode_qsy_frame, QsyFrame,
};
#[cfg(not(target_arch = "wasm32"))]
use openpulse_qsy::session::{QsyAction, QsyPolicy, QsySession};
#[cfg(not(target_arch = "wasm32"))]
use openpulse_qsy::ConnectionTrustLevel;
#[cfg(not(target_arch = "wasm32"))]
use openpulse_radio::RigctldController;
#[cfg(not(target_arch = "wasm32"))]
use openpulse_repeater::CrossBandRepeater;
#[cfg(not(target_arch = "wasm32"))]
use protocol::{
    encode_spectrum_frame, CommandResponse, ControlCommand, ControlEvent, MessageSummary,
};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(not(target_arch = "wasm32"))]
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
#[cfg(not(target_arch = "wasm32"))]
use tokio::net::{TcpListener, TcpStream};
#[cfg(not(target_arch = "wasm32"))]
use tokio::sync::{broadcast, mpsc, Mutex, RwLock};

pub use protocol::ControlCommand as Command;
pub use protocol::ControlEvent as Event;

/// Live engine metrics shared between the main loop and the periodic metrics task.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
pub struct MetricsSnapshot {
    pub afc_correction_hz: f32,
    /// Cumulative bytes decoded from the RF receive path.
    pub total_rx_bytes: u64,
}

#[cfg(not(target_arch = "wasm32"))]
type SharedMetrics = Arc<Mutex<MetricsSnapshot>>;

/// Mutable daemon runtime state touched by side-effectful control commands.
#[cfg(not(target_arch = "wasm32"))]
pub struct RuntimeControlState {
    pub repeater_enabled: bool,
    pub qsy_decisions: HashMap<String, bool>,
    pub qsy_pending_token: Option<String>,
    /// Active QSY negotiation session (present after operator accepts a pending token).
    pub qsy_session: Option<QsySession>,
    /// Candidate frequencies (Hz) supplied from config for QSY scanning.
    pub qsy_candidate_freqs: Vec<u64>,
    /// QSY policy parsed from config; governs which requests are accepted.
    pub qsy_policy: QsyPolicy,
    /// Dwell time per frequency during a QSY scan (milliseconds).
    pub qsy_scan_dwell_ms: u64,
    /// Switchover offset (seconds) encoded in outgoing QSY_ACK frames.
    pub qsy_switchover_offset_s: u32,
    /// Pre-built cross-band repeater; taken and moved into a thread by EnableRepeater.
    pub repeater: Option<CrossBandRepeater>,
    /// Stop flag for the running repeater thread.
    pub repeater_stop: Option<Arc<AtomicBool>>,
    /// Handle for the running repeater thread.
    pub repeater_thread: Option<std::thread::JoinHandle<()>>,
    /// Timestamp of the most recent PttAssert; `None` when PTT is not active.
    pub ptt_asserted_at: Option<Instant>,
    /// Maximum continuous transmit time before the watchdog releases PTT.
    /// Defaults to 3 minutes (180 s) to stay within Part 97 duty-cycle guidance.
    pub ptt_max_duration: Duration,
    /// Loaded trust store for verifying incoming peer handshakes.
    pub trust_store: InMemoryTrustStore,
    /// Optional relay forwarder; `Some` when `[relay] enabled = true` in config.
    pub relay_forwarder: Option<RelayForwarder>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for RuntimeControlState {
    fn default() -> Self {
        Self {
            repeater_enabled: false,
            qsy_decisions: HashMap::new(),
            qsy_pending_token: None,
            qsy_session: None,
            qsy_candidate_freqs: Vec::new(),
            qsy_policy: QsyPolicy::default(),
            qsy_scan_dwell_ms: 500,
            qsy_switchover_offset_s: 5,
            repeater: None,
            repeater_stop: None,
            repeater_thread: None,
            ptt_asserted_at: None,
            ptt_max_duration: Duration::from_secs(180),
            trust_store: InMemoryTrustStore::default(),
            relay_forwarder: None,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl std::fmt::Debug for RuntimeControlState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeControlState")
            .field("repeater_enabled", &self.repeater_enabled)
            .field("qsy_decisions", &self.qsy_decisions)
            .field("qsy_pending_token", &self.qsy_pending_token)
            .field("qsy_session", &self.qsy_session.is_some())
            .field("qsy_candidate_freqs", &self.qsy_candidate_freqs)
            .field("qsy_switchover_offset_s", &self.qsy_switchover_offset_s)
            .field("repeater", &self.repeater.is_some())
            .field("repeater_stop", &self.repeater_stop.is_some())
            .field("repeater_thread", &self.repeater_thread.is_some())
            .field(
                "ptt_asserted_at",
                &self.ptt_asserted_at.map(|t| t.elapsed()),
            )
            .field("ptt_max_duration", &self.ptt_max_duration)
            .field("trust_store_entries", &"<opaque>")
            .field("relay_forwarder", &self.relay_forwarder.is_some())
            .finish()
    }
}

/// Shared mutable mode string, written by `set_mode` commands.
#[cfg(not(target_arch = "wasm32"))]
pub type SharedMode = Arc<Mutex<String>>;
/// Shared mutable TX attenuation (dB), written by `set_tx_attenuation` commands.
#[cfg(not(target_arch = "wasm32"))]
pub type SharedAttenuation = Arc<Mutex<f32>>;
/// Shared QSY enabled flag, toggled by `set_config` commands.
#[cfg(not(target_arch = "wasm32"))]
pub type SharedQsyEnabled = Arc<Mutex<bool>>;
/// Shared bandplan mode string (`"unrestricted"`, `"ham-iaru-r1"`, etc.).
#[cfg(not(target_arch = "wasm32"))]
pub type SharedBandplanMode = Arc<Mutex<String>>;
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
    pub messages: std::collections::VecDeque<StoredMessage>,
}

#[cfg(not(target_arch = "wasm32"))]
impl MessageStore {
    fn new() -> Self {
        Self {
            next_id: 1,
            messages: std::collections::VecDeque::new(),
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
    qsy_enabled: SharedQsyEnabled,
    bandplan_mode: SharedBandplanMode,
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
    /// Whether QSY frequency-agility is enabled.
    pub qsy_enabled: SharedQsyEnabled,
    /// Active bandplan guardrail mode string.
    pub bandplan_mode: SharedBandplanMode,
    /// Audio sample tap; caller may write recent RX samples here.
    pub spectrum_tap: SpectrumTap,
    /// Station callsign and grid square loaded from config at startup.
    pub station_id: SharedStationId,
    /// In-memory message store shared across all control endpoints.
    pub message_store: SharedMessageStore,
    /// Live engine metrics written by the main loop; read by the periodic metrics task.
    pub shared_metrics: SharedMetrics,
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
        initial_qsy_enabled: bool,
        initial_bandplan_mode: String,
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
        let qsy_enabled: SharedQsyEnabled = Arc::new(Mutex::new(initial_qsy_enabled));
        let bandplan_mode: SharedBandplanMode = Arc::new(Mutex::new(initial_bandplan_mode));
        let spectrum_tap: SpectrumTap = Arc::new(RwLock::new(vec![0.0f32; 1024]));
        let station_id: SharedStationId = Arc::new(Mutex::new(initial_station_id));
        let message_store: SharedMessageStore = Arc::new(Mutex::new(MessageStore::new()));
        let shared_metrics: SharedMetrics = Arc::new(Mutex::new(MetricsSnapshot::default()));

        // Background task: forward EngineEvents into the ControlEvent broadcast.
        let mut eng_rx = engine.subscribe();
        let ev_fwd = Arc::clone(&ev_tx);
        tokio::spawn(async move {
            loop {
                match eng_rx.recv().await {
                    Ok(ev) => {
                        let _ = ev_fwd.send(ControlEvent::EngineEvent { event: ev });
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(lost = n, "engine event receiver lagged; events dropped");
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        // Background task: periodic Metrics events at 1 Hz.
        let ev_metrics = Arc::clone(&ev_tx);
        let metrics_snap = Arc::clone(&shared_metrics);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            let mut last_bytes: u64 = 0;
            loop {
                interval.tick().await;
                let (afc, new_bytes) = {
                    let m = metrics_snap.lock().await;
                    (m.afc_correction_hz, m.total_rx_bytes)
                };
                let effective_bps = (new_bytes.saturating_sub(last_bytes) * 8) as f32;
                last_bytes = new_bytes;
                let _ = ev_metrics.send(ControlEvent::Metrics {
                    effective_bps,
                    ecc_rate: None,
                    compress_ratio: None,
                    afc_correction_hz: afc,
                    signal_strength_dbm: None,
                });
            }
        });

        // Acceptor task.
        let ev_tx_a = Arc::clone(&ev_tx);
        let cmd_tx_a = cmd_tx.clone();
        let mode_a = Arc::clone(&active_mode);
        let atten_a = Arc::clone(&tx_attenuation_db);
        let qsy_a = Arc::clone(&qsy_enabled);
        let bp_a = Arc::clone(&bandplan_mode);
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
                            qsy_enabled: Arc::clone(&qsy_a),
                            bandplan_mode: Arc::clone(&bp_a),
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
            qsy_enabled,
            bandplan_mode,
            spectrum_tap,
            station_id,
            message_store,
            shared_metrics,
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
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(lost = n, "TCP client event receiver lagged; events dropped");
                        continue;
                    }
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
                    let bins = ps.compute(&tap.read().await);
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
            // Hold all four locks simultaneously so the snapshot is consistent with SetConfig.
            let mode_guard = ctx.active_mode.lock().await;
            let atten_guard = ctx.tx_attenuation_db.lock().await;
            let qsy_guard = ctx.qsy_enabled.lock().await;
            let bp_guard = ctx.bandplan_mode.lock().await;
            let config = protocol::DaemonConfig {
                callsign: cs,
                grid_square: gs,
                mode: mode_guard.clone(),
                tx_attenuation_db: *atten_guard,
                qsy_enabled: *qsy_guard,
                bandplan_mode: bp_guard.clone(),
            };
            drop(mode_guard);
            drop(atten_guard);
            drop(qsy_guard);
            drop(bp_guard);
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
                store.messages.push_back(StoredMessage {
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
            let resp = dispatch_command(
                &cmd,
                &ctx.cmd_tx,
                &ctx.active_mode,
                &ctx.tx_attenuation_db,
                &ctx.qsy_enabled,
                &ctx.bandplan_mode,
            )
            .await;
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
    qsy_enabled: &SharedQsyEnabled,
    bandplan_mode: &SharedBandplanMode,
) -> CommandResponse {
    if let ControlCommand::SetMode { ref mode } = cmd {
        *active_mode.lock().await = mode.clone();
    }
    if let ControlCommand::SetTxAttenuation { db, .. } = cmd {
        *tx_attenuation_db.lock().await = *db;
    }
    if let ControlCommand::SetConfig { ref config } = cmd {
        // Hold all four locks simultaneously so GetConfig cannot observe a mixed state.
        let (new_qsy, new_bp) = {
            let mut mode = active_mode.lock().await;
            let mut atten = tx_attenuation_db.lock().await;
            let mut qsy = qsy_enabled.lock().await;
            let mut bp = bandplan_mode.lock().await;
            *mode = config.mode.clone();
            *atten = config.tx_attenuation_db;
            *qsy = config.qsy_enabled;
            *bp = config.bandplan_mode.clone();
            (*qsy, bp.clone())
        };
        // Persist QSY settings so they survive a daemon restart.
        if let Err(e) = openpulse_config::save_qsy_config(new_qsy, &new_bp) {
            tracing::warn!("could not persist QSY config: {e}");
        }
    }

    if cmd_tx.send(cmd.clone()).await.is_err() {
        return CommandResponse::err("server shutting down");
    }

    CommandResponse::ok()
}

/// Dispatch a list of [`QsyAction`]s produced by a [`QsySession`].
///
/// Used by both the initiator (`accept_qsy`) and responder (`process_received_bytes`) paths.
#[cfg(not(target_arch = "wasm32"))]
async fn execute_qsy_actions(
    actions: Vec<QsyAction>,
    session: &mut QsySession,
    engine: &mut ModemEngine,
    mut rig_controller: Option<&mut RigctldController>,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
    mode: &str,
    scan_dwell_ms: u64,
) {
    let mut scan_freqs: Option<Vec<u64>> = None;

    for action in actions {
        match action {
            QsyAction::SendFrame(ref frame) => {
                let line = encode_qsy_frame(frame);
                if let Err(e) = engine.transmit(line.as_bytes(), mode, None) {
                    tracing::warn!(error = %e, "qsy: frame transmit failed");
                }
            }
            QsyAction::StartScan { candidates } => {
                scan_freqs = Some(candidates);
            }
            QsyAction::QsyNow { freq_hz } => {
                if let Some(ref mut rig) = rig_controller {
                    if let Err(e) = rig.set_frequency(freq_hz) {
                        tracing::warn!(freq_hz, error = %e, "qsy: set_frequency failed");
                    }
                }
            }
            QsyAction::Reject { reason } => {
                let _ = event_tx.send(ControlEvent::CommandError {
                    command: "qsy".to_string(),
                    reason: format!("QSY rejected: {reason}"),
                });
            }
        }
    }

    if let Some(freqs) = scan_freqs {
        let results: Vec<(u64, f32)> = if let Some(ref mut rig) = rig_controller {
            // Hop to each candidate, dwell briefly, and read the measured SNR.
            // Save and restore the original frequency so the radio is left on-channel.
            // Rig calls are synchronous TCP I/O — run them in block_in_place so they
            // don't stall the Tokio runtime during the scan.
            let original_freq = match tokio::task::block_in_place(|| rig.get_frequency()) {
                Ok(f) => Some(f),
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "qsy scan: failed to read current frequency; will not restore after scan"
                    );
                    None
                }
            };
            let mut scan_results = Vec::with_capacity(freqs.len());
            for &freq in &freqs {
                if let Err(e) = tokio::task::block_in_place(|| rig.set_frequency(freq)) {
                    tracing::warn!(freq, error = %e, "qsy scan: set_frequency failed; using last SNR");
                    scan_results.push((freq, engine.last_rx_snr_db().unwrap_or(0.0)));
                    continue;
                }
                // Dwell per config to let the audio buffer refresh before sampling SNR.
                tokio::time::sleep(Duration::from_millis(scan_dwell_ms)).await;
                match tokio::task::block_in_place(|| engine.receive(mode, None)) {
                    Ok(_) => {}
                    Err(e) => tracing::warn!(freq, error = %e, "qsy scan: receive failed"),
                }
                scan_results.push((freq, engine.last_rx_snr_db().unwrap_or(0.0)));
            }
            if let Some(orig) = original_freq {
                if let Err(e) = tokio::task::block_in_place(|| rig.set_frequency(orig)) {
                    tracing::warn!(freq = orig, error = %e, "qsy scan: failed to restore frequency");
                }
            }
            scan_results
        } else {
            // No rig controller: fall back to uniform SNR from the most recent receive.
            let observed_snr = engine.last_rx_snr_db().unwrap_or(0.0);
            freqs.iter().map(|&f| (f, observed_snr)).collect()
        };
        match session.scan_complete(results) {
            Ok(follow_up) => {
                // scan_complete never returns another StartScan; iterate directly.
                for action in follow_up {
                    match action {
                        QsyAction::SendFrame(ref frame) => {
                            let line = encode_qsy_frame(frame);
                            if let Err(e) = engine.transmit(line.as_bytes(), mode, None) {
                                tracing::warn!(error = %e, "qsy: post-scan frame transmit failed");
                            }
                        }
                        QsyAction::QsyNow { freq_hz } => {
                            if let Some(ref mut rig) = rig_controller {
                                if let Err(e) = rig.set_frequency(freq_hz) {
                                    tracing::warn!(freq_hz, error = %e, "qsy: post-scan set_frequency failed");
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => tracing::warn!(error = %e, "qsy: scan_complete failed"),
        }
    }
}

/// Release PTT if the watchdog deadline has elapsed since `PttAssert`.
///
/// Returns `true` if the watchdog fired (PTT was forcibly released), so the
/// caller can propagate the hardware release through the PTT controller.
#[cfg(not(target_arch = "wasm32"))]
pub fn check_ptt_watchdog(
    runtime_state: &mut RuntimeControlState,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
) -> bool {
    if let Some(asserted_at) = runtime_state.ptt_asserted_at {
        if asserted_at.elapsed() >= runtime_state.ptt_max_duration {
            runtime_state.ptt_asserted_at = None;
            tracing::warn!(
                max_secs = runtime_state.ptt_max_duration.as_secs(),
                "PTT watchdog fired — transmitter has been keyed beyond max duration; releasing"
            );
            let _ = event_tx.send(ControlEvent::PttChanged { active: false });
            return true;
        }
    }
    false
}

/// Process raw bytes received from the modem engine and drive QSY responder logic.
///
/// Called from the main daemon loop after each receive tick. Non-QSY payloads are
/// silently discarded; only valid [`QsyFrame`] lines advance the session.
#[cfg(not(target_arch = "wasm32"))]
pub async fn process_received_bytes(
    bytes: &[u8],
    runtime_state: &mut RuntimeControlState,
    rig_controller: Option<&mut RigctldController>,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
    active_mode: &SharedMode,
    engine: &mut ModemEngine,
) {
    if bytes.is_empty() {
        return;
    }
    let mode = active_mode.lock().await.clone();
    // Attempt relay forwarding on the raw bytes before QSY parsing: WireEnvelope frames
    // are binary and would be dropped by the UTF-8 early return below.
    maybe_relay_forward(bytes, &mode, runtime_state, engine);

    let Ok(text) = std::str::from_utf8(bytes) else {
        return;
    };
    let Ok(frame) = decode_qsy_frame(text.trim()) else {
        return;
    };

    let is_new_session = runtime_state.qsy_session.is_none();
    let session = runtime_state.qsy_session.get_or_insert_with(|| {
        QsySession::new_responder(
            runtime_state.qsy_policy.clone(),
            ConnectionTrustLevel::Unverified,
        )
    });

    // Notify connected clients that a remote station initiated QSY.
    if is_new_session {
        if let QsyFrame::Req {
            ref token,
            n_candidates,
        } = frame
        {
            let _ = event_tx.send(ControlEvent::QsyIncoming {
                token: token.clone(),
                n_candidates,
            });
        }
    }

    match session.apply(frame) {
        Ok(actions) => {
            execute_qsy_actions(
                actions,
                session,
                engine,
                rig_controller,
                event_tx,
                &mode,
                runtime_state.qsy_scan_dwell_ms,
            )
            .await;
        }
        Err(e) => tracing::warn!(error = %e, "qsy responder: apply frame failed"),
    }
}

fn maybe_relay_forward(
    payload: &[u8],
    mode: &str,
    runtime_state: &mut RuntimeControlState,
    engine: &mut ModemEngine,
) {
    use openpulse_core::wire_query::WireEnvelope;

    let Some(ref mut fwd) = runtime_state.relay_forwarder else {
        return;
    };
    let Ok(envelope) = WireEnvelope::decode(payload) else {
        return;
    };
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let forwarded = fwd.forward(&envelope, now_ms);
    match forwarded {
        Ok(out_envelope) => match out_envelope.encode() {
            Ok(out_bytes) => {
                tracing::info!(
                    session_id = ?out_envelope.session_id,
                    hop_index = out_envelope.hop_index,
                    bytes = out_bytes.len(),
                    "relay: forwarding envelope"
                );
                if let Err(e) = engine.transmit(&out_bytes, mode, None) {
                    tracing::warn!(error = %e, "relay: retransmit failed");
                }
            }
            Err(e) => tracing::warn!(error = %e, "relay: envelope encode failed"),
        },
        Err(e) => tracing::info!(reason = ?e, "relay: dropping envelope"),
    }
}
///
/// This complements [`dispatch_command`], which updates shared daemon state and
/// forwards commands to the caller. Commands without runtime support emit a
/// [`ControlEvent::CommandError`] instead of failing silently.
#[cfg(not(target_arch = "wasm32"))]
pub async fn apply_command_to_engine(
    cmd: &ControlCommand,
    engine: &mut ModemEngine,
    active_mode: &SharedMode,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
    rig_controller: Option<&mut RigctldController>,
    runtime_state: &mut RuntimeControlState,
) {
    match cmd {
        ControlCommand::SetMode { mode } => {
            if engine.plugins().get(mode).is_some() {
                *active_mode.lock().await = mode.clone();
            } else {
                let _ = event_tx.send(ControlEvent::CommandError {
                    command: "set_mode".to_string(),
                    reason: format!("unsupported mode '{mode}'"),
                });
            }
        }
        ControlCommand::SetTxAttenuation { db, .. } => {
            engine.set_tx_attenuation_db(*db);
        }
        ControlCommand::SetConfig { config } => {
            if engine.plugins().get(&config.mode).is_some() {
                *active_mode.lock().await = config.mode.clone();
            } else {
                let _ = event_tx.send(ControlEvent::CommandError {
                    command: "set_config".to_string(),
                    reason: format!("unsupported mode '{}'", config.mode),
                });
            }
            engine.set_tx_attenuation_db(config.tx_attenuation_db);
        }
        ControlCommand::PttAssert => {
            runtime_state.ptt_asserted_at = Some(Instant::now());
            let _ = event_tx.send(ControlEvent::PttChanged { active: true });
        }
        ControlCommand::PttRelease => {
            runtime_state.ptt_asserted_at = None;
            let _ = event_tx.send(ControlEvent::PttChanged { active: false });
        }
        ControlCommand::ConnectPeer { callsign } => {
            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            // Use the configured trust level when the peer is in the trust store.
            // Unknown peers (not yet in store) get Full so the session proceeds and
            // trust is established at handshake time.  Revoked peers are rejected.
            let stored_trust = runtime_state.trust_store.trust_level(callsign);
            let key_trust = if stored_trust == PublicKeyTrustLevel::Unknown {
                PublicKeyTrustLevel::Full
            } else {
                stored_trust
            };
            let params = SecureSessionParams {
                local_minimum_mode: SigningMode::Normal,
                peer_supported_modes: vec![SigningMode::Normal, SigningMode::Psk],
                key_trust,
                certificate_source: CertificateSource::OutOfBand,
                psk_validated: false,
            };

            match engine.begin_secure_session(params, now_ms) {
                Ok(_) => {
                    let _ = event_tx.send(ControlEvent::RfConnectionChanged {
                        connected: true,
                        peer: Some(callsign.clone()),
                    });

                    let token = format!("qsy-{now_ms}");
                    runtime_state.qsy_pending_token = Some(token.clone());
                    let _ = event_tx.send(ControlEvent::QsyPending { token });
                }
                Err(err) => {
                    let _ = event_tx.send(ControlEvent::CommandError {
                        command: "connect_peer".to_string(),
                        reason: format!("secure session start failed: {err}"),
                    });
                }
            }
        }
        ControlCommand::DisconnectPeer => {
            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            match engine.end_secure_session(now_ms) {
                Ok(()) => {
                    runtime_state.qsy_pending_token = None;
                    let _ = event_tx.send(ControlEvent::RfConnectionChanged {
                        connected: false,
                        peer: None,
                    });
                }
                Err(err) => {
                    let _ = event_tx.send(ControlEvent::CommandError {
                        command: "disconnect_peer".to_string(),
                        reason: format!("secure session end failed: {err}"),
                    });
                }
            }
        }
        ControlCommand::SendMessage { body, .. } => {
            let mode = active_mode.lock().await.clone();
            if let Err(err) = engine.transmit(body.as_bytes(), &mode, None) {
                tracing::warn!(mode = %mode, error = %err, "daemon rf dispatch failed for SendMessage");
                let _ = event_tx.send(ControlEvent::CommandError {
                    command: "send_message".to_string(),
                    reason: format!("rf dispatch failed in mode '{mode}': {err}"),
                });
            }
        }
        ControlCommand::SetFreq { rig, freq_hz } => {
            if rig != "rigctld" {
                let _ = event_tx.send(ControlEvent::CommandError {
                    command: "set_freq".to_string(),
                    reason: format!("unsupported rig target '{rig}'"),
                });
                return;
            }

            let Some(controller) = rig_controller else {
                let _ = event_tx.send(ControlEvent::CommandError {
                    command: "set_freq".to_string(),
                    reason: "no rigctld controller configured".to_string(),
                });
                return;
            };

            match controller.set_frequency(*freq_hz) {
                Ok(()) => {
                    let _ = event_tx.send(ControlEvent::RigStatus {
                        rig: rig.clone(),
                        freq_hz: *freq_hz,
                        mode: "CAT".to_string(),
                        power_w: None,
                        alc: None,
                        swr: None,
                    });
                }
                Err(err) => {
                    let _ = event_tx.send(ControlEvent::CommandError {
                        command: "set_freq".to_string(),
                        reason: format!("rigctld set_frequency failed: {err}"),
                    });
                }
            }
        }
        ControlCommand::AcceptQsy { token } => {
            let token = token.trim();
            if token.is_empty() {
                let _ = event_tx.send(ControlEvent::CommandError {
                    command: "accept_qsy".to_string(),
                    reason: "empty token".to_string(),
                });
                return;
            }

            if runtime_state.qsy_pending_token.as_deref() != Some(token) {
                let _ = event_tx.send(ControlEvent::CommandError {
                    command: "accept_qsy".to_string(),
                    reason: format!("unknown pending token '{token}'"),
                });
                return;
            }

            match runtime_state.qsy_decisions.get(token) {
                Some(true) => {
                    let _ = event_tx.send(ControlEvent::CommandError {
                        command: "accept_qsy".to_string(),
                        reason: format!("token '{token}' already accepted"),
                    });
                }
                Some(false) => {
                    let _ = event_tx.send(ControlEvent::CommandError {
                        command: "accept_qsy".to_string(),
                        reason: format!("token '{token}' already rejected"),
                    });
                }
                None => {
                    runtime_state.qsy_decisions.insert(token.to_string(), true);
                    runtime_state.qsy_pending_token = None;
                    let _ = event_tx.send(ControlEvent::QsyDecision {
                        token: token.to_string(),
                        accepted: true,
                    });

                    let candidates = runtime_state.qsy_candidate_freqs.clone();
                    if candidates.is_empty() {
                        let _ = event_tx.send(ControlEvent::CommandError {
                            command: "accept_qsy".to_string(),
                            reason: "no candidate frequencies configured (qsy.candidate_freqs_hz)"
                                .to_string(),
                        });
                        return;
                    }

                    let mut session = QsySession::new_initiator()
                        .with_switchover_offset_s(runtime_state.qsy_switchover_offset_s);
                    let actions = match session.initiate(candidates) {
                        Ok(a) => a,
                        Err(e) => {
                            let _ = event_tx.send(ControlEvent::CommandError {
                                command: "accept_qsy".to_string(),
                                reason: format!("QSY session initiate failed: {e}"),
                            });
                            return;
                        }
                    };

                    let mode = active_mode.lock().await.clone();
                    execute_qsy_actions(
                        actions,
                        &mut session,
                        engine,
                        rig_controller,
                        event_tx,
                        &mode,
                        runtime_state.qsy_scan_dwell_ms,
                    )
                    .await;

                    runtime_state.qsy_session = Some(session);
                }
            }
        }
        ControlCommand::RejectQsy { token } => {
            let token = token.trim();
            if token.is_empty() {
                let _ = event_tx.send(ControlEvent::CommandError {
                    command: "reject_qsy".to_string(),
                    reason: "empty token".to_string(),
                });
                return;
            }

            if runtime_state.qsy_pending_token.as_deref() != Some(token) {
                let _ = event_tx.send(ControlEvent::CommandError {
                    command: "reject_qsy".to_string(),
                    reason: format!("unknown pending token '{token}'"),
                });
                return;
            }

            match runtime_state.qsy_decisions.get(token) {
                Some(true) => {
                    let _ = event_tx.send(ControlEvent::CommandError {
                        command: "reject_qsy".to_string(),
                        reason: format!("token '{token}' already accepted"),
                    });
                }
                Some(false) => {
                    let _ = event_tx.send(ControlEvent::CommandError {
                        command: "reject_qsy".to_string(),
                        reason: format!("token '{token}' already rejected"),
                    });
                }
                None => {
                    runtime_state.qsy_decisions.insert(token.to_string(), false);
                    runtime_state.qsy_pending_token = None;
                    let _ = event_tx.send(ControlEvent::QsyDecision {
                        token: token.to_string(),
                        accepted: false,
                    });
                }
            }
        }
        ControlCommand::EnableRepeater => {
            if runtime_state.repeater_enabled {
                let _ = event_tx.send(ControlEvent::CommandError {
                    command: "enable_repeater".to_string(),
                    reason: "repeater already enabled".to_string(),
                });
                return;
            }

            if let Some(mut repeater) = runtime_state.repeater.take() {
                let stop = Arc::new(AtomicBool::new(false));
                let stop_clone = Arc::clone(&stop);
                let thread = std::thread::spawn(move || {
                    if let Err(e) = repeater.run_full_duplex(stop_clone) {
                        tracing::warn!(error = %e, "cross-band repeater exited with error");
                    }
                });
                runtime_state.repeater_stop = Some(stop);
                runtime_state.repeater_thread = Some(thread);
            } else {
                tracing::warn!("enable_repeater: no pre-built repeater in runtime state; audio routing not started");
            }

            runtime_state.repeater_enabled = true;
            let _ = event_tx.send(ControlEvent::RepeaterChanged { enabled: true });
        }
        ControlCommand::DisableRepeater => {
            if !runtime_state.repeater_enabled {
                let _ = event_tx.send(ControlEvent::CommandError {
                    command: "disable_repeater".to_string(),
                    reason: "repeater already disabled".to_string(),
                });
                return;
            }

            if let Some(stop) = runtime_state.repeater_stop.take() {
                stop.store(true, Ordering::Relaxed);
            }
            if let Some(thread) = runtime_state.repeater_thread.take() {
                let _ = thread.join();
            }

            runtime_state.repeater_enabled = false;
            let _ = event_tx.send(ControlEvent::RepeaterChanged { enabled: false });
        }
        // No live-modem side effects for these commands in the engine path.
        // They are handled by dispatch-only paths or request-response control flow.
        ControlCommand::SubscribeSpectrum { .. }
        | ControlCommand::GetConfig
        | ControlCommand::ListMessages
        | ControlCommand::GetMessage { .. }
        | ControlCommand::DeleteMessage { .. } => {}
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod command_apply_tests {
    use super::*;
    use bpsk_plugin::BpskPlugin;
    use openpulse_audio::LoopbackBackend;

    fn test_engine() -> ModemEngine {
        let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
        engine.register_plugin(Box::new(BpskPlugin::new())).unwrap();
        engine
    }

    #[tokio::test]
    async fn apply_set_config_updates_mode_and_tx_attenuation() {
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, _) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);

        let cmd = ControlCommand::SetConfig {
            config: protocol::DaemonConfig {
                callsign: "N0CALL".into(),
                grid_square: "AA00".into(),
                mode: "BPSK250".into(),
                tx_attenuation_db: -6.0,
                qsy_enabled: false,
                bandplan_mode: "unrestricted".into(),
            },
        };

        let mut runtime_state = RuntimeControlState::default();
        apply_command_to_engine(
            &cmd,
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut runtime_state,
        )
        .await;

        assert_eq!(*active_mode.lock().await, "BPSK250");
        assert!((engine.tx_attenuation_db() - (-6.0)).abs() < 1e-6);
    }

    #[tokio::test]
    async fn apply_send_message_transmits_payload_over_active_mode() {
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, _) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);

        let cmd = ControlCommand::SendMessage {
            to: "W1AW".into(),
            subject: "status".into(),
            body: "rf body payload".into(),
        };

        let mut runtime_state = RuntimeControlState::default();
        apply_command_to_engine(
            &cmd,
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut runtime_state,
        )
        .await;

        let rx = engine.receive("BPSK250", None).unwrap();
        assert_eq!(rx, b"rf body payload");
    }

    #[tokio::test]
    async fn apply_send_message_invalid_mode_emits_command_error() {
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("NO_SUCH_MODE".to_string()));
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);

        let cmd = ControlCommand::SendMessage {
            to: "W1AW".into(),
            subject: "status".into(),
            body: "rf body payload".into(),
        };

        let mut runtime_state = RuntimeControlState::default();
        apply_command_to_engine(
            &cmd,
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut runtime_state,
        )
        .await;

        let mut saw_error = false;
        while let Ok(ev) = rx.try_recv() {
            if let ControlEvent::CommandError { command, reason } = ev {
                assert_eq!(command, "send_message");
                assert!(reason.contains("NO_SUCH_MODE"));
                saw_error = true;
                break;
            }
        }
        assert!(saw_error, "expected command_error event");
    }

    #[tokio::test]
    async fn apply_unimplemented_runtime_commands_emit_command_error() {
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);

        let cases = vec![
            (
                ControlCommand::SetFreq {
                    rig: "rigctld".into(),
                    freq_hz: 7_100_000,
                },
                "set_freq",
                "no rigctld controller configured",
            ),
            (
                ControlCommand::AcceptQsy {
                    token: "tok-1".into(),
                },
                "accept_qsy",
                "unknown pending token",
            ),
            (
                ControlCommand::RejectQsy {
                    token: "tok-1".into(),
                },
                "reject_qsy",
                "unknown pending token",
            ),
        ];

        let mut runtime_state = RuntimeControlState::default();
        for (cmd, expected_command, expected_reason_substr) in cases {
            apply_command_to_engine(
                &cmd,
                &mut engine,
                &active_mode,
                &ev_tx,
                None,
                &mut runtime_state,
            )
            .await;
            let ev = rx.recv().await.expect("expected command_error event");
            match ev {
                ControlEvent::CommandError { command, reason } => {
                    assert_eq!(command, expected_command);
                    assert!(reason.contains(expected_reason_substr));
                }
                other => panic!("expected command_error event, got {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn apply_repeater_enable_disable_emits_state_changes() {
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);
        let mut runtime_state = RuntimeControlState::default();

        apply_command_to_engine(
            &ControlCommand::EnableRepeater,
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut runtime_state,
        )
        .await;
        assert!(runtime_state.repeater_enabled);
        match rx.recv().await.expect("expected repeater event") {
            ControlEvent::RepeaterChanged { enabled } => assert!(enabled),
            other => panic!("expected RepeaterChanged, got {other:?}"),
        }

        apply_command_to_engine(
            &ControlCommand::DisableRepeater,
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut runtime_state,
        )
        .await;
        assert!(!runtime_state.repeater_enabled);
        match rx.recv().await.expect("expected repeater event") {
            ControlEvent::RepeaterChanged { enabled } => assert!(!enabled),
            other => panic!("expected RepeaterChanged, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn apply_qsy_accept_reject_record_and_emit_decisions() {
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);
        let mut runtime_state = RuntimeControlState::default();
        runtime_state.qsy_pending_token = Some("tok-accept".to_string());
        runtime_state.qsy_candidate_freqs = vec![14_070_000, 14_077_000];

        apply_command_to_engine(
            &ControlCommand::AcceptQsy {
                token: "tok-accept".into(),
            },
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut runtime_state,
        )
        .await;
        assert_eq!(runtime_state.qsy_decisions.get("tok-accept"), Some(&true));
        assert!(runtime_state.qsy_pending_token.is_none());
        match rx.recv().await.expect("expected qsy event") {
            ControlEvent::QsyDecision { token, accepted } => {
                assert_eq!(token, "tok-accept");
                assert!(accepted);
            }
            other => panic!("expected QsyDecision, got {other:?}"),
        }

        runtime_state.qsy_pending_token = Some("tok-reject".to_string());
        apply_command_to_engine(
            &ControlCommand::RejectQsy {
                token: "tok-reject".into(),
            },
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut runtime_state,
        )
        .await;
        assert_eq!(runtime_state.qsy_decisions.get("tok-reject"), Some(&false));
        assert!(runtime_state.qsy_pending_token.is_none());
        match rx.recv().await.expect("expected qsy event") {
            ControlEvent::QsyDecision { token, accepted } => {
                assert_eq!(token, "tok-reject");
                assert!(!accepted);
            }
            other => panic!("expected QsyDecision, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn apply_connect_disconnect_drive_secure_session_and_pending_qsy() {
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(32);
        let ev_tx = Arc::new(tx);
        let mut runtime_state = RuntimeControlState::default();

        apply_command_to_engine(
            &ControlCommand::ConnectPeer {
                callsign: "W1AW".to_string(),
            },
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut runtime_state,
        )
        .await;

        assert!(engine.hpx_session_id().is_some());
        let token = runtime_state
            .qsy_pending_token
            .clone()
            .expect("connect should create pending qsy token");

        let first = rx.recv().await.expect("expected event");
        let second = rx.recv().await.expect("expected event");
        let saw_connected = matches!(
            (&first, &second),
            (
                ControlEvent::RfConnectionChanged {
                    connected: true,
                    peer: Some(_)
                },
                ControlEvent::QsyPending { .. }
            ) | (
                ControlEvent::QsyPending { .. },
                ControlEvent::RfConnectionChanged {
                    connected: true,
                    peer: Some(_)
                }
            )
        );
        assert!(
            saw_connected,
            "expected rf connected and qsy pending events"
        );

        apply_command_to_engine(
            &ControlCommand::AcceptQsy { token },
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut runtime_state,
        )
        .await;

        apply_command_to_engine(
            &ControlCommand::DisconnectPeer,
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut runtime_state,
        )
        .await;

        assert!(runtime_state.qsy_pending_token.is_none());
        assert!(engine.hpx_session_id().is_none());
    }

    #[tokio::test]
    async fn accept_qsy_with_candidates_initiates_session_and_transmits_req() {
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);
        let mut runtime_state = RuntimeControlState::default();
        runtime_state.qsy_pending_token = Some("tok-qsy".to_string());
        runtime_state.qsy_candidate_freqs = vec![14_070_000, 14_077_000];

        apply_command_to_engine(
            &ControlCommand::AcceptQsy {
                token: "tok-qsy".into(),
            },
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut runtime_state,
        )
        .await;

        assert_eq!(runtime_state.qsy_decisions.get("tok-qsy"), Some(&true));
        assert!(runtime_state.qsy_pending_token.is_none());
        assert!(
            runtime_state.qsy_session.is_some(),
            "QsySession should be stored in runtime_state"
        );

        // QsyDecision event must be first
        match rx.recv().await.expect("expected QsyDecision event") {
            ControlEvent::QsyDecision { token, accepted } => {
                assert_eq!(token, "tok-qsy");
                assert!(accepted);
            }
            other => panic!("expected QsyDecision, got {other:?}"),
        }

        // QSY_REQ and QSY_LIST frames were transmitted; verify loopback receive contains QSY text
        let bytes = engine.receive("BPSK250", None).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            text.contains("QSY_REQ") || text.contains("QSY_LIST"),
            "expected QSY frame in modem output, got: {text:?}"
        );
    }

    #[tokio::test]
    async fn accept_qsy_without_candidates_emits_command_error() {
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);
        let mut runtime_state = RuntimeControlState::default();
        runtime_state.qsy_pending_token = Some("tok-nocand".to_string());
        // qsy_candidate_freqs is empty (default)

        apply_command_to_engine(
            &ControlCommand::AcceptQsy {
                token: "tok-nocand".into(),
            },
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut runtime_state,
        )
        .await;

        // QsyDecision event first, then CommandError for no candidates
        let ev1 = rx.recv().await.expect("expected first event");
        assert!(
            matches!(ev1, ControlEvent::QsyDecision { .. }),
            "expected QsyDecision"
        );
        let ev2 = rx.recv().await.expect("expected CommandError event");
        match ev2 {
            ControlEvent::CommandError { command, reason } => {
                assert_eq!(command, "accept_qsy");
                assert!(reason.contains("candidate"), "reason: {reason}");
            }
            other => panic!("expected CommandError, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn repeater_enable_with_prebuilt_spawns_thread_and_disable_joins_it() {
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);
        let mut runtime_state = RuntimeControlState::default();

        // Inject a pre-built repeater using LoopbackBackend engines.
        let rep_rx = ModemEngine::new(Box::new(LoopbackBackend::new()));
        let rep_tx = ModemEngine::new(Box::new(LoopbackBackend::new()));
        let rep_cfg = openpulse_repeater::RepeaterConfig {
            enabled: true,
            mode: "BPSK250".to_string(),
            tx_hang_ms: 0,
            full_duplex: false,
        };
        runtime_state.repeater = Some(openpulse_repeater::CrossBandRepeater::new(
            Box::new(openpulse_radio::NoOpPtt::new()),
            rep_rx,
            rep_tx,
            rep_cfg,
        ));

        apply_command_to_engine(
            &ControlCommand::EnableRepeater,
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut runtime_state,
        )
        .await;

        assert!(runtime_state.repeater_enabled);
        assert!(
            runtime_state.repeater_thread.is_some(),
            "repeater thread should be spawned"
        );
        match rx.recv().await.expect("expected RepeaterChanged event") {
            ControlEvent::RepeaterChanged { enabled } => assert!(enabled),
            other => panic!("expected RepeaterChanged, got {other:?}"),
        }

        // Disable the repeater — sets stop flag, joins the thread.
        apply_command_to_engine(
            &ControlCommand::DisableRepeater,
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut runtime_state,
        )
        .await;

        assert!(!runtime_state.repeater_enabled);
        assert!(
            runtime_state.repeater_thread.is_none(),
            "thread handle should be cleared after join"
        );
        match rx.recv().await.expect("expected RepeaterChanged event") {
            ControlEvent::RepeaterChanged { enabled } => assert!(!enabled),
            other => panic!("expected RepeaterChanged, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn process_received_bytes_with_qsy_req_creates_responder_session() {
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, _rx) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);
        let mut runtime_state = RuntimeControlState::default();

        // QSY_REQ frame: verb, token, n_candidates
        let qsy_req = b"QSY_REQ tok-resp 2";
        process_received_bytes(
            qsy_req,
            &mut runtime_state,
            None,
            &ev_tx,
            &active_mode,
            &mut engine,
        )
        .await;

        assert!(
            runtime_state.qsy_session.is_some(),
            "responder session should be created on first QSY_REQ"
        );
    }

    #[tokio::test]
    async fn process_received_bytes_ignores_non_qsy_text() {
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, _rx) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);
        let mut runtime_state = RuntimeControlState::default();

        process_received_bytes(
            b"hello world",
            &mut runtime_state,
            None,
            &ev_tx,
            &active_mode,
            &mut engine,
        )
        .await;

        assert!(
            runtime_state.qsy_session.is_none(),
            "no session for non-QSY text"
        );
    }

    #[tokio::test]
    async fn process_received_bytes_ignores_non_utf8() {
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, _rx) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);
        let mut runtime_state = RuntimeControlState::default();

        process_received_bytes(
            &[0xff, 0xfe, 0x00],
            &mut runtime_state,
            None,
            &ev_tx,
            &active_mode,
            &mut engine,
        )
        .await;

        assert!(
            runtime_state.qsy_session.is_none(),
            "no session for non-UTF-8 bytes"
        );
    }

    /// End-to-end test: an initiator session generates a QSY_REQ frame; that
    /// frame is fed to the responder path, which must create a session and emit
    /// `QsyIncoming`.
    #[tokio::test]
    async fn qsy_initiator_req_drives_responder_session_and_emits_event() {
        // ── Initiator: build the QSY_REQ frame ─────────────────────────────
        let candidates: Vec<u64> = vec![14074000, 14070000, 7074000];
        let mut init_session = QsySession::new_initiator();
        let actions = init_session
            .initiate(candidates)
            .expect("initiator initiate should succeed");

        // The first action must be SendFrame(Req).
        let req_text = actions
            .iter()
            .find_map(|a| {
                if let QsyAction::SendFrame(frame @ QsyFrame::Req { .. }) = a {
                    Some(encode_qsy_frame(frame))
                } else {
                    None
                }
            })
            .expect("initiator must produce SendFrame(Req) action");

        // Extract the token from the encoded line for assertion.
        let token_from_line = req_text.split_whitespace().nth(1).unwrap().to_string();

        // ── Responder: feed the frame ───────────────────────────────────────
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(32);
        let ev_tx = Arc::new(tx);
        let mut runtime_state = RuntimeControlState::default();

        process_received_bytes(
            req_text.as_bytes(),
            &mut runtime_state,
            None,
            &ev_tx,
            &active_mode,
            &mut engine,
        )
        .await;

        // Responder must have a session.
        assert!(
            runtime_state.qsy_session.is_some(),
            "responder session should be created on receiving QSY_REQ"
        );

        // A QsyIncoming event must have been broadcast with matching token.
        let ev = rx.recv().await.expect("expected QsyIncoming event");
        match ev {
            ControlEvent::QsyIncoming {
                token,
                n_candidates,
            } => {
                assert_eq!(token, token_from_line, "QsyIncoming token must match frame");
                assert_eq!(
                    n_candidates, 3,
                    "n_candidates must match initiator's candidate list length"
                );
            }
            other => panic!("expected QsyIncoming, got {other:?}"),
        }
    }

    #[test]
    fn ptt_watchdog_fires_after_deadline() {
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);
        // Use a small ptt_max_duration so Instant::now() is guaranteed to be past
        // the deadline without requiring the subtraction to go back farther than the
        // process uptime (avoids panic on freshly booted CI containers).
        let mut state = RuntimeControlState {
            ptt_asserted_at: Some(
                Instant::now()
                    .checked_sub(Duration::from_millis(100))
                    .unwrap_or_else(Instant::now),
            ),
            ptt_max_duration: Duration::from_nanos(1),
            ..RuntimeControlState::default()
        };
        let fired = check_ptt_watchdog(&mut state, &ev_tx);
        assert!(fired, "watchdog must fire when deadline is exceeded");
        assert!(
            state.ptt_asserted_at.is_none(),
            "ptt_asserted_at must be cleared"
        );
        let ev = rx.try_recv().expect("PttChanged event must be emitted");
        assert!(
            matches!(ev, ControlEvent::PttChanged { active: false }),
            "event must be PttChanged {{active: false}}"
        );
    }

    #[test]
    fn ptt_watchdog_silent_before_deadline() {
        let (tx, _) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);
        let mut state = RuntimeControlState {
            ptt_asserted_at: Some(Instant::now()),
            ptt_max_duration: Duration::from_secs(180),
            ..RuntimeControlState::default()
        };
        let fired = check_ptt_watchdog(&mut state, &ev_tx);
        assert!(!fired, "watchdog must not fire before deadline");
        assert!(
            state.ptt_asserted_at.is_some(),
            "ptt_asserted_at must remain set"
        );
    }

    #[test]
    fn ptt_watchdog_silent_when_ptt_not_active() {
        let (tx, _) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);
        let mut state = RuntimeControlState::default();
        let fired = check_ptt_watchdog(&mut state, &ev_tx);
        assert!(!fired, "watchdog must not fire when PTT is not active");
    }
}
