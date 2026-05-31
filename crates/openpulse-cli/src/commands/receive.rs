use anyhow::{Context, Result};
use openpulse_modem::ModemEngine;
use std::time::Duration;

pub fn run(
    mode: &str,
    device: Option<&str>,
    listen_ms: Option<u64>,
    engine: &mut ModemEngine,
) -> Result<()> {
    let payload = match listen_ms {
        Some(ms) => engine
            .receive_with_timeout(mode, device, Duration::from_millis(ms))
            .context("receive failed")?,
        None => engine.receive(mode, device).context("receive failed")?,
    };
    let text = String::from_utf8_lossy(&payload);
    println!("{text}");
    Ok(())
}
