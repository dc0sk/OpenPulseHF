//! Cross-band repeater: receives frames on one modem engine and re-transmits
//! them on a second engine through a separate rig.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use openpulse_core::station_id::StationIdTimer;
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
    /// §97.119 auto-ID of the transmitting rig (rig_b), independent of the daemon's main-engine timer.
    id_timer: Option<StationIdTimer>,
    /// Monotonic clock origin for the ID timer.
    start: Instant,
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
        // Auto-ID only with a callsign and a positive interval; rig_b is an automatically-controlled
        // station (§97.221) that must ID per §97.119, and the daemon's main-engine timer never sees it.
        let id_timer = (!config.callsign.trim().is_empty() && config.id_interval_secs > 0)
            .then(|| StationIdTimer::new(config.id_interval_secs.saturating_mul(1000), 0));
        Self {
            rig_b,
            engine_rx,
            engine_tx,
            config,
            id_timer,
            start: Instant::now(),
        }
    }

    /// Attempt to receive one frame from `engine_rx` and relay it via `engine_tx`.
    ///
    /// Returns the number of bytes relayed, or `None` if no frame was available.
    /// FEC is not applied on the relay path (raw mode).
    pub fn relay_one_frame(&mut self) -> Result<Option<usize>, RepeaterError> {
        let now_ms = self.start.elapsed().as_millis() as u64;
        self.relay_one_frame_at(now_ms)
    }

    /// [`relay_one_frame`] with an explicit monotonic clock (for deterministic ID-timing tests).
    pub fn relay_one_frame_at(&mut self, now_ms: u64) -> Result<Option<usize>, RepeaterError> {
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
        if let Some(t) = self.id_timer.as_mut() {
            t.note_tx(now_ms);
        }
        // §97.119: identify the transmitting rig when the interval has elapsed. In half-duplex PTT is
        // released per-frame, so the ID keys its own PTT; in full-duplex PTT is already held.
        self.maybe_identify(now_ms)?;
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

    /// Transmit `DE <callsign>` on rig_b if the auto-ID interval has elapsed. In half-duplex it keys and
    /// releases its own PTT; in full-duplex the session PTT is already asserted. No-op without a timer.
    fn maybe_identify(&mut self, now_ms: u64) -> Result<(), RepeaterError> {
        let due = self.id_timer.as_ref().is_some_and(|t| t.id_due(now_ms));
        if !due {
            return Ok(());
        }
        let id_body = format!("DE {}", self.config.callsign);
        if !self.config.full_duplex {
            self.rig_b.assert_ptt().map_err(RepeaterError::Ptt)?;
        }
        let tx = self
            .engine_tx
            .transmit(id_body.as_bytes(), &self.config.mode.clone(), None)
            .map_err(|e| RepeaterError::Modem(e.to_string()));
        if !self.config.full_duplex {
            // Release even if the ID transmit failed, so a failure can't leave rig_b keyed.
            self.rig_b.release_ptt().map_err(RepeaterError::Ptt)?;
        }
        tx?;
        if let Some(t) = self.id_timer.as_mut() {
            t.mark_identified(now_ms);
        }
        tracing::info!(callsign = %self.config.callsign, "cross-band relay: transmitted station ID");
        Ok(())
    }

    /// Run the relay loop until `stop` is set, returning the total number of frames relayed.
    ///
    /// In **full-duplex** (`config.full_duplex`) PTT is asserted once for the whole session and
    /// released at the end — that is what the flag means, and the session-long carrier is intended.
    ///
    /// In **half-duplex** (the default) PTT is *not* held here: `relay_one_frame` and
    /// `maybe_identify` key and release per transmission, which is the only correct behaviour on a
    /// shared simplex channel. Keying for the whole session in this mode put an unbounded dead-air
    /// carrier on the band and double-keyed against the per-frame assert (audit 2026-07-19, #2).
    ///
    /// PTT is guaranteed to be released even if an error occurs mid-session.
    pub fn run_full_duplex(&mut self, stop: Arc<AtomicBool>) -> Result<u64, RepeaterError> {
        if !self.config.enabled {
            return Ok(0);
        }
        let hold_ptt = self.config.full_duplex;
        if hold_ptt {
            self.rig_b.assert_ptt().map_err(RepeaterError::Ptt)?;
        }
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
        if !hold_ptt {
            // Nothing was keyed at this level; the per-transmission paths already released.
            return result;
        }
        // Always release PTT before returning, even on error.
        let release_result = self.rig_b.release_ptt().map_err(RepeaterError::Ptt);
        match result {
            Ok(n) => release_result.map(|_| n),
            Err(e) => {
                // PTT release failure is logged but the original error takes priority as return value.
                if let Err(release_err) = release_result {
                    tracing::warn!(error = %release_err, "PTT release failed after repeater error; hardware may remain keyed");
                }
                Err(e)
            }
        }
    }

    /// Return whether the repeater is enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }
}
