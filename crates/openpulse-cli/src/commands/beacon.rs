//! `openpulse beacon` subcommand handler.

use anyhow::Result;
use openpulse_modem::ModemEngine;

/// Run the beacon loop.
///
/// Sends a `BroadcastFrame` every `interval_s` seconds until Ctrl+C.
pub fn run(
    engine: &mut ModemEngine,
    mode: &str,
    interval_s: u64,
    callsign: &str,
    ttl: u8,
) -> Result<()> {
    let payload = format!("DE {callsign}").into_bytes();
    engine.set_callsign(callsign);

    println!("beacon: {callsign} every {interval_s}s via {mode} (ttl={ttl}) — Ctrl+C to stop");

    loop {
        engine.broadcast(&payload, mode, ttl, None)?;
        println!("beacon sent");
        std::thread::sleep(std::time::Duration::from_secs(interval_s));
    }
}
