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
    AdaptiveHpxOfdmHf,
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
            Self::AdaptiveHpxOfdmHf => "adaptive_hpx_ofdm_hf",
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
    fn snr_slug(snr_db: f32) -> String {
        // Keep enough precision to avoid collisions for nearby sweep points.
        format!("{snr_db:.2}").replace('.', "p")
    }

    pub fn label(&self) -> String {
        match self {
            Self::Clean => "clean".into(),
            Self::Awgn { snr_db, .. } => format!("awgn_{snr_db:.0}dB"),
            Self::WattersonGoodF1 => "watterson_good_f1".into(),
            Self::WattersonGoodF2 => "watterson_good_f2".into(),
            Self::WattersonGoodF1Snr { snr_db, .. } => {
                format!("watterson_good_f1_{}dB", Self::snr_slug(*snr_db))
            }
            Self::WattersonGoodF2Snr { snr_db, .. } => {
                format!("watterson_good_f2_{}dB", Self::snr_slug(*snr_db))
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
        FecMode::LdpcHighRate => "ldpc_hr",
        FecMode::Turbo => "turbo",
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
