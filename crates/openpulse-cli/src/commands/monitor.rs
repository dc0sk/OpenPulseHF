//! `openpulse monitor` — stream engine events as NDJSON to stdout.

use anyhow::Result;
use openpulse_modem::ModemEngine;

/// Run the monitor subcommand.
///
/// Drives the engine receive loop and prints each [`EngineEvent`] as a
/// newline-delimited JSON object to stdout.  Exits on receive error or when
/// the user terminates the process.
pub fn run(engine: &mut ModemEngine, mode: &str) -> Result<()> {
    let mut rx = engine.subscribe();

    loop {
        // receive() drives internal event emission; ignore frame errors
        // (e.g. empty loopback) so we keep looping.
        let _ = engine.receive(mode, None);

        // Drain all queued events after each receive call.
        loop {
            match rx.try_recv() {
                Ok(event) => {
                    if let Ok(json) = serde_json::to_string(&event) {
                        println!("{json}");
                    }
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
                Err(tokio::sync::broadcast::error::TryRecvError::Closed) => return Ok(()),
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(n)) => {
                    eprintln!("monitor: dropped {n} events (channel full)");
                }
            }
        }
    }
}
