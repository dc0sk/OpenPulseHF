use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

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
}

impl ModemBridge {
    pub fn new(
        engine: ModemEngine,
        mode: String,
        loopback: bool,
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
