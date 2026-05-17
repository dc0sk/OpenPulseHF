//! Bandplan policy checks for QSY and operating-mode guardrails.

use thiserror::Error;

/// Supported bandplan modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BandplanMode {
    /// HAM/IARU HF bandplan guardrails; use region-specific variants instead.
    #[deprecated(
        since = "1.5.0",
        note = "use region-specific variants (HamIaruRegion1/2/3) instead"
    )]
    HamIaru,
    /// IARU Region 1 (Europe, Africa, Middle East) HF bandplan.
    HamIaruRegion1,
    /// IARU Region 2 (Americas) HF bandplan.
    HamIaruRegion2,
    /// IARU Region 3 (Asia-Pacific) HF bandplan.
    HamIaruRegion3,
}

impl BandplanMode {
    fn parse_impl(value: &str) -> Result<Self, BandplanError> {
        match value.trim().to_ascii_lowercase().as_str() {
            #[allow(deprecated)]
            "ham-iaru" => Ok(Self::HamIaru),
            "ham-iaru-r1" | "ham-iaru-region1" => Ok(Self::HamIaruRegion1),
            "ham-iaru-r2" | "ham-iaru-region2" => Ok(Self::HamIaruRegion2),
            "ham-iaru-r3" | "ham-iaru-region3" => Ok(Self::HamIaruRegion3),
            other => Err(BandplanError::UnknownMode(other.to_string())),
        }
    }
}

impl std::str::FromStr for BandplanMode {
    type Err = BandplanError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse_impl(value)
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

/// Additional non-fatal diagnostics emitted while evaluating bandplan policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BandplanWarning {
    /// Region 3 currently uses Region 1 allocations as a conservative proxy.
    Region3UsesRegion1Proxy,
}

impl Default for BandplanPolicy {
    fn default() -> Self {
        Self {
            awareness_enabled: true,
            mode: BandplanMode::HamIaruRegion1,
            enforce_max_channel_width: true,
            enforce_segment_conventions: true,
        }
    }
}

/// Errors emitted while evaluating bandplan policy.
#[derive(Debug, Error, PartialEq)]
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
        "mode {mode} requires ~{required_hz} Hz occupied bandwidth, exceeds policy limit {max_hz} Hz"
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
        self.validate_frequency_with_warnings(freq_hz, operating_mode)
            .map(|_| ())
    }

    /// Evaluate whether a frequency/mode pair is permitted by the policy and
    /// return any non-fatal warnings about approximation or proxy behavior.
    pub fn validate_frequency_with_warnings(
        &self,
        freq_hz: u64,
        operating_mode: &str,
    ) -> Result<Vec<BandplanWarning>, BandplanError> {
        if !self.awareness_enabled {
            return Ok(vec![]);
        }

        let warnings = match self.mode {
            #[allow(deprecated)]
            BandplanMode::HamIaru => {
                validate_ham_iaru_base(
                    freq_hz,
                    operating_mode,
                    self.enforce_segment_conventions,
                    self.enforce_max_channel_width,
                )?;
                vec![]
            }
            BandplanMode::HamIaruRegion1 => {
                validate_iaru_region(
                    freq_hz,
                    operating_mode,
                    ItuRegion::Region1,
                    self.enforce_segment_conventions,
                    self.enforce_max_channel_width,
                )?;
                vec![]
            }
            BandplanMode::HamIaruRegion2 => {
                validate_iaru_region(
                    freq_hz,
                    operating_mode,
                    ItuRegion::Region2,
                    self.enforce_segment_conventions,
                    self.enforce_max_channel_width,
                )?;
                vec![]
            }
            BandplanMode::HamIaruRegion3 => {
                validate_iaru_region(
                    freq_hz,
                    operating_mode,
                    ItuRegion::Region3,
                    self.enforce_segment_conventions,
                    self.enforce_max_channel_width,
                )?;
                vec![BandplanWarning::Region3UsesRegion1Proxy]
            }
        };

        Ok(warnings)
    }
}

fn validate_ham_iaru_base(
    freq_hz: u64,
    operating_mode: &str,
    enforce_segments: bool,
    enforce_width: bool,
) -> Result<(), BandplanError> {
    let band = find_ham_iaru_band(freq_hz).ok_or(BandplanError::FrequencyOutOfBand {
        freq_hz,
        mode: "ham-iaru",
    })?;

    let max_bw_hz = if enforce_segments {
        find_ham_iaru_segment(freq_hz)
            .map(|segment| segment.max_bw_hz)
            .ok_or(BandplanError::SegmentViolation {
                freq_hz,
                mode: "ham-iaru",
            })?
    } else {
        band.max_bw_hz
    };

    if enforce_width {
        let bw =
            occupied_bandwidth_hz(operating_mode).ok_or(BandplanError::UnknownOperatingMode {
                mode: operating_mode.to_string(),
            })?;
        if bw > max_bw_hz {
            return Err(BandplanError::ChannelWidthExceeded {
                mode: operating_mode.to_string(),
                required_hz: bw,
                max_hz: max_bw_hz,
            });
        }
    }

    Ok(())
}

/// ITU radio regions for bandplan allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ItuRegion {
    Region1,
    Region2,
    Region3,
}

impl ItuRegion {
    fn mode_string(self) -> &'static str {
        match self {
            Self::Region1 => "ham-iaru-r1",
            Self::Region2 => "ham-iaru-r2",
            Self::Region3 => "ham-iaru-r3",
        }
    }
}

fn validate_iaru_region(
    freq_hz: u64,
    operating_mode: &str,
    region: ItuRegion,
    enforce_segments: bool,
    enforce_width: bool,
) -> Result<(), BandplanError> {
    let band = find_band_for_region(freq_hz, region).ok_or(BandplanError::FrequencyOutOfBand {
        freq_hz,
        mode: region.mode_string(),
    })?;

    let max_bw_hz = if enforce_segments {
        find_segment_for_region(freq_hz, region)
            .map(|segment| segment.max_bw_hz)
            .ok_or(BandplanError::SegmentViolation {
                freq_hz,
                mode: region.mode_string(),
            })?
    } else {
        band.max_bw_hz
    };

    if enforce_width {
        let bw =
            occupied_bandwidth_hz(operating_mode).ok_or(BandplanError::UnknownOperatingMode {
                mode: operating_mode.to_string(),
            })?;
        if bw > max_bw_hz {
            return Err(BandplanError::ChannelWidthExceeded {
                mode: operating_mode.to_string(),
                required_hz: bw,
                max_hz: max_bw_hz,
            });
        }
    }

    Ok(())
}

fn find_band_for_region(freq_hz: u64, region: ItuRegion) -> Option<HamBand> {
    match region {
        ItuRegion::Region1 => find_region1_band(freq_hz),
        ItuRegion::Region2 => find_region2_band(freq_hz),
        ItuRegion::Region3 => find_region3_band(freq_hz),
    }
}

fn find_segment_for_region(freq_hz: u64, region: ItuRegion) -> Option<DigitalSegment> {
    match region {
        ItuRegion::Region1 => find_region1_segment(freq_hz),
        ItuRegion::Region2 => find_region2_segment(freq_hz),
        ItuRegion::Region3 => find_region3_segment(freq_hz),
    }
}

/// Conservative occupied-bandwidth estimates used for policy checks.
pub fn occupied_bandwidth_hz(mode: &str) -> Option<u32> {
    match mode {
        "BPSK31" => Some(100),
        "BPSK63" => Some(150),
        "BPSK100" => Some(200),
        "BPSK250" => Some(500),
        "BPSK250-RRC" => Some(350),
        "QPSK125" => Some(350),
        "QPSK250" => Some(700),
        "QPSK500" => Some(1400),
        "QPSK500-RRC" => Some(675),
        "QPSK1000" => Some(2800),
        "QPSK1000-HF" | "QPSK1000-HF-RRC" => Some(1350),
        "QPSK2000" => Some(5600),
        "QPSK9600" | "QPSK9600-RRC" => Some(12000),
        "8PSK500" => Some(1500),
        "8PSK500-RRC" => Some(675),
        "8PSK1000" | "8PSK1000-HF" => Some(3000),
        "8PSK1000-RRC" | "8PSK1000-HF-RRC" => Some(1350),
        "8PSK2000" => Some(6000),
        "8PSK2000-RRC" => Some(2700),
        "8PSK9600" | "8PSK9600-RRC" => Some(12000),
        "64QAM500" => Some(1600),
        "64QAM1000" => Some(3200),
        "64QAM2000-RRC" => Some(6400),
        "OFDM16" => Some(2200),
        "OFDM52" => Some(3200),
        "SCFDMA16" => Some(2200),
        "SCFDMA52" | "SCFDMA52-64QAM-P4" => Some(3200),
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

#[derive(Clone, Copy)]
struct HamBand {
    min_hz: u64,
    max_hz: u64,
    max_bw_hz: u32,
}

fn find_ham_iaru_band(freq_hz: u64) -> Option<HamBand> {
    const BANDS: [HamBand; 9] = [
        HamBand {
            min_hz: 1_800_000,
            max_hz: 2_000_000,
            max_bw_hz: 2_700,
        }, // 160m
        HamBand {
            min_hz: 3_500_000,
            max_hz: 4_000_000,
            max_bw_hz: 2_700,
        }, // 80m
        HamBand {
            min_hz: 7_000_000,
            max_hz: 7_300_000,
            max_bw_hz: 2_700,
        }, // 40m
        HamBand {
            min_hz: 10_100_000,
            max_hz: 10_150_000,
            max_bw_hz: 500,
        }, // 30m (narrow data)
        HamBand {
            min_hz: 14_000_000,
            max_hz: 14_350_000,
            max_bw_hz: 2_700,
        }, // 20m
        HamBand {
            min_hz: 18_068_000,
            max_hz: 18_168_000,
            max_bw_hz: 2_700,
        }, // 17m
        HamBand {
            min_hz: 21_000_000,
            max_hz: 21_450_000,
            max_bw_hz: 2_700,
        }, // 15m
        HamBand {
            min_hz: 24_890_000,
            max_hz: 24_990_000,
            max_bw_hz: 2_700,
        }, // 12m
        HamBand {
            min_hz: 28_000_000,
            max_hz: 29_700_000,
            max_bw_hz: 2_700,
        }, // 10m
    ];

    BANDS
        .iter()
        .copied()
        .find(|band| (band.min_hz..=band.max_hz).contains(&freq_hz))
}

fn shared_base_digital_segments() -> &'static [DigitalSegment; 9] {
    &[
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
        }, // 20m — base (Region 1 narrow)
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
    ]
}

fn find_ham_iaru_segment(freq_hz: u64) -> Option<DigitalSegment> {
    shared_base_digital_segments()
        .iter()
        .copied()
        .find(|s| (s.min_hz..=s.max_hz).contains(&freq_hz))
}

/// IARU Region 1 (Europe, Africa, Middle East) digital segments.
fn find_region1_band(freq_hz: u64) -> Option<HamBand> {
    // Region 1 uses standard HF allocation; reuse base implementation.
    find_ham_iaru_band(freq_hz)
}

fn find_region1_segment(freq_hz: u64) -> Option<DigitalSegment> {
    // Region 1 uses the shared base table (narrower 20m: 14.070-14.112).
    shared_base_digital_segments()
        .iter()
        .copied()
        .find(|s| (s.min_hz..=s.max_hz).contains(&freq_hz))
}

/// IARU Region 2 (Americas) digital segments — generally wider digital allocations.
fn find_region2_band(freq_hz: u64) -> Option<HamBand> {
    // Region 2 generally shares the same band allocations; reuse base.
    find_ham_iaru_band(freq_hz)
}

fn find_region2_segment(freq_hz: u64) -> Option<DigitalSegment> {
    // Region 2 uses shared base except 20m segment is wider (14.070-14.150).
    let freq = freq_hz;
    if (14_070_000..=14_150_000).contains(&freq) {
        return Some(DigitalSegment {
            min_hz: 14_070_000,
            max_hz: 14_150_000,
            max_bw_hz: 2_700,
        });
    }
    shared_base_digital_segments()
        .iter()
        .copied()
        .find(|s| (s.min_hz..=s.max_hz).contains(&freq_hz))
}

/// IARU Region 3 (Asia-Pacific) digital segments — varies by administration.
/// For simplicity, use Region 1 allocations as a reasonable default.
fn find_region3_band(freq_hz: u64) -> Option<HamBand> {
    find_ham_iaru_band(freq_hz)
}

fn find_region3_segment(freq_hz: u64) -> Option<DigitalSegment> {
    // Region 3 allocations vary by country; default to Region 1.
    find_region1_segment(freq_hz)
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
            Err(BandplanError::SegmentViolation { .. })
        ));
    }

    #[test]
    fn ham_iaru_allows_non_segment_when_conventions_disabled() {
        let mut policy = BandplanPolicy::default();
        policy.enforce_segment_conventions = false;
        assert!(policy.validate_frequency(14_200_000, "BPSK250").is_ok());
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

    #[test]
    fn region1_accepts_20m_digital_lower_edge() {
        let mut policy = BandplanPolicy::default();
        policy.mode = BandplanMode::HamIaruRegion1;
        assert!(policy.validate_frequency(14_070_000, "BPSK250").is_ok());
    }

    #[test]
    fn region1_accepts_20m_digital_upper_edge() {
        let mut policy = BandplanPolicy::default();
        policy.mode = BandplanMode::HamIaruRegion1;
        assert!(policy.validate_frequency(14_112_000, "BPSK250").is_ok());
    }

    #[test]
    fn region1_rejects_20m_outside_segment_upper() {
        let mut policy = BandplanPolicy::default();
        policy.mode = BandplanMode::HamIaruRegion1;
        assert!(matches!(
            policy.validate_frequency(14_120_000, "BPSK250"),
            Err(BandplanError::SegmentViolation { .. })
        ));
    }

    #[test]
    fn region2_accepts_wider_20m_digital_segment() {
        let mut policy = BandplanPolicy::default();
        policy.mode = BandplanMode::HamIaruRegion2;
        // Region 2 allows up to 14.150 MHz, whereas Region 1 stops at 14.112
        assert!(policy.validate_frequency(14_140_000, "BPSK250").is_ok());
    }

    #[test]
    fn region2_rejects_outside_20m_segment_upper() {
        let mut policy = BandplanPolicy::default();
        policy.mode = BandplanMode::HamIaruRegion2;
        assert!(matches!(
            policy.validate_frequency(14_160_000, "BPSK250"),
            Err(BandplanError::SegmentViolation { .. })
        ));
    }

    #[test]
    fn region3_defaults_to_region1_allocations() {
        let mut policy = BandplanPolicy::default();
        policy.mode = BandplanMode::HamIaruRegion3;
        // Region 3 defaults to Region 1 allocations
        assert!(policy.validate_frequency(14_112_000, "BPSK250").is_ok());
        assert!(matches!(
            policy.validate_frequency(14_120_000, "BPSK250"),
            Err(BandplanError::SegmentViolation { .. })
        ));
    }

    #[test]
    fn region3_validation_emits_proxy_warning() {
        let mut policy = BandplanPolicy::default();
        policy.mode = BandplanMode::HamIaruRegion3;

        let warnings = policy
            .validate_frequency_with_warnings(14_112_000, "BPSK250")
            .expect("region3 proxy validation should still succeed");

        assert_eq!(warnings, vec![BandplanWarning::Region3UsesRegion1Proxy]);
    }

    #[test]
    fn region2_accepts_20m_upper_edge() {
        let mut policy = BandplanPolicy::default();
        policy.mode = BandplanMode::HamIaruRegion2;
        // Verify exact upper boundary: 14.150 MHz is the Region 2 limit for 20m digital
        assert!(policy.validate_frequency(14_150_000, "BPSK250").is_ok());
    }

    #[test]
    #[allow(deprecated)]
    fn parse_region_mode_strings() {
        assert_eq!(
            "ham-iaru".parse::<BandplanMode>(),
            Ok(BandplanMode::HamIaru)
        );
        assert_eq!(
            "ham-iaru-r1".parse::<BandplanMode>(),
            Ok(BandplanMode::HamIaruRegion1)
        );
        assert_eq!(
            "ham-iaru-region1".parse::<BandplanMode>(),
            Ok(BandplanMode::HamIaruRegion1)
        );
        assert_eq!(
            "ham-iaru-region2".parse::<BandplanMode>(),
            Ok(BandplanMode::HamIaruRegion2)
        );
        assert_eq!(
            "ham-iaru-r3".parse::<BandplanMode>(),
            Ok(BandplanMode::HamIaruRegion3)
        );
        assert_eq!(
            "ham-iaru-region3".parse::<BandplanMode>(),
            Ok(BandplanMode::HamIaruRegion3)
        );
    }

    #[test]
    fn parse_region_mode_strings_case_insensitive() {
        assert_eq!(
            "HAM-IARU-R1".parse::<BandplanMode>(),
            Ok(BandplanMode::HamIaruRegion1)
        );
        assert_eq!(
            "Ham-IARU-Region2".parse::<BandplanMode>(),
            Ok(BandplanMode::HamIaruRegion2)
        );
        assert_eq!(
            "HAM-IARU-REGION1".parse::<BandplanMode>(),
            Ok(BandplanMode::HamIaruRegion1)
        );
        assert_eq!(
            "ham-iaru-region3".parse::<BandplanMode>(),
            Ok(BandplanMode::HamIaruRegion3)
        );
    }

    #[test]
    fn default_policy_uses_region1_mode() {
        let policy = BandplanPolicy::default();
        assert_eq!(policy.mode, BandplanMode::HamIaruRegion1);
    }

    #[test]
    fn occupied_bandwidth_covers_active_rrc_and_dense_pilot_modes() {
        assert_eq!(occupied_bandwidth_hz("BPSK250-RRC"), Some(350));
        assert_eq!(occupied_bandwidth_hz("QPSK500-RRC"), Some(675));
        assert_eq!(occupied_bandwidth_hz("QPSK1000-HF-RRC"), Some(1350));
        assert_eq!(occupied_bandwidth_hz("8PSK500-RRC"), Some(675));
        assert_eq!(occupied_bandwidth_hz("8PSK1000-RRC"), Some(1350));
        assert_eq!(occupied_bandwidth_hz("8PSK1000-HF-RRC"), Some(1350));
        assert_eq!(occupied_bandwidth_hz("8PSK2000-RRC"), Some(2700));
        assert_eq!(occupied_bandwidth_hz("SCFDMA52-64QAM-P4"), Some(3200));
    }

    #[test]
    fn region1_accepts_rrc_mode_when_bandwidth_is_known() {
        let policy = BandplanPolicy::default();
        assert!(policy.validate_frequency(14_074_000, "QPSK500-RRC").is_ok());
    }
}
