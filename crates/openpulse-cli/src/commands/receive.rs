use anyhow::{Context, Result};
use openpulse_modem::ModemEngine;

pub fn run(mode: &str, device: Option<&str>, engine: &mut ModemEngine) -> Result<()> {
    let payload = engine.receive(mode, device).context("receive failed")?;
    let text = String::from_utf8_lossy(&payload);
    println!("{text}");
    Ok(())
}
