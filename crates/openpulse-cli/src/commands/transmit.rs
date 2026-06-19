use anyhow::{Context, Result};
use openpulse_core::fec::FecMode;
use openpulse_modem::ModemEngine;
use openpulse_radio::PttController;

use crate::commands::bandplan_guard::enforce_mode_guardrails;

pub fn run(
    data: &str,
    mode: &str,
    fec: FecMode,
    device: Option<&str>,
    engine: &mut ModemEngine,
    ptt: &mut dyn PttController,
) -> Result<()> {
    enforce_mode_guardrails(mode)?;

    ptt.assert_ptt().context("PTT assert failed")?;
    let tx_result = engine
        .transmit_with_fec_mode(data.as_bytes(), mode, fec, device)
        .context("transmit failed");
    let rel_result = ptt.release_ptt().context("PTT release failed");
    tx_result?;
    rel_result?;
    println!(
        "Transmitted {} bytes in {mode} mode (fec={fec:?}).",
        data.len()
    );
    Ok(())
}
