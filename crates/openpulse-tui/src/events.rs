//! Background thread that drives `ModemEngine::receive()` and forwards events to the TUI.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use openpulse_core::error::ModemError;
use openpulse_modem::ModemEngine;
use tokio::sync::broadcast;

use crate::app::App;

/// Messages sent from the background worker to the TUI main loop.
pub enum WorkerMsg {
    Event(openpulse_modem::EngineEvent),
    FatalError(String),
}

/// Spawn a background thread that calls `engine.receive()` in a loop, forwarding
/// all emitted `EngineEvent`s to the TUI via the returned `mpsc::Receiver`.
pub fn spawn_worker(
    mut engine: ModemEngine,
    mode: String,
    mut rx: broadcast::Receiver<openpulse_modem::EngineEvent>,
) -> mpsc::Receiver<WorkerMsg> {
    let (tx, recv) = mpsc::channel();
    thread::spawn(move || loop {
        let no_data = match engine.receive(&mode, None) {
            Ok(_) => false,
            // Demodulation/Frame errors are expected when the loopback buffer is empty.
            Err(ModemError::Demodulation(_)) | Err(ModemError::Frame(_)) => true,
            Err(ModemError::ChannelBusy) => false,
            Err(e) => {
                let _ = tx.send(WorkerMsg::FatalError(e.to_string()));
                return;
            }
        };

        // Avoid busy-looping when the loopback backend has no samples queued.
        if no_data {
            thread::sleep(Duration::from_millis(5));
        }

        loop {
            use tokio::sync::broadcast::error::TryRecvError;
            match rx.try_recv() {
                Ok(ev) => {
                    if tx.send(WorkerMsg::Event(ev)).is_err() {
                        return;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Closed) => return,
                Err(TryRecvError::Lagged(_)) => {}
            }
        }
    });
    recv
}

/// Apply all pending worker messages to `App`.
///
/// Returns `Ok(())` while the worker is healthy.  Returns `Err(message)` when
/// the worker sends a fatal error or its channel closes, so the caller can
/// surface the message before exiting.
pub fn drain_worker(app: &mut App, worker: &mpsc::Receiver<WorkerMsg>) -> Result<(), String> {
    loop {
        match worker.try_recv() {
            Ok(WorkerMsg::Event(ev)) => app.apply_event(ev),
            Ok(WorkerMsg::FatalError(msg)) => return Err(msg),
            Err(mpsc::TryRecvError::Empty) => return Ok(()),
            Err(mpsc::TryRecvError::Disconnected) => {
                return Err("worker thread exited unexpectedly".to_string())
            }
        }
    }
}
