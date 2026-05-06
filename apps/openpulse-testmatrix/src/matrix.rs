use serde::{Deserialize, Serialize};

use openpulse_core::compression::CompressionAlgorithm;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tier {
    Quick,
    Full,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UseCase {
    RawModem,
    AdaptiveHpx500,
    AdaptiveHpx2300,
    Ardop,
    Kiss,
    B2f,
}

impl UseCase {
    pub fn label(&self) -> &'static str {
        match self {
            Self::RawModem => "raw_modem",
            Self::AdaptiveHpx500 => "adaptive_hpx500",
            Self::AdaptiveHpx2300 => "adaptive_hpx2300",
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
    WattersonModerateF1,
    WattersonPoorF1,
    GilbertElliottLight,
    GilbertElliottModerate,
    GilbertElliottHeavy,
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
            Self::WattersonModerateF1 => "watterson_moderate_f1".into(),
            Self::WattersonPoorF1 => "watterson_poor_f1".into(),
            Self::GilbertElliottLight => "ge_light".into(),
            Self::GilbertElliottModerate => "ge_moderate".into(),
            Self::GilbertElliottHeavy => "ge_heavy".into(),
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestCase {
    pub use_case: UseCase,
    pub mode: String,
    pub fec: bool,
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
            if self.fec { "fec" } else { "raw" },
            match self.compression {
                CompressionAlgorithm::None => "nocomp",
                CompressionAlgorithm::Lz4 => "lz4",
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
    pub ber: Option<f64>,
    pub bytes_rx: usize,
    pub duration_ms: u64,
    pub note: Option<String>,
}
