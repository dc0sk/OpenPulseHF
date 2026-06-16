//! Modulation-plugin registration shared across CLI commands.

use anyhow::{Context, Result};

use bpsk_plugin::BpskPlugin;
use fsk4_plugin::Fsk4Plugin;
use ofdm_plugin::OfdmPlugin;
use openpulse_modem::ModemEngine;
use psk8_plugin::Psk8Plugin;
use qam64_plugin::Qam64Plugin;
use qpsk_plugin::QpskPlugin;
use scfdma_plugin::ScFdmaPlugin;

/// Register every built-in modulation plugin onto `engine`.
pub fn register_all(engine: &mut ModemEngine) -> Result<()> {
    engine
        .register_plugin(Box::new(BpskPlugin::new()))
        .context("failed to register BPSK plugin")?;
    engine
        .register_plugin(Box::new(Fsk4Plugin::new()))
        .context("failed to register FSK4 plugin")?;
    engine
        .register_plugin(Box::new(OfdmPlugin::new()))
        .context("failed to register OFDM plugin")?;
    engine
        .register_plugin(Box::new(Psk8Plugin::new()))
        .context("failed to register 8PSK plugin")?;
    engine
        .register_plugin(Box::new(Qam64Plugin::new()))
        .context("failed to register 64QAM plugin")?;
    engine
        .register_plugin(Box::new(QpskPlugin::new()))
        .context("failed to register QPSK plugin")?;
    engine
        .register_plugin(Box::new(ScFdmaPlugin::new()))
        .context("failed to register SC-FDMA plugin")?;
    Ok(())
}
