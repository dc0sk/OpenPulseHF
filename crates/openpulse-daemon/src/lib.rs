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
use openpulse_radio::RigctldController;
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

/// Execute side-effectful control commands against the live modem engine.
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
            let _ = event_tx.send(ControlEvent::PttChanged { active: true });
        }
        ControlCommand::PttRelease => {
            let _ = event_tx.send(ControlEvent::PttChanged { active: false });
        }
        ControlCommand::ConnectPeer { callsign } => {
            let _ = event_tx.send(ControlEvent::RfConnectionChanged {
                connected: true,
                peer: Some(callsign.clone()),
            });
        }
        ControlCommand::DisconnectPeer => {
            let _ = event_tx.send(ControlEvent::RfConnectionChanged {
                connected: false,
                peer: None,
            });
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
        ControlCommand::AcceptQsy { .. } => {
            let _ = event_tx.send(ControlEvent::CommandError {
                command: "accept_qsy".to_string(),
                reason: "not implemented in daemon runtime".to_string(),
            });
        }
        ControlCommand::RejectQsy { .. } => {
            let _ = event_tx.send(ControlEvent::CommandError {
                command: "reject_qsy".to_string(),
                reason: "not implemented in daemon runtime".to_string(),
            });
        }
        ControlCommand::EnableRepeater => {
            let _ = event_tx.send(ControlEvent::CommandError {
                command: "enable_repeater".to_string(),
                reason: "not implemented in daemon runtime".to_string(),
            });
        }
        ControlCommand::DisableRepeater => {
            let _ = event_tx.send(ControlEvent::CommandError {
                command: "disable_repeater".to_string(),
                reason: "not implemented in daemon runtime".to_string(),
            });
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

        apply_command_to_engine(&cmd, &mut engine, &active_mode, &ev_tx, None).await;

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

        apply_command_to_engine(&cmd, &mut engine, &active_mode, &ev_tx, None).await;

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

        apply_command_to_engine(&cmd, &mut engine, &active_mode, &ev_tx, None).await;

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
                "not implemented",
            ),
            (
                ControlCommand::RejectQsy {
                    token: "tok-2".into(),
                },
                "reject_qsy",
                "not implemented",
            ),
            (
                ControlCommand::EnableRepeater,
                "enable_repeater",
                "not implemented",
            ),
            (
                ControlCommand::DisableRepeater,
                "disable_repeater",
                "not implemented",
            ),
        ];

        for (cmd, expected_command, expected_reason_substr) in cases {
            apply_command_to_engine(&cmd, &mut engine, &active_mode, &ev_tx, None).await;
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
}
