/// Transmission metadata for regulatory compliance logging.
///
/// Every transmitted frame should be associated with:
/// - Station callsign (identifies the operator/station)
/// - Transmission timestamp (millisecond precision, UTC)
/// - Mode (modulation)
/// - Power (watts)
///
/// This metadata is captured at transmission time and logged separately
/// from the wire frame itself.
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

/// Errors produced while appending transmission metadata to a session log.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TxSessionLogError {
    #[error("tx session log station mismatch: log station '{expected}', frame station '{got}'")]
    StationMismatch { expected: String, got: String },
}

/// Metadata associated with a single transmitted frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TxMetadata {
    /// Station callsign (e.g., "W5ABC")
    pub station_id: String,
    /// Transmission timestamp (milliseconds since Unix epoch, UTC)
    pub timestamp_ms: u64,
    /// Modulation mode (e.g., "BPSK250")
    pub mode: String,
    /// TX power in watts
    pub power_watts: f32,
    /// Frame sequence number (for association with wire frame)
    pub frame_sequence: u16,
}

impl TxMetadata {
    /// Create new transmission metadata with current timestamp.
    pub fn new(
        station_id: impl Into<String>,
        mode: impl Into<String>,
        power_watts: f32,
        frame_sequence: u16,
    ) -> Self {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        Self {
            station_id: station_id.into(),
            timestamp_ms,
            mode: mode.into(),
            power_watts,
            frame_sequence,
        }
    }

    /// Create transmission metadata with explicit timestamp (for testing).
    pub fn with_timestamp(
        station_id: impl Into<String>,
        timestamp_ms: u64,
        mode: impl Into<String>,
        power_watts: f32,
        frame_sequence: u16,
    ) -> Self {
        Self {
            station_id: station_id.into(),
            timestamp_ms,
            mode: mode.into(),
            power_watts,
            frame_sequence,
        }
    }

    /// Format metadata as a compact log line.
    pub fn to_log_line(&self) -> String {
        format!(
            "[{}] {} seq={} @{:.0}W mode={}",
            self.station_id, self.timestamp_ms, self.frame_sequence, self.power_watts, self.mode
        )
    }
}

/// Frames the in-memory window keeps. Disk is the record (the engine spills every frame); memory
/// is a query cache, so this only has to cover "what happened recently" — at a realistic HF frame
/// cadence it is hours of traffic.
pub const DEFAULT_TX_LOG_RETAIN: usize = 1024;

/// Session transmission log — a bounded in-memory window over the §97 TX record.
///
/// **This is a window, not the whole session (#1110).** It used to be an unbounded `Vec` appended
/// at every emit seam with nothing ever clearing it, so a long-running daemon grew it without
/// limit. It is now capped at `retain` frames, and `ModemEngine` spills every frame to disk so the
/// full record survives both the cap and a restart — the in-memory log was also lost on restart,
/// which made it unfit as a compliance record independently of its size.
///
/// `frame_count()` therefore reports the **true total transmitted**, not the retained count.
/// Reporting the window size there would be the truncation-that-reads-as-completeness defect: a
/// caller asking "how many frames did we transmit" would silently get a smaller number once the
/// cap engaged.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxSessionLog {
    /// Station callsign
    pub station_id: String,
    /// The most recent frames, oldest first — at most `retain` of them. NOT the whole session.
    pub frames: Vec<TxMetadata>,
    /// Window size.
    retain: usize,
    /// Every frame ever logged in this session, including ones evicted from the window.
    total_logged: u64,
}

impl Default for TxSessionLog {
    fn default() -> Self {
        Self::new(String::new())
    }
}

impl TxSessionLog {
    /// Create a new transmission session log with the default window size.
    pub fn new(station_id: impl Into<String>) -> Self {
        Self {
            station_id: station_id.into(),
            frames: Vec::new(),
            retain: DEFAULT_TX_LOG_RETAIN,
            total_logged: 0,
        }
    }

    /// Create a log with an explicit window size. A `retain` of 0 is raised to 1: a window of
    /// nothing would make `get_by_sequence`/`time_range` permanently empty, which is a silent
    /// behaviour change rather than a configuration.
    pub fn with_retain(station_id: impl Into<String>, retain: usize) -> Self {
        Self {
            retain: retain.max(1),
            ..Self::new(station_id)
        }
    }

    /// Append a transmitted frame, evicting the oldest if the window is full.
    pub fn log_frame(&mut self, metadata: TxMetadata) -> Result<(), TxSessionLogError> {
        if metadata.station_id != self.station_id {
            return Err(TxSessionLogError::StationMismatch {
                expected: self.station_id.clone(),
                got: metadata.station_id,
            });
        }
        self.frames.push(metadata);
        // Counted only after the station check, so a rejected frame is not counted as transmitted.
        self.total_logged = self.total_logged.saturating_add(1);
        if self.frames.len() > self.retain {
            let excess = self.frames.len() - self.retain;
            self.frames.drain(0..excess);
        }
        Ok(())
    }

    /// Total frames transmitted in this session — including any evicted from the window.
    pub fn frame_count(&self) -> usize {
        // usize on every target this runs on is 64-bit; the saturating cast is for correctness on
        // a 32-bit build rather than a real concern.
        self.total_logged.min(usize::MAX as u64) as usize
    }

    /// How many frames the in-memory window currently holds.
    pub fn retained(&self) -> usize {
        self.frames.len()
    }

    /// Whether the window has evicted anything — i.e. `frames` is no longer the whole session.
    pub fn is_truncated(&self) -> bool {
        self.frame_count() > self.frames.len()
    }

    /// Get frame by sequence number, if present **in the retained window**.
    pub fn get_by_sequence(&self, seq: u16) -> Option<&TxMetadata> {
        self.frames.iter().find(|m| m.frame_sequence == seq)
    }

    /// Minimum and maximum timestamps **in the retained window**, not in the whole session.
    pub fn time_range(&self) -> Option<(u64, u64)> {
        if self.frames.is_empty() {
            return None;
        }
        let min = self.frames.iter().map(|m| m.timestamp_ms).min()?;
        let max = self.frames.iter().map(|m| m.timestamp_ms).max()?;
        Some((min, max))
    }

    /// Export as JSON for logging/compliance records.
    pub fn to_json_string(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tx_metadata_creation() {
        let meta = TxMetadata::with_timestamp("W5ABC", 1000, "BPSK250", 50.0, 1);
        assert_eq!(meta.station_id, "W5ABC");
        assert_eq!(meta.timestamp_ms, 1000);
        assert_eq!(meta.mode, "BPSK250");
        assert_eq!(meta.power_watts, 50.0);
        assert_eq!(meta.frame_sequence, 1);
    }

    #[test]
    fn test_tx_metadata_log_line() {
        let meta = TxMetadata::with_timestamp("DL1ABC", 5000, "QPSK500", 100.0, 42);
        let line = meta.to_log_line();
        assert!(line.contains("DL1ABC"));
        assert!(line.contains("5000"));
        assert!(line.contains("42"));
        assert!(line.contains("100"));
    }

    #[test]
    fn test_tx_session_log() {
        let mut log = TxSessionLog::new("G4XYZ");
        assert_eq!(log.frame_count(), 0);

        log.log_frame(TxMetadata::with_timestamp("G4XYZ", 1000, "BPSK31", 25.0, 1))
            .expect("matching station should append");
        log.log_frame(TxMetadata::with_timestamp("G4XYZ", 2000, "BPSK31", 25.0, 2))
            .expect("matching station should append");
        log.log_frame(TxMetadata::with_timestamp(
            "G4XYZ", 3000, "QPSK250", 50.0, 3,
        ))
        .expect("matching station should append");

        assert_eq!(log.frame_count(), 3);
        assert_eq!(log.get_by_sequence(2).unwrap().mode, "BPSK31");
        assert_eq!(log.get_by_sequence(3).unwrap().power_watts, 50.0);

        let (min, max) = log.time_range().unwrap();
        assert_eq!(min, 1000);
        assert_eq!(max, 3000);
    }

    #[test]
    fn test_tx_session_log_json() {
        let mut log = TxSessionLog::new("N0CALL");
        log.log_frame(TxMetadata::with_timestamp(
            "N0CALL", 1000, "FSK4-ACK", 10.0, 99,
        ))
        .expect("matching station should append");

        let json = log.to_json_string().unwrap();
        assert!(json.contains("N0CALL"));
        assert!(json.contains("1000"));
        assert!(json.contains("FSK4-ACK"));
    }

    #[test]
    fn test_tx_session_log_rejects_cross_station_frame() {
        let mut log = TxSessionLog::new("W5ABC");

        let err = log
            .log_frame(TxMetadata::with_timestamp(
                "K7XYZ", 1000, "BPSK250", 50.0, 1,
            ))
            .expect_err("cross-station frame should be rejected");

        assert_eq!(
            err,
            TxSessionLogError::StationMismatch {
                expected: "W5ABC".to_string(),
                got: "K7XYZ".to_string(),
            }
        );
        assert_eq!(log.frame_count(), 0);
    }
}
