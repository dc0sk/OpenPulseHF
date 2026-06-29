use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock as StdRwLock};
use tokio::sync::{broadcast, RwLock};

use openpulse_core::ack::{AckFrame, AckType};
use openpulse_core::handshake::InMemoryTrustStore;
use openpulse_core::relay::RelayForwarder;
use openpulse_modem::ModemEngine;
use openpulse_radio::{NoOpPtt, PttController};

use crate::state::TncState;

/// Shared state coordinating the command and data port handlers.
pub struct ModemBridge {
    pub engine: Arc<std::sync::Mutex<ModemEngine>>,
    pub state: Arc<RwLock<TncState>>,
    pub callsign: Arc<RwLock<String>>,
    pub gridsquare: Arc<RwLock<String>>,
    /// ARQ bandwidth in Hz (200/500/1000/2000); default 500.
    pub arq_bw: Arc<RwLock<u16>>,
    /// ARQ connection timeout in seconds; default 120.
    pub arq_timeout: Arc<RwLock<u16>>,
    /// Active modem mode string; changeable at runtime via the `WAVEFORM` command.
    pub mode: Arc<StdRwLock<String>>,
    /// Unsolicited event push channel to all connected command clients.
    pub event_tx: broadcast::Sender<String>,
    /// Received data pushed from the worker to all data port clients.
    pub rx_data_tx: broadcast::Sender<Vec<u8>>,
    /// Data queued by data port clients for transmission.
    pub tx_data_tx: std::sync::mpsc::SyncSender<Vec<u8>>,
    /// Pending TX bytes (for BUFFER command).
    pub tx_pending: Arc<AtomicUsize>,
    /// When true the worker echoes TX data back as RX data without RF.
    pub loopback: bool,
    /// When true the next TX frame uses FEC encoding (FECSEND mode).
    pub fec_tx: Arc<AtomicBool>,
    /// When true the worker receives with FEC decoding (FECRCV mode).
    pub fec_rx: Arc<AtomicBool>,
    /// When true the TNC accepts relay frames alongside direct ARQ traffic.
    pub mesh_mode: Arc<AtomicBool>,
    /// Loaded from `trust.store_path`; empty if no path is configured.
    pub trust_store: Arc<InMemoryTrustStore>,
    /// Present when relay is enabled in the config; enforces hop-limit and dedup.
    pub relay_forwarder: Option<Arc<std::sync::Mutex<RelayForwarder>>>,
    /// PTT controller for hardware transmit gating; `NoOpPtt` when not configured.
    pub ptt: Arc<std::sync::Mutex<Box<dyn PttController + Send>>>,
}

impl ModemBridge {
    pub fn new(
        engine: ModemEngine,
        mode: String,
        loopback: bool,
        trust_store: InMemoryTrustStore,
        relay_forwarder: Option<RelayForwarder>,
    ) -> (Arc<Self>, std::sync::mpsc::Receiver<Vec<u8>>) {
        Self::with_ptt(
            engine,
            mode,
            loopback,
            trust_store,
            relay_forwarder,
            Box::new(NoOpPtt::new()),
        )
    }

    pub fn with_ptt(
        engine: ModemEngine,
        mode: String,
        loopback: bool,
        trust_store: InMemoryTrustStore,
        relay_forwarder: Option<RelayForwarder>,
        ptt: Box<dyn PttController + Send>,
    ) -> (Arc<Self>, std::sync::mpsc::Receiver<Vec<u8>>) {
        let (event_tx, _) = broadcast::channel(32);
        let (rx_data_tx, _) = broadcast::channel(32);
        let (tx_data_tx, tx_data_rx) = std::sync::mpsc::sync_channel(64);
        let bridge = Arc::new(Self {
            engine: Arc::new(std::sync::Mutex::new(engine)),
            state: Arc::new(RwLock::new(TncState::Disc)),
            callsign: Arc::new(RwLock::new(String::new())),
            gridsquare: Arc::new(RwLock::new(String::new())),
            arq_bw: Arc::new(RwLock::new(500)),
            arq_timeout: Arc::new(RwLock::new(120)),
            mode: Arc::new(StdRwLock::new(mode)),
            event_tx,
            rx_data_tx,
            tx_data_tx,
            tx_pending: Arc::new(AtomicUsize::new(0)),
            loopback,
            fec_tx: Arc::new(AtomicBool::new(false)),
            fec_rx: Arc::new(AtomicBool::new(false)),
            mesh_mode: Arc::new(AtomicBool::new(false)),
            trust_store: Arc::new(trust_store),
            relay_forwarder: relay_forwarder.map(|f| Arc::new(std::sync::Mutex::new(f))),
            ptt: Arc::new(std::sync::Mutex::new(ptt)),
        });
        (bridge, tx_data_rx)
    }

    /// Push an unsolicited event line to all connected command clients.
    pub fn push_event(&self, msg: impl Into<String>) {
        let _ = self.event_tx.send(msg.into());
    }

    /// Update TNC state.
    pub async fn set_state(&self, state: TncState) {
        *self.state.write().await = state;
    }
}

/// Spawn the background worker thread that processes TX/RX data via the engine.
pub fn spawn_worker(bridge: Arc<ModemBridge>, tx_data_rx: std::sync::mpsc::Receiver<Vec<u8>>) {
    std::thread::Builder::new()
        .name("ardop-modem-worker".into())
        .spawn(move || worker_loop(bridge, tx_data_rx))
        .expect("failed to spawn ardop-modem-worker thread");
}

fn worker_loop(bridge: Arc<ModemBridge>, tx_data_rx: std::sync::mpsc::Receiver<Vec<u8>>) {
    // Last successful ARQ exchange, for the ARQTIMEOUT inactivity disconnect.
    let mut last_activity = std::time::Instant::now();
    // Last applied ARQBW cap (Hz); 0 = none applied yet, so the first real value always applies.
    let mut last_arq_bw: u16 = 0;
    loop {
        // Snapshot the current mode once per iteration to avoid holding the lock across I/O.
        let mode = bridge
            .mode
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();

        // Apply the ARQBW host cap to the adaptive ladder when it changes (no-op when no adaptive
        // session is active, since `adaptive_profile_modes()` is then empty).
        if let Ok(bw) = bridge.arq_bw.try_read().map(|g| *g) {
            if bw != last_arq_bw {
                let mut engine = bridge.engine.lock().unwrap_or_else(|e| e.into_inner());
                if engine.current_tx_level().is_some() {
                    let modes = engine.adaptive_profile_modes();
                    let cap =
                        openpulse_qsy::bandplan::max_speed_level_for_bandwidth(&modes, bw as u32);
                    engine.set_arq_max_tx_level(cap);
                    tracing::debug!(arq_bw_hz = bw, ?cap, "applied ARQBW cap to adaptive ladder");
                }
                drop(engine);
                last_arq_bw = bw;
            }
        }

        // ARQTIMEOUT: drop an idle connection after `arq_timeout` seconds with no successful
        // exchange. Uses non-blocking lock access since this is a sync worker over tokio RwLocks.
        if let Ok(timeout_s) = bridge.arq_timeout.try_read().map(|g| *g) {
            let connected = matches!(
                bridge.state.try_read().as_deref(),
                Ok(TncState::Connected { .. })
            );
            if connected
                && last_activity.elapsed() >= std::time::Duration::from_secs(timeout_s as u64)
            {
                if let Ok(mut st) = bridge.state.try_write() {
                    *st = TncState::Disc;
                }
                let _ = bridge.event_tx.send("DISCONNECTED".to_string());
                tracing::info!(timeout_s, "ARQ connection timed out (ARQTIMEOUT)");
                last_activity = std::time::Instant::now();
            }
        }

        while let Ok(data) = tx_data_rx.try_recv() {
            let len = data.len();
            // Clear one-shot flag regardless of path so loopback mode doesn't leak it.
            let use_fec = bridge.fec_tx.swap(false, Ordering::Relaxed);
            if bridge.loopback {
                let _ = bridge.rx_data_tx.send(data);
            } else {
                // Acquire the lock once to read adaptive state and perform TX in the same scope.
                let mut engine = bridge.engine.lock().unwrap_or_else(|e| e.into_inner());
                let adaptive = engine.current_tx_level().is_some();

                if adaptive && !use_fec {
                    // ISS adaptive path: transmit with ARQ retry (up to 3 retransmits).
                    // FEC transmissions use their own reliability mechanism and skip ARQ.
                    match engine.transmit_arq(&data, &mode, None, 3) {
                        Ok(rate_event) => {
                            tracing::debug!(rate_event = ?rate_event, "ARQ TX succeeded");
                            last_activity = std::time::Instant::now();
                        }
                        Err(e) => {
                            tracing::warn!("ARQ TX failed: {e}");
                        }
                    }
                    drop(engine);
                } else {
                    let tx_result = if use_fec {
                        engine.transmit_with_fec(&data, &mode, None)
                    } else {
                        engine.transmit(&data, &mode, None)
                    };
                    drop(engine);
                    if let Err(ref e) = tx_result {
                        tracing::warn!("modem TX error: {e}");
                    }
                    if tx_result.is_ok() {
                        if let Some(rx) = do_receive(&bridge, &mode) {
                            maybe_relay_forward(&bridge, &rx, &mode);
                            if !rx.is_empty() {
                                let _ = bridge.rx_data_tx.send(rx);
                            }
                        }
                    }
                }
            }
            bridge.tx_pending.fetch_sub(
                len.min(bridge.tx_pending.load(Ordering::Relaxed)),
                Ordering::Relaxed,
            );
        }

        if !bridge.loopback {
            // IRS path: acquire the engine lock once for both the adaptive check and
            // the receive+ACK dispatch to avoid lock churn and inconsistent state.
            let mut engine = bridge.engine.lock().unwrap_or_else(|e| e.into_inner());
            let adaptive = engine.current_tx_level().is_some();
            let received = if adaptive {
                // Adaptive IRS: receive with SNR hint then immediately reply with ACK
                // or Nack — all within the same lock scope so no other caller can
                // interleave between receive and the ACK transmit.
                match engine.receive_with_ack_hint(&mode, None) {
                    Ok((payload, ack_type)) => {
                        let ack_frame = AckFrame::new(ack_type, &mode);
                        if let Err(e) = engine.transmit_ack_with_short_fec(&ack_frame, None) {
                            tracing::warn!("IRS ACK transmit failed: {e}");
                        }
                        Some(payload)
                    }
                    Err(e) => {
                        tracing::debug!("IRS receive_with_ack_hint failed ({e}); sending Nack");
                        let nack = AckFrame::new(AckType::Nack, &mode);
                        if let Err(e) = engine.transmit_ack_with_short_fec(&nack, None) {
                            tracing::warn!("IRS Nack transmit failed: {e}");
                        }
                        None
                    }
                }
            } else {
                let use_fec_rx = bridge.fec_rx.load(Ordering::Relaxed);
                if use_fec_rx {
                    engine
                        .receive_with_fec(&mode, None)
                        .inspect_err(|e| {
                            tracing::debug!("non-adaptive IRS receive_with_fec failed: {e}")
                        })
                        .ok()
                } else {
                    engine
                        .receive(&mode, None)
                        .inspect_err(|e| tracing::debug!("non-adaptive IRS receive failed: {e}"))
                        .ok()
                }
            };
            drop(engine);
            if let Some(rx) = received {
                last_activity = std::time::Instant::now();
                maybe_relay_forward(&bridge, &rx, &mode);
                if !rx.is_empty() {
                    let _ = bridge.rx_data_tx.send(rx);
                }
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn do_receive(bridge: &ModemBridge, mode: &str) -> Option<Vec<u8>> {
    let use_fec_rx = bridge.fec_rx.load(Ordering::Relaxed);
    if use_fec_rx {
        bridge
            .engine
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .receive_with_fec(mode, None)
            .ok()
    } else {
        bridge
            .engine
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .receive(mode, None)
            .ok()
    }
}

/// Attempt to forward `payload` as a relay `WireEnvelope` when relay is enabled.
///
/// ## Layering contract
/// `engine.receive()` returns the decoded HPX frame *payload*.  A relay sender
/// must therefore call `engine.transmit(&envelope.encode()?, …)` so that the
/// `WireEnvelope` bytes land in the HPX payload slot.  The relay receiver then
/// gets those bytes here and probes them with `WireEnvelope::decode`.
///
/// The probe is cheap: `decode` checks the 4-byte `OPHF` magic first and
/// returns `Err(InvalidMagic)` immediately for ordinary user-data payloads.
/// When the magic matches the forwarder increments `hop_index` and re-transmits.
fn maybe_relay_forward(bridge: &ModemBridge, payload: &[u8], mode: &str) {
    use openpulse_core::wire_query::WireEnvelope;

    let Some(ref fwd_arc) = bridge.relay_forwarder else {
        return;
    };

    let Ok(envelope) = WireEnvelope::decode(payload) else {
        return;
    };

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let forwarded = {
        let mut fwd = fwd_arc.lock().unwrap_or_else(|e| e.into_inner());
        fwd.forward(&envelope, now_ms)
    };

    match forwarded {
        Ok(out_envelope) => {
            if let Ok(out_bytes) = out_envelope.encode() {
                if let Err(e) = bridge
                    .engine
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .transmit(&out_bytes, mode, None)
                {
                    tracing::warn!("relay TX error: {e}");
                }
                tracing::debug!(
                    session_id = out_envelope.session_id,
                    hop_index = out_envelope.hop_index,
                    "relay: forwarded envelope"
                );
            }
        }
        Err(e) => {
            tracing::debug!("relay: envelope not forwarded: {e:?}");
        }
    }
}
