use openpulse_core::compression::{CompressionAlgorithm, ZSTD_DICT_ID};
use openpulse_core::fec::FecMode;

use crate::channels::channel_suite;
use crate::matrix::{ChannelSpec, TestCase, Tier, UseCase};

// ── Mode constants ─────────────────────────────────────────────────────────────

const MULTICARRIER_MODES: &[&str] = &["OFDM16", "OFDM52", "SCFDMA16", "SCFDMA52"];

const NARROWBAND_MODES: &[&str] = &["QPSK2000", "QPSK2000-RRC", "8PSK2000", "8PSK2000-RRC"];

// All HF modes (≤2700 Hz BW, 8 kHz audio, suitable for standard test matrix).
const HF_FAST_MODES: &[&str] = &[
    "BPSK250",
    "BPSK250-RRC",
    "QPSK125",
    "QPSK250",
    "QPSK500",
    "QPSK1000-HF",
    "QPSK500-RRC",
    "QPSK1000-RRC",
    "8PSK500",
    "8PSK1000-HF",
    "8PSK500-RRC",
    "8PSK1000-RRC",
];

// Modes that are slow enough that large case counts are impractical.
const HF_SLOW_MODES: &[&str] = &["BPSK31", "BPSK63", "BPSK100"];

// All data FEC modes applicable to raw modem (excludes ShortRs which is ACK-only).
const DATA_FEC_MODES: &[FecMode] = &[
    FecMode::None,
    FecMode::Rs,
    FecMode::RsInterleaved,
    FecMode::Concatenated,
    FecMode::RsStrong,
    FecMode::SoftConcatenated,
];

// ── Case builder ──────────────────────────────────────────────────────────────

/// Build all test cases for the given tier.
pub fn build_cases(tier: Tier) -> Vec<TestCase> {
    let channels = channel_suite(tier);
    let awgn_channels: Vec<_> = channels
        .iter()
        .filter(|c| c.is_awgn_family())
        .cloned()
        .collect();
    let prop_channels: Vec<_> = channels
        .iter()
        .filter(|c| !c.is_awgn_family())
        .cloned()
        .collect();
    let mut cases = Vec::new();

    // ── 1. Smoke: every mode × clean × no FEC × no compression × 32 bytes ────────
    let all_hf_modes: Vec<&str> = HF_SLOW_MODES
        .iter()
        .chain(HF_FAST_MODES.iter())
        .chain(MULTICARRIER_MODES.iter())
        .copied()
        .collect();

    for mode in &all_hf_modes {
        cases.push(raw_case(
            mode,
            FecMode::None,
            CompressionAlgorithm::None,
            ChannelSpec::Clean,
            32,
            tier,
        ));
    }

    // FSK4-ACK smoke: 5-byte payload, no FEC, no compression, clean + awgn 20 dB
    for channel in &[
        ChannelSpec::Clean,
        ChannelSpec::Awgn {
            snr_db: 20.0,
            seed: 42,
        },
    ] {
        cases.push(raw_case(
            "FSK4-ACK",
            FecMode::None,
            CompressionAlgorithm::None,
            channel.clone(),
            5,
            tier,
        ));
    }

    // ── 2. AWGN SNR sweep: fast HF modes × all AWGN channels × {None, Rs, RsInterleaved} ×
    //       {128, 512} bytes ───────────────────────────────────────────────────────
    for mode in HF_FAST_MODES {
        for channel in &awgn_channels {
            for &fec in &[FecMode::None, FecMode::Rs, FecMode::RsInterleaved] {
                for &payload_len in &[128usize, 512] {
                    cases.push(raw_case(
                        mode,
                        fec,
                        CompressionAlgorithm::None,
                        channel.clone(),
                        payload_len,
                        tier,
                    ));
                }
            }
        }
    }

    // ── 3. All FEC modes × key HF modes × awgn 10 dB and 20 dB × 128 bytes ────────
    //       (ensures every FecMode has coverage)
    let key_modes = &["BPSK250", "QPSK500", "8PSK500"];
    let key_awgn = vec![
        ChannelSpec::Awgn {
            snr_db: 10.0,
            seed: 42,
        },
        ChannelSpec::Awgn {
            snr_db: 20.0,
            seed: 42,
        },
    ];
    for mode in key_modes {
        for channel in &key_awgn {
            for &fec in DATA_FEC_MODES {
                cases.push(raw_case(
                    mode,
                    fec,
                    CompressionAlgorithm::None,
                    channel.clone(),
                    128,
                    tier,
                ));
            }
        }
    }

    // ── 4. Compression matrix: HF fast modes × clean × all FEC × all compression × 128 bytes
    for mode in HF_FAST_MODES {
        for &fec in &[FecMode::None, FecMode::Rs] {
            for compression in [
                CompressionAlgorithm::None,
                CompressionAlgorithm::Lz4,
                CompressionAlgorithm::Zstd(ZSTD_DICT_ID),
            ] {
                cases.push(raw_case(
                    mode,
                    fec,
                    compression,
                    ChannelSpec::Clean,
                    128,
                    tier,
                ));
            }
        }
    }

    // ── 5. Multi-carrier modes × AWGN sweep × {None, Rs} × 128 bytes ────────────
    for mode in MULTICARRIER_MODES {
        for channel in &awgn_channels {
            for &fec in &[FecMode::None, FecMode::Rs] {
                cases.push(raw_case(
                    mode,
                    fec,
                    CompressionAlgorithm::None,
                    channel.clone(),
                    128,
                    tier,
                ));
            }
        }
    }

    // ── 6. Adaptive profiles (clean + AWGN channels) ──────────────────────────────
    for channel in &awgn_channels {
        cases.push(adaptive_case(
            UseCase::AdaptiveHpx500,
            "HPX500",
            channel.clone(),
            64,
            tier,
        ));
        cases.push(adaptive_case(
            UseCase::AdaptiveHpxHf,
            "HPX_HF",
            channel.clone(),
            64,
            tier,
        ));
        cases.push(adaptive_case(
            UseCase::AdaptiveHpxWideband,
            "HPX_WIDEBAND",
            channel.clone(),
            64,
            tier,
        ));
    }

    // ── 7. Protocol loopbacks (clean only) ───────────────────────────────────────
    cases.push(proto_case(
        UseCase::Ardop,
        "BPSK250",
        ChannelSpec::Clean,
        64,
        tier,
    ));
    cases.push(proto_case(
        UseCase::Kiss,
        "BPSK250",
        ChannelSpec::Clean,
        64,
        tier,
    ));
    for channel in &awgn_channels {
        cases.push(proto_case(
            UseCase::B2f,
            "BPSK250",
            channel.clone(),
            64,
            tier,
        ));
    }

    // ── 8. Full-tier additions ───────────────────────────────────────────────────
    if tier == Tier::Full {
        // Slow modes × full AWGN sweep × all FEC × {32, 128} bytes
        for mode in HF_SLOW_MODES {
            for channel in &awgn_channels {
                for &fec in DATA_FEC_MODES {
                    for &payload_len in &[32usize, 128] {
                        cases.push(raw_case(
                            mode,
                            fec,
                            CompressionAlgorithm::None,
                            channel.clone(),
                            payload_len,
                            tier,
                        ));
                    }
                }
            }
        }

        // HF fast modes × all propagation channels × all FEC × {32, 128, 512} bytes
        for mode in HF_FAST_MODES {
            for channel in &prop_channels {
                for &fec in DATA_FEC_MODES {
                    for &payload_len in &[32usize, 128, 512] {
                        cases.push(raw_case(
                            mode,
                            fec,
                            CompressionAlgorithm::None,
                            channel.clone(),
                            payload_len,
                            tier,
                        ));
                    }
                }
            }
        }

        // Multi-carrier × all propagation channels × {None, Rs, SoftConcatenated}
        for mode in MULTICARRIER_MODES {
            for channel in &prop_channels {
                for &fec in &[FecMode::None, FecMode::Rs, FecMode::SoftConcatenated] {
                    cases.push(raw_case(
                        mode,
                        fec,
                        CompressionAlgorithm::None,
                        channel.clone(),
                        128,
                        tier,
                    ));
                }
            }
        }

        // Narrowband modes (8 kHz audio, PMR) × clean + AWGN × key FEC
        for mode in NARROWBAND_MODES {
            for channel in &awgn_channels {
                for &fec in &[FecMode::None, FecMode::Rs] {
                    cases.push(raw_case(
                        mode,
                        fec,
                        CompressionAlgorithm::None,
                        channel.clone(),
                        128,
                        tier,
                    ));
                }
            }
        }

        // Large payload (4096 bytes): key modes × key FEC × clean + awgn20
        let large_channels = vec![
            ChannelSpec::Clean,
            ChannelSpec::Awgn {
                snr_db: 20.0,
                seed: 42,
            },
        ];
        for mode in &["BPSK250", "QPSK500", "8PSK500", "OFDM52", "SCFDMA52"] {
            for channel in &large_channels {
                for &fec in &[FecMode::None, FecMode::Rs, FecMode::SoftConcatenated] {
                    cases.push(raw_case(
                        mode,
                        fec,
                        CompressionAlgorithm::None,
                        channel.clone(),
                        4096,
                        tier,
                    ));
                }
            }
        }
    }

    // Deduplicate (same case may appear from multiple generators)
    cases.sort_by_key(|c| c.id());
    cases.dedup_by_key(|c| c.id());
    cases
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn raw_case(
    mode: &str,
    fec_mode: FecMode,
    compression: CompressionAlgorithm,
    channel: ChannelSpec,
    payload_len: usize,
    tier: Tier,
) -> TestCase {
    TestCase {
        use_case: UseCase::RawModem,
        mode: mode.to_string(),
        fec_mode,
        compression,
        channel,
        payload_len,
        tier,
    }
}

fn adaptive_case(
    use_case: UseCase,
    mode: &str,
    channel: ChannelSpec,
    payload_len: usize,
    tier: Tier,
) -> TestCase {
    TestCase {
        use_case,
        mode: mode.to_string(),
        fec_mode: FecMode::None,
        compression: CompressionAlgorithm::None,
        channel,
        payload_len,
        tier,
    }
}

fn proto_case(
    use_case: UseCase,
    mode: &str,
    channel: ChannelSpec,
    payload_len: usize,
    tier: Tier,
) -> TestCase {
    TestCase {
        use_case,
        mode: mode.to_string(),
        fec_mode: FecMode::None,
        compression: CompressionAlgorithm::None,
        channel,
        payload_len,
        tier,
    }
}
