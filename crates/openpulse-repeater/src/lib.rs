//! Cross-band repeater: receives frames on one modem engine and re-transmits
//! them on a second engine through a separate rig.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use openpulse_modem::ModemEngine;
use openpulse_radio::PttController;
use thiserror::Error;

pub use config::RepeaterConfig;

pub mod config;

#[derive(Debug, Error)]
pub enum RepeaterError {
    #[error("modem error: {0}")]
    Modem(String),
    #[error("PTT error: {0}")]
    Ptt(#[from] openpulse_radio::PttError),
}

/// Relays decoded frames from `engine_rx` to `engine_tx`, asserting PTT on `rig_b`.
pub struct CrossBandRepeater {
    /// PTT controller for the transmitting rig.
    rig_b: Box<dyn PttController + Send>,
    /// Modem engine used for receiving (driven by rig_a audio).
    engine_rx: ModemEngine,
    /// Modem engine used for re-transmitting (drives rig_b audio).
    engine_tx: ModemEngine,
    config: RepeaterConfig,
}

impl CrossBandRepeater {
    /// Create a new cross-band repeater.
    ///
    /// - `rig_b`: PTT controller for the transmitting rig.
    /// - `engine_rx`: modem engine wired to rig_a's audio input.
    /// - `engine_tx`: modem engine wired to rig_b's audio output.
    /// - `config`: repeater configuration.
    pub fn new(
        rig_b: Box<dyn PttController + Send>,
        engine_rx: ModemEngine,
        engine_tx: ModemEngine,
        config: RepeaterConfig,
    ) -> Self {
        Self {
            rig_b,
            engine_rx,
            engine_tx,
            config,
        }
    }

    /// Attempt to receive one frame from `engine_rx` and relay it via `engine_tx`.
    ///
    /// Returns the number of bytes relayed, or `None` if no frame was available.
    /// FEC is not applied on the relay path (raw mode).
    pub fn relay_one_frame(&mut self) -> Result<Option<usize>, RepeaterError> {
        if !self.config.enabled {
            return Ok(None);
        }

        let bytes = self
            .engine_rx
            .receive(&self.config.mode.clone(), None)
            .map_err(|e| RepeaterError::Modem(e.to_string()))?;

        if bytes.is_empty() {
            return Ok(None);
        }

        let n = bytes.len();
        if !self.config.full_duplex {
            self.rig_b.assert_ptt().map_err(RepeaterError::Ptt)?;
        }
        self.engine_tx
            .transmit(&bytes, &self.config.mode.clone(), None)
            .map_err(|e| RepeaterError::Modem(e.to_string()))?;
        if !self.config.full_duplex {
            if self.config.tx_hang_ms > 0 {
                std::thread::sleep(std::time::Duration::from_millis(self.config.tx_hang_ms));
            }
            self.rig_b.release_ptt().map_err(RepeaterError::Ptt)?;
        }

        tracing::info!(
            mode = %self.config.mode,
            bytes = n,
            "cross-band relay: relayed frame"
        );

        Ok(Some(n))
    }

    /// Run in full-duplex mode: assert PTT once, relay frames until `stop` is set,
    /// then release PTT.
    ///
    /// Returns the total number of frames relayed. PTT is guaranteed to be released
    /// even if an error occurs mid-session.
    pub fn run_full_duplex(&mut self, stop: Arc<AtomicBool>) -> Result<u64, RepeaterError> {
        if !self.config.enabled {
            return Ok(0);
        }
        self.rig_b.assert_ptt().map_err(RepeaterError::Ptt)?;
        let mut count = 0u64;
        let result = loop {
            if stop.load(Ordering::Relaxed) {
                break Ok(count);
            }
            match self.relay_one_frame() {
                Ok(Some(_)) => count += 1,
                Ok(None) => {}
                Err(e) => break Err(e),
            }
        };
        // Always release PTT before returning, even on error.
        let release_result = self.rig_b.release_ptt().map_err(RepeaterError::Ptt);
        match result {
            Ok(n) => release_result.map(|_| n),
            Err(e) => {
                // Release error is swallowed; original error takes priority.
                let _ = release_result;
                Err(e)
            }
        }
    }

    /// Return whether the repeater is enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }
}
