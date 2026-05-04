//! KissBridge: shared state coordinating the TCP server and modem worker.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use tokio::sync::broadcast;

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
}

impl KissBridge {
    pub fn new(
        engine: ModemEngine,
        mode: String,
        loopback: bool,
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
                let _ = bridge
                    .engine
                    .lock()
                    .unwrap()
                    .transmit(&data, &bridge.mode, None);
                if let Ok(received) = bridge.engine.lock().unwrap().receive(&bridge.mode, None) {
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
            if let Ok(received) = bridge.engine.lock().unwrap().receive(&bridge.mode, None) {
                if !received.is_empty() {
                    let _ = bridge.rx_data_tx.send(received);
                }
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
