//! Bandplan policy checks for QSY and operating-mode guardrails.

use thiserror::Error;

/// Supported bandplan modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BandplanMode {
    /// HAM/IARU HF bandplan guardrails.
    HamIaru,
}

impl BandplanMode {
    /// Parse from config text.
    pub fn from_str(value: &str) -> Result<Self, BandplanError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "ham-iaru" => Ok(Self::HamIaru),
            other => Err(BandplanError::UnknownMode(other.to_string())),
        }
    }
}

/// Bandplan policy settings.
#[derive(Debug, Clone)]
pub struct BandplanPolicy {
    /// Enables all bandplan-aware checks.
    pub awareness_enabled: bool,
    /// Which ruleset to apply.
    pub mode: BandplanMode,
    /// Enforce per-segment maximum occupied channel width.
    pub enforce_max_channel_width: bool,
    /// Enforce convention-bound digital/data segments.
    pub enforce_segment_conventions: bool,
}

impl Default for BandplanPolicy {
    fn default() -> Self {
        Self {
            awareness_enabled: true,
            mode: BandplanMode::HamIaru,
            enforce_max_channel_width: true,
            enforce_segment_conventions: true,
        }
    }
}

/// Errors emitted while evaluating bandplan policy.
#[derive(Debug, Error)]
pub enum BandplanError {
    #[error("unknown bandplan mode: {0}")]
    UnknownMode(String),
    #[error("frequency {freq_hz} Hz is outside supported amateur HF bands for {mode}")]
    FrequencyOutOfBand { freq_hz: u64, mode: &'static str },
    #[error("frequency {freq_hz} Hz is outside allowed digital segment for {mode}")]
    SegmentViolation { freq_hz: u64, mode: &'static str },
    #[error("unknown modulation mode for bandwidth checks: {mode}")]
    UnknownOperatingMode { mode: String },
    #[error(
        "mode {mode} requires ~{required_hz} Hz occupied bandwidth, exceeds segment limit {max_hz} Hz"
    )]
    ChannelWidthExceeded {
        mode: String,
        required_hz: u32,
        max_hz: u32,
    },
}

impl BandplanPolicy {
    /// Evaluate whether a frequency/mode pair is permitted by the policy.
    pub fn validate_frequency(
        &self,
        freq_hz: u64,
        operating_mode: &str,
    ) -> Result<(), BandplanError> {
        if !self.awareness_enabled {
            return Ok(());
        }

        match self.mode {
            BandplanMode::HamIaru => validate_ham_iaru(
                freq_hz,
                operating_mode,
                self.enforce_segment_conventions,
                self.enforce_max_channel_width,
            ),
        }
    }
}

fn validate_ham_iaru(
    freq_hz: u64,
    operating_mode: &str,
    enforce_segments: bool,
    enforce_width: bool,
) -> Result<(), BandplanError> {
    let segment = find_ham_iaru_segment(freq_hz).ok_or(BandplanError::FrequencyOutOfBand {
        freq_hz,
        mode: "ham-iaru",
    })?;

    if enforce_segments && !(segment.min_hz..=segment.max_hz).contains(&freq_hz) {
        return Err(BandplanError::SegmentViolation {
            freq_hz,
            mode: "ham-iaru",
        });
    }

    if enforce_width {
        let bw =
            occupied_bandwidth_hz(operating_mode).ok_or(BandplanError::UnknownOperatingMode {
                mode: operating_mode.to_string(),
            })?;
        if bw > segment.max_bw_hz {
            return Err(BandplanError::ChannelWidthExceeded {
                mode: operating_mode.to_string(),
                required_hz: bw,
                max_hz: segment.max_bw_hz,
            });
        }
    }

    Ok(())
}

/// Conservative occupied-bandwidth estimates used for policy checks.
pub fn occupied_bandwidth_hz(mode: &str) -> Option<u32> {
    match mode {
        "BPSK31" => Some(100),
        "BPSK63" => Some(150),
        "BPSK100" => Some(200),
        "BPSK250" => Some(500),
        "QPSK125" => Some(350),
        "QPSK250" => Some(700),
        "QPSK500" => Some(1400),
        "QPSK1000" | "QPSK1000-HF" => Some(2800),
        "QPSK2000" => Some(5600),
        "QPSK9600" | "QPSK9600-RRC" => Some(12000),
        "8PSK500" => Some(1500),
        "8PSK1000" | "8PSK1000-HF" => Some(3000),
        "8PSK2000" => Some(6000),
        "8PSK9600" | "8PSK9600-RRC" => Some(12000),
        "64QAM500" => Some(1600),
        "64QAM1000" => Some(3200),
        "64QAM2000-RRC" => Some(6400),
        "OFDM16" => Some(2200),
        "OFDM52" => Some(3200),
        "SCFDMA16" => Some(2200),
        "SCFDMA52" => Some(3200),
        "FSK4-ACK" => Some(400),
        _ => None,
    }
}

#[derive(Clone, Copy)]
struct DigitalSegment {
    min_hz: u64,
    max_hz: u64,
    max_bw_hz: u32,
}

fn find_ham_iaru_segment(freq_hz: u64) -> Option<DigitalSegment> {
    const SEGMENTS: [DigitalSegment; 9] = [
        DigitalSegment {
            min_hz: 1_838_000,
            max_hz: 1_843_000,
            max_bw_hz: 2_700,
        }, // 160m
        DigitalSegment {
            min_hz: 3_570_000,
            max_hz: 3_600_000,
            max_bw_hz: 2_700,
        }, // 80m
        DigitalSegment {
            min_hz: 7_040_000,
            max_hz: 7_125_000,
            max_bw_hz: 2_700,
        }, // 40m
        DigitalSegment {
            min_hz: 10_130_000,
            max_hz: 10_150_000,
            max_bw_hz: 500,
        }, // 30m
        DigitalSegment {
            min_hz: 14_070_000,
            max_hz: 14_112_000,
            max_bw_hz: 2_700,
        }, // 20m
        DigitalSegment {
            min_hz: 18_100_000,
            max_hz: 18_110_000,
            max_bw_hz: 2_700,
        }, // 17m
        DigitalSegment {
            min_hz: 21_070_000,
            max_hz: 21_149_000,
            max_bw_hz: 2_700,
        }, // 15m
        DigitalSegment {
            min_hz: 24_920_000,
            max_hz: 24_929_000,
            max_bw_hz: 2_700,
        }, // 12m
        DigitalSegment {
            min_hz: 28_070_000,
            max_hz: 28_190_000,
            max_bw_hz: 2_700,
        }, // 10m
    ];

    SEGMENTS
        .iter()
        .copied()
        .find(|s| (s.min_hz..=s.max_hz).contains(&freq_hz))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ham_iaru_accepts_known_good_pair() {
        let policy = BandplanPolicy::default();
        assert!(policy.validate_frequency(14_074_000, "BPSK250").is_ok());
    }

    #[test]
    fn ham_iaru_rejects_out_of_segment_frequency() {
        let policy = BandplanPolicy::default();
        assert!(matches!(
            policy.validate_frequency(14_200_000, "BPSK250"),
            Err(BandplanError::FrequencyOutOfBand { .. })
        ));
    }

    #[test]
    fn ham_iaru_enforces_width_on_30m() {
        let policy = BandplanPolicy::default();
        assert!(matches!(
            policy.validate_frequency(10_140_000, "QPSK1000"),
            Err(BandplanError::ChannelWidthExceeded { .. })
        ));
    }

    #[test]
    fn awareness_override_allows_anything() {
        let mut policy = BandplanPolicy::default();
        policy.awareness_enabled = false;
        assert!(policy.validate_frequency(14_200_000, "QPSK2000").is_ok());
    }
}
