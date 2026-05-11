use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

use openpulse_core::handshake::InMemoryTrustStore;
use openpulse_core::relay::RelayForwarder;
use openpulse_modem::ModemEngine;

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
    pub mode: String,
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
}

impl ModemBridge {
    pub fn new(
        engine: ModemEngine,
        mode: String,
        loopback: bool,
        trust_store: InMemoryTrustStore,
        relay_forwarder: Option<RelayForwarder>,
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
            mode,
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
    std::thread::spawn(move || worker_loop(bridge, tx_data_rx));
}

fn worker_loop(bridge: Arc<ModemBridge>, tx_data_rx: std::sync::mpsc::Receiver<Vec<u8>>) {
    loop {
        while let Ok(data) = tx_data_rx.try_recv() {
            let len = data.len();
            // Clear one-shot flag regardless of path so loopback mode doesn't leak it.
            let use_fec = bridge.fec_tx.swap(false, Ordering::Relaxed);
            if bridge.loopback {
                let _ = bridge.rx_data_tx.send(data);
            } else {
                let mut engine = bridge.engine.lock().unwrap();
                let tx_result = if use_fec {
                    engine.transmit_with_fec(&data, &bridge.mode, None)
                } else {
                    engine.transmit(&data, &bridge.mode, None)
                };
                drop(engine);
                if tx_result.is_ok() {
                    if let Some(rx) = do_receive(&bridge) {
                        maybe_relay_forward(&bridge, &rx);
                        if !rx.is_empty() {
                            let _ = bridge.rx_data_tx.send(rx);
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
            if let Some(rx) = do_receive(&bridge) {
                maybe_relay_forward(&bridge, &rx);
                if !rx.is_empty() {
                    let _ = bridge.rx_data_tx.send(rx);
                }
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn do_receive(bridge: &ModemBridge) -> Option<Vec<u8>> {
    let use_fec_rx = bridge.fec_rx.load(Ordering::Relaxed);
    if use_fec_rx {
        bridge
            .engine
            .lock()
            .unwrap()
            .receive_with_fec(&bridge.mode, None)
            .ok()
    } else {
        bridge
            .engine
            .lock()
            .unwrap()
            .receive(&bridge.mode, None)
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
fn maybe_relay_forward(bridge: &ModemBridge, payload: &[u8]) {
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
        let mut fwd = fwd_arc.lock().unwrap();
        fwd.forward(&envelope, now_ms)
    };

    match forwarded {
        Ok(out_envelope) => {
            if let Ok(out_bytes) = out_envelope.encode() {
                let _ = bridge
                    .engine
                    .lock()
                    .unwrap()
                    .transmit(&out_bytes, &bridge.mode, None);
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
