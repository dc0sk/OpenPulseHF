//! KissBridge: shared state coordinating the TCP server and modem worker.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock as StdRwLock};

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
    /// Active modem mode string; changeable at runtime via `set_mode()`.
    pub mode: Arc<StdRwLock<String>>,
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
            mode: Arc::new(StdRwLock::new(mode)),
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

impl KissBridge {
    /// Change the active modem mode at runtime.
    pub fn set_mode(&self, mode: String) {
        *self.mode.write().unwrap_or_else(|e| e.into_inner()) = mode;
    }

    /// Read the active modem mode.
    pub fn current_mode(&self) -> String {
        self.mode.read().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// The AX.25 source callsign of `frame` iff it is valid for on-air TX (§97.119): a decodable address
/// header whose source is non-empty and not `N0CALL`. Returns `None` otherwise so the worker can refuse
/// unidentified frames uniformly. The address header format is common to all AX.25 frame types, so this
/// does not restrict connected-mode traffic.
fn tx_source_callsign(frame: &[u8]) -> Option<String> {
    let call = crate::ax25::Ax25Addr::source_from_frame(frame)?.callsign_str();
    openpulse_core::station_id::callsign_is_valid(&call).then_some(call)
}

/// Spawn the background worker thread that drives TX/RX via the modem engine.
pub fn spawn_worker(bridge: Arc<KissBridge>, tx_data_rx: std::sync::mpsc::Receiver<Vec<u8>>) {
    std::thread::Builder::new()
        .name("kiss-modem-worker".into())
        .spawn(move || worker_loop(bridge, tx_data_rx))
        .expect("failed to spawn kiss-modem-worker thread");
}

fn worker_loop(bridge: Arc<KissBridge>, tx_data_rx: std::sync::mpsc::Receiver<Vec<u8>>) {
    loop {
        let mode = bridge
            .mode
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();

        while let Ok(data) = tx_data_rx.try_recv() {
            let len = data.len();
            if bridge.loopback {
                if bridge.rx_data_tx.send(data).is_err() {
                    tracing::debug!("KISS loopback RX: no subscribers, frame dropped");
                }
            } else if tx_source_callsign(&data).is_none() {
                // §97.119: a packet station identifies via the AX.25 *source* address in each frame.
                // Refuse to key the transmitter for a frame whose source is absent, `N0CALL`, or not a
                // decodable AX.25 address header — an unidentified emission. (KISS is a bare frame pipe
                // with no host response channel, so this is logged; the frame is dropped.)
                tracing::warn!(
                    "refusing on-air TX: AX.25 source callsign missing or invalid (§97.119)"
                );
            } else {
                match bridge
                    .engine
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .transmit(&data, &mode, None)
                {
                    Ok(_) => {}
                    Err(e) => tracing::warn!("modem TX error: {e}"),
                }
                if let Ok(received) = bridge
                    .engine
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .receive(&mode, None)
                {
                    maybe_relay_forward(&bridge, &received, &mode);
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
                .receive(&mode, None)
            {
                maybe_relay_forward(&bridge, &received, &mode);
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
fn maybe_relay_forward(bridge: &KissBridge, payload: &[u8], mode: &str) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ax25::{Ax25Addr, Ax25UiFrame};
    use openpulse_audio::LoopbackBackend;
    use std::time::Duration;

    /// An AX.25 UI frame with source callsign `src`.
    fn frame_from(src: &str) -> Vec<u8> {
        Ax25UiFrame {
            dest: Ax25Addr::parse("APRS").unwrap(),
            src: Ax25Addr::parse(src).unwrap(),
            info: b"hello".to_vec(),
        }
        .encode()
        .unwrap()
    }

    /// A non-loopback bridge (so TX goes through the modem, exercising the §97.119 gate).
    fn onair_bridge() -> (Arc<KissBridge>, std::sync::mpsc::Receiver<Vec<u8>>) {
        let mut engine = ModemEngine::new(Box::new(LoopbackBackend::default()));
        engine
            .register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
            .expect("register BPSK plugin");
        KissBridge::new(engine, "BPSK250".into(), false)
    }

    #[test]
    fn tx_source_callsign_gates_on_the_ax25_source() {
        assert_eq!(
            tx_source_callsign(&frame_from("W1AW-9")).as_deref(),
            Some("W1AW"),
            "a valid source call is usable for TX"
        );
        assert!(
            tx_source_callsign(&frame_from("N0CALL")).is_none(),
            "placeholder source is refused"
        );
        assert!(
            tx_source_callsign(&frame_from("")).is_none(),
            "empty source is refused"
        );
        assert!(
            tx_source_callsign(&[0u8; 8]).is_none(),
            "a frame too short for an address header is refused"
        );
    }

    #[test]
    fn worker_refuses_invalid_source_but_passes_a_valid_one() {
        let (bridge, rx) = onair_bridge();
        let tx = bridge.tx_data_tx.clone();
        spawn_worker(bridge.clone(), rx);

        // Queued in order: the N0CALL frame must be refused, the valid one transmitted. Waiting for the
        // valid frame to go out proves *both* were processed (in order), so `frames == 1` shows the
        // N0CALL frame was dropped rather than merely not-yet-reached.
        tx.send(frame_from("N0CALL")).expect("queue invalid frame");
        tx.send(frame_from("W1AW-9")).expect("queue valid frame");

        let mut frames = 0;
        for _ in 0..200 {
            frames = bridge
                .engine
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .frames_transmitted();
            if frames > 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            frames, 1,
            "exactly the valid-source frame is transmitted; the N0CALL frame is refused (§97.119)"
        );
    }
}
