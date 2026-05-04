//! Background thread that drives `ModemEngine::receive()` and forwards events to the TUI.

use std::sync::mpsc;
use std::thread;

use openpulse_core::error::ModemError;
use openpulse_modem::ModemEngine;
use tokio::sync::broadcast;

use crate::app::App;

/// Messages sent from the background worker to the TUI main loop.
pub enum WorkerMsg {
    Event(openpulse_modem::EngineEvent),
    #[allow(dead_code)]
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
        match engine.receive(&mode, None) {
            Ok(_)
            | Err(ModemError::Demodulation(_))
            | Err(ModemError::Frame(_))
            | Err(ModemError::ChannelBusy) => {}
            Err(e) => {
                let _ = tx.send(WorkerMsg::FatalError(e.to_string()));
                return;
            }
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

/// Apply all pending worker messages to `App`. Returns `false` when the channel is closed
/// or a fatal error was received (caller should exit).
pub fn drain_worker(app: &mut App, worker: &mpsc::Receiver<WorkerMsg>) -> bool {
    loop {
        match worker.try_recv() {
            Ok(WorkerMsg::Event(ev)) => app.apply_event(ev),
            Ok(WorkerMsg::FatalError(_)) => return false,
            Err(mpsc::TryRecvError::Empty) => return true,
            Err(mpsc::TryRecvError::Disconnected) => return false,
        }
    }
}
