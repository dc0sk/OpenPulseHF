use anyhow::{Context, Result};
use openpulse_core::fec::FecMode;
use openpulse_modem::ModemEngine;
use std::time::Duration;

pub fn run(
    mode: &str,
    fec: FecMode,
    device: Option<&str>,
    listen_ms: Option<u64>,
    engine: &mut ModemEngine,
) -> Result<()> {
    let payload = match listen_ms {
        Some(ms) => engine
            .receive_with_fec_mode_timeout(mode, fec, device, Duration::from_millis(ms))
            .context("receive failed")?,
        None => engine
            .receive_with_fec_mode(mode, fec, device)
            .context("receive failed")?,
    };
    let text = String::from_utf8_lossy(&payload);
    println!("{text}");
    Ok(())
}
