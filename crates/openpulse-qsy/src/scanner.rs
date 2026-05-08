//! QSY frequency scanner using a rigctld CAT controller.
//!
//! Tunes to each candidate frequency, dwells for `dwell_ms`, reads the S-meter,
//! then returns the rig to the original frequency.

use std::thread;
use std::time::Duration;

use openpulse_radio::{RadioError, RigctldController};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum QsyScannerError {
    #[error("rig control error: {0}")]
    Radio(#[from] RadioError),
}

/// Scans candidate frequencies and returns `(freq_hz, snr_dbm)` readings.
pub struct QsyScanner {
    rig: RigctldController,
    dwell_ms: u64,
}

impl QsyScanner {
    /// Create a scanner using an already-connected `RigctldController`.
    pub fn new(rig: RigctldController, dwell_ms: u64) -> Self {
        Self { rig, dwell_ms }
    }

    /// Scan each candidate frequency.
    ///
    /// Gets the current frequency as the home frequency, tunes to each candidate,
    /// reads the S-meter, then restores the original frequency.  Returns readings
    /// in the same order as the input.
    pub fn scan(&mut self, candidates: &[u64]) -> Result<Vec<(u64, f32)>, QsyScannerError> {
        let home = self.rig.get_frequency()?;
        let mut results = Vec::with_capacity(candidates.len());
        for &freq in candidates {
            self.rig.set_frequency(freq)?;
            if self.dwell_ms > 0 {
                thread::sleep(Duration::from_millis(self.dwell_ms));
            }
            let strength = self.rig.get_signal_strength()? as f32;
            results.push((freq, strength));
        }
        self.rig.set_frequency(home)?;
        Ok(results)
    }
}
