use anyhow::{Context, Result};
use openpulse_modem::ModemEngine;

pub fn run(data: &str, mode: &str, device: Option<&str>, engine: &mut ModemEngine) -> Result<()> {
    engine
        .transmit(data.as_bytes(), mode, device)
        .context("transmit failed")?;
    println!("Transmitted {} bytes in {mode} mode.", data.len());
    Ok(())
}
