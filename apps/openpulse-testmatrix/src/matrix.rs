use openpulse_core::compression::CompressionAlgorithm;
use openpulse_core::fec::FecMode;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tier {
    Quick,
    Full,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UseCase {
    RawModem,
    AdaptiveHpx500,
    AdaptiveHpxHf,
    AdaptiveHpxWideband,
    Ardop,
    Kiss,
    B2f,
}

impl UseCase {
    pub fn label(&self) -> &'static str {
        match self {
            Self::RawModem => "raw_modem",
            Self::AdaptiveHpx500 => "adaptive_hpx500",
            Self::AdaptiveHpxHf => "adaptive_hpx_hf",
            Self::AdaptiveHpxWideband => "adaptive_hpx_wideband",
            Self::Ardop => "ardop",
            Self::Kiss => "kiss",
            Self::B2f => "b2f",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ChannelSpec {
    Clean,
    Awgn { snr_db: f32, seed: u64 },
    WattersonGoodF1,
    WattersonGoodF2,
    WattersonGoodF1Snr { snr_db: f32, seed: u64 },
    WattersonGoodF2Snr { snr_db: f32, seed: u64 },
    WattersonModerateF1,
    WattersonPoorF1,
    WattersonExtreme,
    GilbertElliottLight,
    GilbertElliottModerate,
    GilbertElliottHeavy,
    GilbertElliottSevere,
    QrnLight,
    QrmTone,
    QsbSlow,
    ChirpSlow,
}

impl ChannelSpec {
    pub fn label(&self) -> String {
        match self {
            Self::Clean => "clean".into(),
            Self::Awgn { snr_db, .. } => format!("awgn_{snr_db:.0}dB"),
            Self::WattersonGoodF1 => "watterson_good_f1".into(),
            Self::WattersonGoodF2 => "watterson_good_f2".into(),
            Self::WattersonGoodF1Snr { snr_db, .. } => {
                format!("watterson_good_f1_{snr_db:.0}dB")
            }
            Self::WattersonGoodF2Snr { snr_db, .. } => {
                format!("watterson_good_f2_{snr_db:.0}dB")
            }
            Self::WattersonModerateF1 => "watterson_moderate_f1".into(),
            Self::WattersonPoorF1 => "watterson_poor_f1".into(),
            Self::WattersonExtreme => "watterson_extreme".into(),
            Self::GilbertElliottLight => "ge_light".into(),
            Self::GilbertElliottModerate => "ge_moderate".into(),
            Self::GilbertElliottHeavy => "ge_heavy".into(),
            Self::GilbertElliottSevere => "ge_severe".into(),
            Self::QrnLight => "qrn_light".into(),
            Self::QrmTone => "qrm_tone".into(),
            Self::QsbSlow => "qsb_slow".into(),
            Self::ChirpSlow => "chirp_slow".into(),
        }
    }

    pub fn is_awgn_family(&self) -> bool {
        matches!(self, Self::Clean | Self::Awgn { .. })
    }
}

/// Approximate effective throughput for a mode in bits per second (payload bits, no overhead).
#[allow(dead_code)]
pub fn mode_effective_bps(
    _mode: &str,
    _fec_mode: FecMode,
    payload_len: usize,
    duration_ms: u64,
) -> Option<f64> {
    if duration_ms == 0 {
        return None;
    }
    let payload_bits = payload_len as f64 * 8.0;
    let duration_s = duration_ms as f64 / 1000.0;
    Some(payload_bits / duration_s)
}

/// Theoretical nominal baud rate for a mode string (for reference column in reports).
#[allow(dead_code)]
pub fn mode_nominal_baud(mode: &str) -> Option<f32> {
    let base = mode.trim_end_matches("-RRC").trim_end_matches("-HF");
    let digits: String = base.chars().skip_while(|c| !c.is_ascii_digit()).collect();
    match digits.as_str() {
        "31" => Some(31.25),
        "63" => Some(62.5),
        "100" => Some(100.0),
        "125" => Some(125.0),
        "250" => Some(250.0),
        "500" => Some(500.0),
        "1000" => Some(1000.0),
        "2000" => Some(2000.0),
        "9600" => Some(9600.0),
        "16" | "52" => None, // OFDM/SCFDMA — not baud-based
        _ => None,
    }
}

pub fn fec_label(fec_mode: FecMode) -> &'static str {
    match fec_mode {
        FecMode::None => "none",
        FecMode::Rs => "rs",
        FecMode::RsInterleaved => "rs_il",
        FecMode::Concatenated => "concat",
        FecMode::ShortRs => "short_rs",
        FecMode::RsStrong => "rs_strong",
        FecMode::SoftConcatenated => "soft_concat",
        FecMode::Ldpc => "ldpc",
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestCase {
    pub use_case: UseCase,
    pub mode: String,
    pub fec_mode: FecMode,
    pub compression: CompressionAlgorithm,
    pub channel: ChannelSpec,
    pub payload_len: usize,
    pub tier: Tier,
}

impl TestCase {
    pub fn id(&self) -> String {
        format!(
            "{}/{}/{}/{}/{}/{}B",
            self.use_case.label(),
            self.mode,
            fec_label(self.fec_mode),
            match self.compression {
                CompressionAlgorithm::None => "nocomp",
                CompressionAlgorithm::Lz4 => "lz4",
                CompressionAlgorithm::Zstd(_) => "zstd",
            },
            self.channel.label(),
            self.payload_len,
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    pub case: TestCase,
    pub passed: bool,
    /// True when the case was intentionally skipped (e.g. unsupported FEC mode).
    /// Skipped cases are excluded from pass-rate aggregation.
    pub skipped: bool,
    pub ber: Option<f64>,
    pub bytes_rx: usize,
    pub duration_ms: u64,
    /// Effective payload throughput in bits/second (payload bits / wall time).
    pub effective_bps: Option<f64>,
    pub note: Option<String>,
}
