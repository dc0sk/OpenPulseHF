//! KissBridge: shared state coordinating the TCP server and modem worker.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use tokio::sync::broadcast;

use openpulse_core::handshake::InMemoryTrustStore;
use openpulse_core::relay::RelayForwarder;
use openpulse_modem::ModemEngine;

/// KissBridge configuration.
#[derive(Debug, Clone)]
pub struct KissConfig {
    /// Bind address (default `127.0.0.1`).
    pub bind_addr: String,
    /// TCP port (KISS default: 8100).
    pub port: u16,
    /// Default modem mode.
    pub mode: String,
    /// When `true`, TX data is echoed as RX without going through the modem engine.
    pub loopback: bool,
}

impl Default for KissConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1".into(),
            port: 8100,
            mode: "BPSK250".into(),
            loopback: false,
        }
    }
}

/// Shared state coordinating the per-client handlers and the modem worker.
pub struct KissBridge {
    pub engine: Arc<std::sync::Mutex<ModemEngine>>,
    pub mode: String,
    /// Raw payloads (AX.25 frames) pushed from the worker to all connected clients.
    pub rx_data_tx: broadcast::Sender<Vec<u8>>,
    /// Raw payloads queued by clients for transmission.
    pub tx_data_tx: std::sync::mpsc::SyncSender<Vec<u8>>,
    /// Pending TX byte count (mirrors ARDOP BUFFER tracking).
    pub tx_pending: Arc<AtomicUsize>,
    pub loopback: bool,
    /// Loaded from `trust.store_path`; empty if no path is configured.
    pub trust_store: Arc<InMemoryTrustStore>,
    /// Present when relay is enabled in the config; enforces hop-limit and dedup.
    pub relay_forwarder: Option<Arc<std::sync::Mutex<RelayForwarder>>>,
}

impl KissBridge {
    pub fn new(
        engine: ModemEngine,
        mode: String,
        loopback: bool,
    ) -> (Arc<Self>, std::sync::mpsc::Receiver<Vec<u8>>) {
        Self::with_trust_and_relay(engine, mode, loopback, Default::default(), None)
    }

    pub fn with_trust_and_relay(
        engine: ModemEngine,
        mode: String,
        loopback: bool,
        trust_store: InMemoryTrustStore,
        relay_forwarder: Option<RelayForwarder>,
    ) -> (Arc<Self>, std::sync::mpsc::Receiver<Vec<u8>>) {
        let (rx_data_tx, _) = broadcast::channel(32);
        let (tx_data_tx, tx_data_rx) = std::sync::mpsc::sync_channel(64);
        let bridge = Arc::new(Self {
            engine: Arc::new(std::sync::Mutex::new(engine)),
            mode,
            rx_data_tx,
            tx_data_tx,
            tx_pending: Arc::new(AtomicUsize::new(0)),
            loopback,
            trust_store: Arc::new(trust_store),
            relay_forwarder: relay_forwarder.map(|f| Arc::new(std::sync::Mutex::new(f))),
        });
        (bridge, tx_data_rx)
    }
}

/// Spawn the background worker thread that drives TX/RX via the modem engine.
pub fn spawn_worker(bridge: Arc<KissBridge>, tx_data_rx: std::sync::mpsc::Receiver<Vec<u8>>) {
    std::thread::spawn(move || worker_loop(bridge, tx_data_rx));
}

fn worker_loop(bridge: Arc<KissBridge>, tx_data_rx: std::sync::mpsc::Receiver<Vec<u8>>) {
    loop {
        while let Ok(data) = tx_data_rx.try_recv() {
            let len = data.len();
            if bridge.loopback {
                let _ = bridge.rx_data_tx.send(data);
            } else {
                match bridge
                    .engine
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .transmit(&data, &bridge.mode, None)
                {
                    Ok(_) => {}
                    Err(e) => tracing::warn!("modem TX error: {e}"),
                }
                if let Ok(received) = bridge
                    .engine
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .receive(&bridge.mode, None)
                {
                    maybe_relay_forward(&bridge, &received);
                    if !received.is_empty() {
                        let _ = bridge.rx_data_tx.send(received);
                    }
                }
            }
            bridge.tx_pending.fetch_sub(
                len.min(bridge.tx_pending.load(Ordering::Relaxed)),
                Ordering::Relaxed,
            );
        }

        if !bridge.loopback {
            if let Ok(received) = bridge
                .engine
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .receive(&bridge.mode, None)
            {
                maybe_relay_forward(&bridge, &received);
                if !received.is_empty() {
                    let _ = bridge.rx_data_tx.send(received);
                }
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(5));
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
fn maybe_relay_forward(bridge: &KissBridge, payload: &[u8]) {
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
                    .transmit(&out_bytes, &bridge.mode, None)
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
