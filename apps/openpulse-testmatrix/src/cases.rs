use openpulse_core::compression::{CompressionAlgorithm, ZSTD_DICT_ID};
use openpulse_core::fec::FecMode;

use crate::channels::channel_suite;
use crate::matrix::{ChannelSpec, TestCase, Tier, UseCase};

// ── Mode constants ─────────────────────────────────────────────────────────────

const MULTICARRIER_MODES: &[&str] = &["OFDM16", "OFDM52", "SCFDMA16", "SCFDMA52"];

/// Padded multicarrier modes that round-trip the length-prefixed raw frame (`FecMode::None`) but
/// NOT the hard 255-byte-block RS framing (`Rs`/`RsInterleaved`): the demodulator emits a padded
/// byte count that RS reads as a corrupted block, so RS decode fails at block 0 even on a clean
/// channel (a known limitation — these modes run FEC-protected with soft/concatenated coding in
/// practice). They're still exercised in the raw-modem matrix with `None` here, with `Rs` via the
/// OFDM-HOM section, and FEC-protected elsewhere — so this is a documented exclusion, not a silent
/// one (the coverage regression test still requires the mode to appear in a raw-modem case).
const OFDM_RAW_FRAMING_ONLY: &[&str] = &["OFDM52"];

/// True when `mode`'s padded framing can't round-trip `fec` because it breaks the hard 255-byte
/// RS block boundary (see [`OFDM_RAW_FRAMING_ONLY`]). Only `FecMode::None` survives, so every
/// case-gen site that pairs a raw-framing-only mode with FEC must skip the RS-family modes —
/// otherwise they surface as spurious RS-decode failures even on a clean channel.
fn raw_framing_excludes(mode: &str, fec: FecMode) -> bool {
    OFDM_RAW_FRAMING_ONLY.contains(&mode) && fec != FecMode::None
}

/// SC-FDMA higher-order modulation modes: the full-width SCFDMA52 family plus the
/// narrowband SCFDMA26 fallback rungs (half width, ~+3 dB per-SC SNR).  All require
/// higher minimum SNR than QPSK; thresholds set in mode_min_snr_db().
const SCFDMA_HOM_MODES: &[&str] = &[
    "SCFDMA52-8PSK",
    "SCFDMA52-16QAM",
    "SCFDMA52-32QAM",
    "SCFDMA52-64QAM",
    "SCFDMA52-64QAM-P4",
    "SCFDMA26-8PSK",
    "SCFDMA26-16QAM",
    "SCFDMA26-32QAM",
];

/// OFDM higher-order modulation modes (OFDM52 with denser constellations) — the
/// high-throughput / high-reliability HF path (per-SC equalization handles
/// frequency-selective fading without SC-FDMA's de-spread noise enhancement).
/// Same min-SNR profile as the SC-FDMA HOM modes.
const OFDM_HOM_MODES: &[&str] = &[
    "OFDM52-8PSK",
    "OFDM52-16QAM",
    "OFDM52-32QAM",
    "OFDM52-64QAM",
];

const QAM64_MODES: &[&str] = &["64QAM500", "64QAM1000", "64QAM2000-RRC"];

/// Pilot-framed single-carrier family — a representative slice (the 500-baud
/// ladder, both pulse shapes, plus a 1000-baud throughput rung). Pilot-aided
/// carrier recovery; soft-capable (so they run under LDPC too).
const PILOT_MODES: &[&str] = &[
    "PILOT-QPSK500",
    "PILOT-8PSK500",
    "PILOT-16QAM500",
    "PILOT-32APSK500",
    "PILOT-QPSK500-RRC",
    "PILOT-16QAM500-RRC",
    "PILOT-16QAM1000",
];

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
    "QPSK1000-HF-RRC",
    // UHF/VHF narrowband: RRC variant works at all payload sizes at 4 sps (8 kHz / 2000 baud).
    "QPSK2000-RRC",
    "8PSK500",
    "8PSK1000-HF",
    "8PSK500-RRC",
    "8PSK1000-RRC",
    "8PSK1000-HF-RRC",
];

/// Modes that are smoke-tested (clean/32B) only.
///
/// - Plain QPSK2000 (rectangular pulse) has timing drift at >32-byte payloads at 4 samples/symbol.
/// - Plain QPSK1000 / 8PSK1000 decode cleanly but are AWGN-marginal (fail the quick sweep at
///   10–20 dB without FEC); their `-HF` / `-RRC` siblings carry the robust AWGN coverage.
/// - 8PSK2000-RRC is SNR-marginal in the quick AWGN sweep.
///
/// Included for feature-presence tracking, but not in the AWGN sweep.
const SMOKE_ONLY_MODES: &[&str] = &["QPSK2000", "QPSK1000", "8PSK1000", "8PSK2000-RRC"];

/// Registered modes with a known decode limitation at the 8 kHz HF sample rate, kept in the
/// codebase but not exercised (they cannot pass even a clean smoke case as-is).
///
/// 8PSK2000 (plain, rectangular pulse) closes the eye at 4 samples/symbol — use 8PSK2000-RRC
/// for HF. Listed here so it is an explicit, tracked limitation rather than a silent omission
/// (see the coverage regression test). Revisit post-v1.0.
pub const KNOWN_LIMITATION_MODES: &[&str] = &["8PSK2000"];

/// Wideband modes deferred to a post-v1.0 release.
///
/// 9600-baud modes need a channel wider than the 3 kHz HF SSB passband (10 m HF, UHF, VHF)
/// and ≥ 38.4 kHz audio Fs (≥ 4 samples/symbol), so they cannot run on the 8 kHz HF path.
/// The mode code stays in the plugins; this list documents the deferral explicitly so the
/// modes are NOT silently excluded (see the coverage regression test) and gives a single
/// place to drop them once a wider-channel transport exists. Roadmap: V1.x wider-than-3 kHz
/// channel support.
pub const WIDEBAND_POST_V1_MODES: &[&str] =
    &["QPSK9600", "QPSK9600-RRC", "8PSK9600", "8PSK9600-RRC"];

/// SC-FDMA PAPR demonstrators: registered by `scfdma-plugin`, deliberately in NO adaptive profile,
/// and therefore not swept by the channel matrix.
///
/// - `SCFDMA52-LP`: localized block-pilot layout with a single-tap (flat) channel estimate. It assumes
///   flat gain/phase, near-zero residual timing offset, and no passband tilt; under frequency
///   selectivity or a ±1-sample sync error it can SILENTLY mis-decode. Sweeping it over the matrix's
///   propagation channels would assert behaviour the mode does not claim.
/// - `SCFDMA52-P2`: SCFDMA52 with PN-phase pilots — identical geometry, rate and channel estimator,
///   ~1.6 dB lower envelope PAPR. A versioned waveform experiment, not a rung.
///
/// Both carry plugin-level coverage (PAPR bound + clean round-trip) in `plugins/scfdma/src/lib.rs`.
/// Listed here so they are explicitly tracked rather than silently excluded (see the coverage
/// regression test); promote `SCFDMA52-P2` into `MULTICARRIER_MODES` if it ever enters a profile.
pub const DEMONSTRATOR_MODES: &[&str] = &["SCFDMA52-LP", "SCFDMA52-P2"];

/// Higher-rate pilot-framed variants registered by `pilot-plugin` but not yet in the quick
/// matrix. The matrix covers a representative baseline subset (`PILOT_MODES`: the 500-baud
/// family plus PILOT-16QAM1000); the 1000/2000-baud variants and the remaining 500-RRC
/// variants exist in the plugin but are not yet swept. Listed here so they are explicitly
/// tracked rather than silently excluded (see the coverage regression test); promote into
/// `PILOT_MODES` as quick-matrix pilot coverage is broadened. Roadmap: V1.x pilot ladder.
pub const PILOT_POST_V1_MODES: &[&str] = &[
    "PILOT-8PSK500-RRC",
    "PILOT-32APSK500-RRC",
    "PILOT-QPSK1000",
    "PILOT-8PSK1000",
    "PILOT-32APSK1000",
    "PILOT-QPSK1000-RRC",
    "PILOT-8PSK1000-RRC",
    "PILOT-16QAM1000-RRC",
    "PILOT-32APSK1000-RRC",
    "PILOT-QPSK2000-RRC",
    "PILOT-8PSK2000-RRC",
    "PILOT-16QAM2000-RRC",
    "PILOT-32APSK2000-RRC",
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

/// Minimum AWGN SNR (dB) at which a mode is reliably testable.
///
/// - 8PSK without RRC needs ≥ 15 dB (insufficient margin at 10 dB).
/// - OFDM52 / SCFDMA52 are wideband: ICI raises the effective noise floor.
/// - 64QAM requires ≥ 20 dB (6 bits/symbol; very sensitive to noise).
pub fn mode_min_snr_db(mode: &str) -> f32 {
    // 64QAM at 2000 baud / 4 sps needs ~26 dB; it fails at 20 dB without FEC.
    if mode == "64QAM2000-RRC" {
        return 26.0;
    }
    if mode.starts_with("64QAM") {
        return 20.0;
    }
    // Pilot family: per-constellation floors (same as the hpx_pilot profile), baud
    // and pulse-shape independent (the matched filter gives the same Es/N0).
    if mode.starts_with("PILOT-") {
        if mode.contains("32APSK") {
            return 23.0;
        } else if mode.contains("16QAM") {
            return 17.0;
        } else if mode.contains("8PSK") {
            return 12.0;
        } else if mode.contains("QPSK") {
            return 6.0;
        }
    }
    match mode {
        "8PSK500" | "8PSK1000-HF" | "OFDM16" | "OFDM52" | "SCFDMA52" => 15.0,
        "SCFDMA52-8PSK" | "OFDM52-8PSK" => 15.0,
        "SCFDMA52-16QAM" | "OFDM52-16QAM" => 20.0,
        "SCFDMA52-32QAM" | "OFDM52-32QAM" => 25.0,
        "SCFDMA52-64QAM" | "SCFDMA52-64QAM-P4" | "OFDM52-64QAM" => 30.0,
        // Narrowband SCFDMA26 fallback: ~3 dB more robust than the SCFDMA52 family.
        "SCFDMA26-8PSK" => 12.0,
        "SCFDMA26-16QAM" => 17.0,
        "SCFDMA26-32QAM" => 22.0,
        _ => 0.0,
    }
}

fn channel_snr_db(channel: &ChannelSpec) -> Option<f32> {
    if let ChannelSpec::Awgn { snr_db, .. } = channel {
        Some(*snr_db)
    } else {
        None
    }
}

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
        .chain(QAM64_MODES.iter())
        .chain(MULTICARRIER_MODES.iter())
        .chain(SCFDMA_HOM_MODES.iter())
        .chain(OFDM_HOM_MODES.iter())
        .chain(SMOKE_ONLY_MODES.iter())
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
    //       {128, 223} bytes — 223 is the max RS(255,223) input without SAR
    for mode in HF_FAST_MODES.iter().chain(QAM64_MODES.iter()) {
        for channel in &awgn_channels {
            if channel_snr_db(channel).is_some_and(|s| s < mode_min_snr_db(mode)) {
                continue;
            }
            for &fec in &[FecMode::None, FecMode::Rs, FecMode::RsInterleaved] {
                for &payload_len in &[128usize, 223] {
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
            // 8PSK without RRC is unreliable below 15 dB; only test it at the high-SNR tier.
            if channel_snr_db(channel).is_some_and(|s| s < mode_min_snr_db(mode)) {
                continue;
            }
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

    // ── 4. Compression matrix: HF fast + 64QAM modes × clean × {None, Rs} FEC × all compression × 128 bytes
    for mode in HF_FAST_MODES.iter().chain(QAM64_MODES.iter()) {
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

    // ── 5. Multi-carrier modes × AWGN sweep × {None, Rs, RsInterleaved} × 128 bytes ─────
    for mode in MULTICARRIER_MODES {
        // Padded full-width OFDM can't round-trip hard-RS block framing (see OFDM_RAW_FRAMING_ONLY);
        // exercise it raw (None) only — it stays covered, without the spurious RS-decode failures.
        let fecs: &[FecMode] = if OFDM_RAW_FRAMING_ONLY.contains(mode) {
            &[FecMode::None]
        } else {
            &[FecMode::None, FecMode::Rs, FecMode::RsInterleaved]
        };
        for channel in &awgn_channels {
            // OFDM52/SCFDMA52 are wideband modes not viable at 10 dB SNR; OFDM16 needs ≥15 dB too.
            if channel_snr_db(channel).is_some_and(|s| s < mode_min_snr_db(mode)) {
                continue;
            }
            for &fec in fecs {
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

    // ── 5a. LDPC × key HF modes × clean + AWGN × 32 bytes ───────────────────────────
    //        Payload capped at 32 bytes: LDPC single-block limit is ~112 bytes of user
    //        data after frame overhead; 32 bytes gives comfortable headroom and is
    //        sufficient to exercise the full soft-decode path.
    let ldpc_modes = &["BPSK250", "QPSK500", "8PSK500"];
    let ldpc_channels = vec![
        ChannelSpec::Clean,
        ChannelSpec::Awgn {
            snr_db: 20.0,
            seed: 42,
        },
    ];
    for mode in ldpc_modes {
        for channel in &ldpc_channels {
            cases.push(raw_case(
                mode,
                FecMode::Ldpc,
                CompressionAlgorithm::None,
                channel.clone(),
                32,
                tier,
            ));
        }
    }

    // ── 5b. Turbo × key HF modes × clean + AWGN × 32 bytes ──────────────────────────
    //        Payload capped at 32 bytes for the same reason as LDPC; turbo one-block
    //        limit is 764 bytes of info after the codec's own 4-byte wrapper.
    let turbo_modes = &["BPSK250", "QPSK500"];
    let turbo_channels = vec![
        ChannelSpec::Clean,
        ChannelSpec::Awgn {
            snr_db: 20.0,
            seed: 43,
        },
    ];
    for mode in turbo_modes {
        for channel in &turbo_channels {
            cases.push(raw_case(
                mode,
                FecMode::Turbo,
                CompressionAlgorithm::None,
                channel.clone(),
                32,
                tier,
            ));
        }
    }
    //        Payload kept at 32 bytes: these wideband modes are SNR-sensitive and
    //        the smoke + FEC coverage is sufficient for the Quick tier.
    //        SoftConcatenated included because the dense HOM modes operate under a
    //        soft code in practice (it is what closes them on a real link — see the
    //        --fec hardware loopback results); testing only None/Rs missed that path.
    for mode in SCFDMA_HOM_MODES.iter().chain(OFDM_HOM_MODES.iter()) {
        for channel in &awgn_channels {
            if channel_snr_db(channel).is_some_and(|s| s < mode_min_snr_db(mode)) {
                continue;
            }
            for &fec in &[FecMode::None, FecMode::Rs, FecMode::SoftConcatenated] {
                cases.push(raw_case(
                    mode,
                    fec,
                    CompressionAlgorithm::None,
                    channel.clone(),
                    32,
                    tier,
                ));
            }
        }
    }

    // ── 5c. Pilot-framed family × clean+AWGN × {None, Rs, Ldpc} × 32 bytes ────────
    //        Pilot is soft-capable, so Ldpc (rate-1/2) is its realistic dense-rung
    //        code; Rs is the hard baseline. Gated on the per-constellation floor.
    for mode in PILOT_MODES {
        for channel in &awgn_channels {
            if channel_snr_db(channel).is_some_and(|s| s < mode_min_snr_db(mode)) {
                continue;
            }
            for &fec in &[FecMode::None, FecMode::Rs, FecMode::Ldpc] {
                cases.push(raw_case(
                    mode,
                    fec,
                    CompressionAlgorithm::None,
                    channel.clone(),
                    32,
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
    // The OFDM higher-order ladder starts at OFDM16 (8 dB floor) and cannot step below
    // it, so skip AWGN channels weaker than that — unlike the BPSK31-floored ladders,
    // a sub-floor channel would fail every rung. Clean (no SNR) is always included.
    for channel in &awgn_channels {
        if channel_snr_db(channel).is_some_and(|snr| snr < 8.0) {
            continue;
        }
        cases.push(adaptive_case(
            UseCase::AdaptiveHpxOfdmHf,
            "HPX_OFDM_HF",
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

        // HF fast modes × all propagation channels × all FEC × {32, 128, 223} bytes
        for mode in HF_FAST_MODES {
            for channel in &prop_channels {
                for &fec in DATA_FEC_MODES {
                    for &payload_len in &[32usize, 128, 223] {
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

        // Multi-carrier × all propagation channels × {None, Rs, RsInterleaved, SoftConcatenated}
        for mode in MULTICARRIER_MODES {
            for channel in &prop_channels {
                for &fec in &[
                    FecMode::None,
                    FecMode::Rs,
                    FecMode::RsInterleaved,
                    FecMode::SoftConcatenated,
                ] {
                    if raw_framing_excludes(mode, fec) {
                        continue;
                    }
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

        // SC-FDMA higher-order × all propagation channels × {None, Rs, SoftConcatenated}
        // × 32 bytes. SoftConcatenated is the soft code these dense HOM modes run
        // under in practice (it is what closes them on a real link — see the --fec
        // hardware loopback results and the quick-tier sweep above); characterising
        // them across the fading channels with only None/Rs missed that path.
        for mode in SCFDMA_HOM_MODES.iter().chain(OFDM_HOM_MODES.iter()) {
            for channel in &prop_channels {
                for &fec in &[FecMode::None, FecMode::Rs, FecMode::SoftConcatenated] {
                    cases.push(raw_case(
                        mode,
                        fec,
                        CompressionAlgorithm::None,
                        channel.clone(),
                        32,
                        tier,
                    ));
                }
            }
        }

        // Large payload (223 bytes): key modes × key FEC × clean + awgn20
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
                    if raw_framing_excludes(mode, fec) {
                        continue;
                    }
                    cases.push(raw_case(
                        mode,
                        fec,
                        CompressionAlgorithm::None,
                        channel.clone(),
                        223,
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

#[cfg(test)]
mod coverage_tests {
    use super::*;
    use crate::runners::register_all;
    use openpulse_audio::loopback::LoopbackBackend;
    use openpulse_modem::ModemEngine;
    use std::collections::BTreeSet;

    fn raw_modem_modes() -> BTreeSet<String> {
        build_cases(Tier::Full)
            .into_iter()
            .filter(|c| matches!(c.use_case, UseCase::RawModem))
            .map(|c| c.mode)
            .collect()
    }

    fn registered_modes() -> BTreeSet<String> {
        let mut engine = ModemEngine::new(Box::new(LoopbackBackend::default()));
        register_all(&mut engine);
        engine
            .plugins()
            .list()
            .iter()
            .flat_map(|p| p.supported_modes.iter().cloned())
            .collect()
    }

    /// Every registered modulation mode must either have test coverage or be an explicit,
    /// documented post-v1.0 deferral. No silent exclusions.
    #[test]
    fn every_registered_mode_is_covered_or_deferred() {
        let covered = raw_modem_modes();
        let accounted: BTreeSet<String> = WIDEBAND_POST_V1_MODES
            .iter()
            .chain(KNOWN_LIMITATION_MODES.iter())
            .chain(PILOT_POST_V1_MODES.iter())
            .chain(DEMONSTRATOR_MODES.iter())
            .map(|s| s.to_string())
            .collect();
        let missing: Vec<String> = registered_modes()
            .into_iter()
            .filter(|m| !covered.contains(m) && !accounted.contains(m))
            .collect();
        assert!(
            missing.is_empty(),
            "registered modes with no matrix coverage and not explicitly accounted for \
             (add to a mode list in cases.rs, or to WIDEBAND_POST_V1_MODES / \
             KNOWN_LIMITATION_MODES / DEMONSTRATOR_MODES): {missing:?}"
        );
    }

    /// The deferred / known-limitation modes must really be excluded — never generated as
    /// cases (wideband modes can't run at 8 kHz; the known-limitation mode can't decode).
    #[test]
    fn deferred_and_known_limitation_modes_generate_no_cases() {
        let covered = raw_modem_modes();
        for m in WIDEBAND_POST_V1_MODES
            .iter()
            .chain(KNOWN_LIMITATION_MODES.iter())
            .chain(PILOT_POST_V1_MODES.iter())
            .chain(DEMONSTRATOR_MODES.iter())
        {
            assert!(
                !covered.contains(*m),
                "excused mode {m} was unexpectedly generated as a test case"
            );
        }
    }

    /// The excused lists must only contain modes that actually exist in the registry,
    /// so they cannot rot into referencing removed modes.
    #[test]
    fn excused_modes_exist_in_registry() {
        let registered = registered_modes();
        for m in WIDEBAND_POST_V1_MODES
            .iter()
            .chain(KNOWN_LIMITATION_MODES.iter())
            .chain(PILOT_POST_V1_MODES.iter())
            .chain(DEMONSTRATOR_MODES.iter())
        {
            assert!(
                registered.contains(*m),
                "excused-mode list references {m}, which is not a registered mode"
            );
        }
    }
}
