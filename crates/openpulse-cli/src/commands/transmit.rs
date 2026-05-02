use anyhow::{Context, Result};
use openpulse_modem::ModemEngine;
use openpulse_radio::PttController;

pub fn run(
    data: &str,
    mode: &str,
    device: Option<&str>,
    engine: &mut ModemEngine,
    ptt: &mut dyn PttController,
) -> Result<()> {
    ptt.assert_ptt().context("PTT assert failed")?;
    let tx_result = engine
        .transmit(data.as_bytes(), mode, device)
        .context("transmit failed");
    let rel_result = ptt.release_ptt().context("PTT release failed");
    tx_result?;
    rel_result?;
    println!("Transmitted {} bytes in {mode} mode.", data.len());
    Ok(())
}
