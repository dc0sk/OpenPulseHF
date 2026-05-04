//! `openpulse monitor` — stream engine events as NDJSON to stdout.

use std::io::{self, Write as _};

use anyhow::Result;
use openpulse_core::error::ModemError;
use openpulse_modem::ModemEngine;

/// Run the monitor subcommand.
///
/// Drives the engine receive loop and prints each [`EngineEvent`] as a
/// newline-delimited JSON object to stdout.  Exits on channel close or when
/// the user terminates the process.
pub fn run(engine: &mut ModemEngine, mode: &str) -> Result<()> {
    let mut rx = engine.subscribe();
    let stdout = io::stdout();

    loop {
        match engine.receive(mode, None) {
            Ok(_) | Err(ModemError::Demodulation(_)) | Err(ModemError::Frame(_)) => {}
            Err(e) => return Err(e.into()),
        }

        // Drain all queued events after each receive call.
        loop {
            match rx.try_recv() {
                Ok(event) => match serde_json::to_string(&event) {
                    Ok(json) => {
                        let mut out = stdout.lock();
                        let _ = writeln!(out, "{json}");
                        let _ = out.flush();
                    }
                    Err(e) => eprintln!("monitor: serialize error: {e}"),
                },
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
                Err(tokio::sync::broadcast::error::TryRecvError::Closed) => return Ok(()),
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(n)) => {
                    eprintln!("monitor: dropped {n} events (channel full)");
                }
            }
        }
    }
}
