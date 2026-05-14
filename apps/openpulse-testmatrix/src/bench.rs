//! Multi-frame throughput benchmark.
//!
//! Runs N frames of each configuration through the full signal path (modulate →
//! channel model → demodulate, including FEC and compression) and measures the
//! fraction of frames decoded successfully and the resulting effective throughput.
//!
//! Unlike the single-frame pass/fail testmatrix, this produces *measured* bitrates
//! that account for frame-loss probability under each channel condition — suitable
//! as a reference baseline before on-air tests.
//!
//! Channel models are reused across frames within a configuration so that stateful
//! models (Watterson fading envelope, Gilbert-Elliott Markov chain) evolve
//! continuously, as they would on a real HF link.

use std::fs;
use std::path::Path;

use openpulse_core::compression::{compress_if_smaller, decompress, CompressionAlgorithm};
use openpulse_core::fec::FecMode;
use serde::{Deserialize, Serialize};

use crate::cases::mode_min_snr_db;
use crate::channels::build as build_channel;
use crate::matrix::{fec_label, ChannelSpec, TestCase, Tier, UseCase};
use crate::report::RunMeta;
use crate::runners::register_all;

const SAMPLE_RATE_HZ: f64 = 8000.0;
pub const PILOT_DENSITY_BASELINE_MODE: &str = "SCFDMA52-64QAM";
pub const PILOT_DENSITY_DENSE_MODE: &str = "SCFDMA52-64QAM-P4";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PilotDensitySweepProfile {
    Full,
    Crossover,
}

#[derive(Debug, Clone)]
pub struct PilotDensityGateResult {
    pub passed: bool,
    pub checks: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
pub enum CrossModeLevel {
    Sl12Baseline,
    Sl13,
    Sl14,
}

impl CrossModeLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Sl12Baseline => "sl12",
            Self::Sl13 => "sl13",
            Self::Sl14 => "sl14",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossModeBenchCase {
    pub family: String,
    pub level: CrossModeLevel,
    pub case: TestCase,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossModeBenchResult {
    pub family: String,
    pub level: CrossModeLevel,
    pub result: BenchResult,
}

#[derive(Debug, Clone)]
pub struct CrossModeGateResult {
    pub passed: bool,
    pub checks: Vec<String>,
}

/// Payload pattern representative of typical HF digital radio traffic.
///
/// A 64-byte repeating ASCII template is tiled to the requested length.
/// LZ4 compresses this to roughly 12–15% of the original size for large
/// payloads (good template for testing compression effectiveness).
fn bench_payload(len: usize) -> Vec<u8> {
    const TEMPLATE: &[u8] =
        b"OpenPulseHF benchmark payload - typical HF traffic pattern. 73 de TEST\n";
    TEMPLATE.iter().cycle().take(len).copied().collect()
}

/// Gross bit rate for a mode (symbol_rate × bits_per_symbol).
pub fn mode_gross_bps(mode: &str) -> f64 {
    match mode {
        "BPSK31" => 31.25,
        "BPSK63" => 62.5,
        "BPSK100" => 100.0,
        "BPSK250" | "BPSK250-RRC" => 250.0,
        "QPSK125" => 250.0,
        "QPSK250" => 500.0,
        "QPSK500" | "QPSK500-RRC" => 1000.0,
        "QPSK1000" | "QPSK1000-HF" | "QPSK1000-RRC" => 2000.0,
        "QPSK2000" | "QPSK2000-RRC" => 4000.0,
        "8PSK500" | "8PSK500-RRC" => 1500.0,
        "8PSK1000" | "8PSK1000-HF" | "8PSK1000-RRC" => 3000.0,
        "8PSK2000" | "8PSK2000-RRC" => 6000.0,
        "64QAM500" => 3000.0,
        "64QAM1000" => 6000.0,
        "64QAM2000-RRC" => 12000.0,
        "SCFDMA52-64QAM-P4" => 8167.0,
        "FSK4-ACK" => 200.0,
        "OFDM16" | "SCFDMA16" => 889.0,
        "OFDM52" | "SCFDMA52" => 2889.0,
        _ => 0.0,
    }
}

// ── Result type ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchResult {
    pub mode: String,
    pub channel: String,
    pub fec: String,
    pub compression: String,
    pub payload_len: usize,
    pub n_frames: usize,
    pub frames_ok: usize,
    /// Payload bytes successfully delivered across all frames.
    pub bytes_delivered: usize,
    /// Total TX samples across all frames (on-air sample count).
    pub total_tx_samples: usize,
    /// Physical on-air time: total_tx_samples / 8000 Hz.
    pub on_air_s: f64,
    /// frames_ok / n_frames × 100.
    pub success_rate_pct: f64,
    /// bytes_delivered × 8 / on_air_s — actual delivered throughput.
    pub measured_bps: f64,
    /// Symbol rate × bits/symbol — theoretical maximum with no losses or overhead.
    pub theoretical_gross_bps: f64,
    /// measured_bps / theoretical_gross_bps × 100.
    pub efficiency_pct: f64,
    /// Median per-frame on-air time across the benchmark run.
    pub median_frame_time_ms: u64,
    /// p95 per-frame on-air time across the benchmark run.
    pub p95_frame_time_ms: u64,
}

fn percentile_u64(values: &[u64], percentile: f64) -> u64 {
    if values.is_empty() {
        return 0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let rank = ((sorted.len() - 1) as f64 * percentile).round() as usize;
    sorted[rank.min(sorted.len() - 1)]
}

// ── Runner ────────────────────────────────────────────────────────────────────

/// Run `n_frames` of `case` through the full signal path and collect throughput statistics.
pub fn run_bench(case: &TestCase, n_frames: usize) -> BenchResult {
    use openpulse_modem::channel_sim::ChannelSimHarness;

    let payload = bench_payload(case.payload_len);
    // Build channel once so its internal state evolves across frames.
    let mut channel = build_channel(&case.channel);

    let mut frames_ok = 0usize;
    let mut bytes_delivered = 0usize;
    let mut total_tx_samples = 0usize;
    let mut frame_time_ms = Vec::with_capacity(n_frames);

    for _ in 0..n_frames {
        // Fresh harness per frame: independent timing/carrier recovery state.
        let mut h = ChannelSimHarness::new();
        register_all(&mut h.tx_engine);
        register_all(&mut h.rx_engine);

        // Compress
        let (wire, actual_algo) = match case.compression {
            CompressionAlgorithm::None => (payload.clone(), CompressionAlgorithm::None),
            CompressionAlgorithm::Lz4 | CompressionAlgorithm::Zstd(_) => {
                compress_if_smaller(&payload)
            }
        };

        // Transmit (dispatch on FecMode)
        let tx_ok = match case.fec_mode {
            FecMode::None => h.tx_engine.transmit(&wire, &case.mode, None),
            FecMode::Rs => h.tx_engine.transmit_with_fec(&wire, &case.mode, None),
            FecMode::RsInterleaved => h
                .tx_engine
                .transmit_with_fec_interleaved(&wire, &case.mode, None, 5),
            FecMode::Concatenated => h
                .tx_engine
                .transmit_with_concatenated_fec(&wire, &case.mode, None),
            FecMode::RsStrong => h
                .tx_engine
                .transmit_with_strong_fec(&wire, &case.mode, None),
            FecMode::SoftConcatenated => h
                .tx_engine
                .transmit_with_soft_viterbi_fec(&wire, &case.mode, None),
            // ACK-frame-only or deferred — not applicable in bench.
            FecMode::ShortRs | FecMode::Ldpc => break,
        };

        // Route through channel regardless of TX outcome: on-air time is always consumed.
        let tx_samples = h.route(channel.as_mut());
        total_tx_samples += tx_samples;
        frame_time_ms.push((tx_samples as f64 * 1000.0 / SAMPLE_RATE_HZ).round() as u64);

        if tx_ok.is_err() {
            continue;
        }

        // Receive (matching FecMode)
        let rx_raw = match case.fec_mode {
            FecMode::None => h.rx_engine.receive(&case.mode, None),
            FecMode::Rs => h.rx_engine.receive_with_fec(&case.mode, None),
            FecMode::RsInterleaved => h
                .rx_engine
                .receive_with_fec_interleaved(&case.mode, None, 5),
            FecMode::Concatenated => h.rx_engine.receive_with_concatenated_fec(&case.mode, None),
            FecMode::RsStrong => h.rx_engine.receive_with_strong_fec(&case.mode, None),
            FecMode::SoftConcatenated => {
                h.rx_engine.receive_with_soft_viterbi_fec(&case.mode, None)
            }
            FecMode::ShortRs | FecMode::Ldpc => break,
        };

        let Ok(rx_raw) = rx_raw else { continue };
        let Ok(rx_data) = decompress(&rx_raw, actual_algo) else {
            continue;
        };

        if rx_data == payload {
            frames_ok += 1;
            bytes_delivered += payload.len();
        }
    }

    let on_air_s = total_tx_samples as f64 / SAMPLE_RATE_HZ;
    let measured_bps = if on_air_s > 0.0 {
        bytes_delivered as f64 * 8.0 / on_air_s
    } else {
        0.0
    };
    let theoretical = mode_gross_bps(&case.mode);
    let success_rate_pct = frames_ok as f64 / n_frames.max(1) as f64 * 100.0;
    let efficiency_pct = if theoretical > 0.0 {
        measured_bps / theoretical * 100.0
    } else {
        0.0
    };
    let median_frame_time_ms = percentile_u64(&frame_time_ms, 0.5);
    let p95_frame_time_ms = percentile_u64(&frame_time_ms, 0.95);
    let comp_label = match case.compression {
        CompressionAlgorithm::None => "none",
        CompressionAlgorithm::Lz4 => "lz4",
        CompressionAlgorithm::Zstd(_) => "zstd",
    };

    BenchResult {
        mode: case.mode.clone(),
        channel: case.channel.label(),
        fec: fec_label(case.fec_mode).to_string(),
        compression: comp_label.to_string(),
        payload_len: case.payload_len,
        n_frames,
        frames_ok,
        bytes_delivered,
        total_tx_samples,
        on_air_s,
        success_rate_pct,
        measured_bps,
        theoretical_gross_bps: theoretical,
        efficiency_pct,
        median_frame_time_ms,
        p95_frame_time_ms,
    }
}

// ── Case builder ──────────────────────────────────────────────────────────────

/// Build the throughput benchmark case list.
///
/// Full tier keeps comprehensive coverage; quick tier keeps representative
/// cases so `--bench` remains practical during iterative development.
pub fn build_bench_cases(payload_len: usize, tier: Tier) -> Vec<TestCase> {
    const LOW_SNR_SWEEP_DB: &[f32] = &[10.0, 8.0, 5.0, 3.0, 0.0];

    const BENCH_MODES_FULL: &[&str] = &[
        "BPSK250",
        "BPSK250-RRC",
        "QPSK500",
        "QPSK500-RRC",
        "QPSK1000-HF",
        "QPSK1000-RRC",
        "8PSK500",
        "8PSK1000-HF",
        "64QAM500",
        "64QAM1000",
        "OFDM52",
        "SCFDMA52",
        "SCFDMA52-16QAM",
        "SCFDMA52-64QAM",
        "SCFDMA52-64QAM-P4",
    ];

    const BENCH_MODES_QUICK: &[&str] = &[
        "BPSK250",
        "QPSK500",
        "QPSK1000-HF",
        "8PSK1000-HF",
        "64QAM1000",
        "SCFDMA52",
        "SCFDMA52-64QAM",
        "SCFDMA52-64QAM-P4",
    ];

    let mut bench_channels_full = vec![
        ChannelSpec::Clean,
        ChannelSpec::Awgn {
            snr_db: 30.0,
            seed: 42,
        },
        ChannelSpec::Awgn {
            snr_db: 25.0,
            seed: 42,
        },
        ChannelSpec::Awgn {
            snr_db: 20.0,
            seed: 42,
        },
        ChannelSpec::Awgn {
            snr_db: 15.0,
            seed: 42,
        },
        ChannelSpec::Awgn {
            snr_db: 10.0,
            seed: 42,
        },
        ChannelSpec::Awgn {
            snr_db: 5.0,
            seed: 42,
        },
        ChannelSpec::Awgn {
            snr_db: 0.0,
            seed: 42,
        },
        ChannelSpec::WattersonGoodF1,
        ChannelSpec::WattersonGoodF2,
        ChannelSpec::WattersonPoorF1,
        ChannelSpec::GilbertElliottLight,
        ChannelSpec::GilbertElliottModerate,
    ];

    bench_channels_full.extend(
        LOW_SNR_SWEEP_DB
            .iter()
            .copied()
            .map(|snr_db| ChannelSpec::WattersonGoodF1Snr { snr_db, seed: 101 }),
    );
    bench_channels_full.extend(
        LOW_SNR_SWEEP_DB
            .iter()
            .copied()
            .map(|snr_db| ChannelSpec::WattersonGoodF2Snr { snr_db, seed: 102 }),
    );

    let bench_channels_quick = vec![
        ChannelSpec::Clean,
        ChannelSpec::Awgn {
            snr_db: 20.0,
            seed: 42,
        },
        ChannelSpec::Awgn {
            snr_db: 10.0,
            seed: 42,
        },
        ChannelSpec::Awgn {
            snr_db: 5.0,
            seed: 42,
        },
        ChannelSpec::Awgn {
            snr_db: 0.0,
            seed: 42,
        },
        ChannelSpec::WattersonGoodF1,
        ChannelSpec::WattersonGoodF2,
        ChannelSpec::WattersonGoodF1Snr {
            snr_db: 5.0,
            seed: 101,
        },
        ChannelSpec::WattersonGoodF2Snr {
            snr_db: 5.0,
            seed: 102,
        },
    ];

    const BENCH_FEC_FULL: &[FecMode] = &[FecMode::None, FecMode::Rs, FecMode::SoftConcatenated];
    const BENCH_COMP_FULL: &[CompressionAlgorithm] =
        &[CompressionAlgorithm::None, CompressionAlgorithm::Lz4];
    const BENCH_FEC_QUICK: &[FecMode] = &[FecMode::None, FecMode::Rs];
    const BENCH_COMP_QUICK: &[CompressionAlgorithm] = &[CompressionAlgorithm::None];

    let bench_modes = if tier == Tier::Full {
        BENCH_MODES_FULL
    } else {
        BENCH_MODES_QUICK
    };
    let bench_channels = if tier == Tier::Full {
        &bench_channels_full
    } else {
        &bench_channels_quick
    };
    let bench_fec = if tier == Tier::Full {
        BENCH_FEC_FULL
    } else {
        BENCH_FEC_QUICK
    };
    let bench_comp = if tier == Tier::Full {
        BENCH_COMP_FULL
    } else {
        BENCH_COMP_QUICK
    };

    let mut cases = Vec::new();
    for &mode in bench_modes {
        for channel in bench_channels {
            // Enforce per-mode SNR floor on AWGN channels.
            if let ChannelSpec::Awgn { snr_db, .. } = channel {
                if snr_db < &mode_min_snr_db(mode) {
                    continue;
                }
            }
            for &fec in bench_fec {
                for &comp in bench_comp {
                    cases.push(TestCase {
                        use_case: UseCase::RawModem,
                        mode: mode.to_string(),
                        fec_mode: fec,
                        compression: comp,
                        channel: channel.clone(),
                        payload_len,
                        tier,
                    });
                }
            }
        }
    }
    cases
}

/// Build focused BL-TP-7 pilot-density sweep cases.
///
/// This sweep compares only baseline vs dense-pilot SC-FDMA 64QAM modes across
/// a multi-seed SNR ladder for AWGN and Watterson Good F1/F2 channels.
pub fn build_pilot_density_sweep_cases(
    payload_len: usize,
    tier: Tier,
    profile: PilotDensitySweepProfile,
) -> Vec<TestCase> {
    let modes = [PILOT_DENSITY_BASELINE_MODE, PILOT_DENSITY_DENSE_MODE];
    let fec_modes = [FecMode::None, FecMode::Rs];
    let mut cases = Vec::new();

    if profile == PilotDensitySweepProfile::Crossover {
        let awgn_snr_db = [22.0, 24.0];
        let awgn_seeds = [42, 123, 777, 4242, 9123];
        let watter_snr_db = [20.0, 22.0, 24.0];
        let watter_seeds = [101, 202, 303, 404, 505];

        for &mode in &modes {
            for &fec in &fec_modes {
                for &snr_db in &awgn_snr_db {
                    for &seed in &awgn_seeds {
                        cases.push(TestCase {
                            use_case: UseCase::RawModem,
                            mode: mode.to_string(),
                            fec_mode: fec,
                            compression: CompressionAlgorithm::None,
                            channel: ChannelSpec::Awgn { snr_db, seed },
                            payload_len,
                            tier,
                        });
                    }
                }

                for &snr_db in &watter_snr_db {
                    for &seed in &watter_seeds {
                        cases.push(TestCase {
                            use_case: UseCase::RawModem,
                            mode: mode.to_string(),
                            fec_mode: fec,
                            compression: CompressionAlgorithm::None,
                            channel: ChannelSpec::WattersonGoodF1Snr { snr_db, seed },
                            payload_len,
                            tier,
                        });
                        cases.push(TestCase {
                            use_case: UseCase::RawModem,
                            mode: mode.to_string(),
                            fec_mode: fec,
                            compression: CompressionAlgorithm::None,
                            channel: ChannelSpec::WattersonGoodF2Snr { snr_db, seed },
                            payload_len,
                            tier,
                        });
                    }
                }
            }
        }

        return cases;
    }

    let (awgn_snr_db, awgn_seeds, watter_snr_db, watter_seeds): (&[f32], &[u64], &[f32], &[u64]) =
        if tier == Tier::Full {
            (
                &[
                    16.0, 17.0, 18.0, 19.0, 20.0, 21.0, 22.0, 23.0, 24.0, 25.0, 26.0, 27.0, 28.0,
                ],
                &[11, 42, 77, 123],
                &[14.0, 12.0, 10.0, 8.0, 6.0, 5.0, 4.0],
                &[101, 202, 303, 404],
            )
        } else {
            (
                &[18.0, 20.0, 22.0, 24.0, 26.0, 28.0],
                &[42, 123, 777],
                &[24.0, 22.0, 20.0, 18.0, 16.0, 14.0, 12.0, 10.0, 8.0, 6.0],
                &[101, 202, 303],
            )
        };

    for &mode in &modes {
        for &fec in &fec_modes {
            for &snr_db in awgn_snr_db {
                for &seed in awgn_seeds {
                    cases.push(TestCase {
                        use_case: UseCase::RawModem,
                        mode: mode.to_string(),
                        fec_mode: fec,
                        compression: CompressionAlgorithm::None,
                        channel: ChannelSpec::Awgn { snr_db, seed },
                        payload_len,
                        tier,
                    });
                }
            }

            for &snr_db in watter_snr_db {
                for &seed in watter_seeds {
                    cases.push(TestCase {
                        use_case: UseCase::RawModem,
                        mode: mode.to_string(),
                        fec_mode: fec,
                        compression: CompressionAlgorithm::None,
                        channel: ChannelSpec::WattersonGoodF1Snr { snr_db, seed },
                        payload_len,
                        tier,
                    });
                    cases.push(TestCase {
                        use_case: UseCase::RawModem,
                        mode: mode.to_string(),
                        fec_mode: fec,
                        compression: CompressionAlgorithm::None,
                        channel: ChannelSpec::WattersonGoodF2Snr { snr_db, seed },
                        payload_len,
                        tier,
                    });
                }
            }
        }
    }

    cases
}

pub fn build_cross_mode_cases(payload_len: usize, tier: Tier) -> Vec<CrossModeBenchCase> {
    let families: &[(&str, &[(CrossModeLevel, &str, FecMode)])] = &[
        (
            "BPSK250",
            &[
                (CrossModeLevel::Sl12Baseline, "BPSK250", FecMode::RsStrong),
                (CrossModeLevel::Sl13, "BPSK250", FecMode::Rs),
                (CrossModeLevel::Sl14, "BPSK250", FecMode::None),
            ],
        ),
        (
            "QPSK500",
            &[
                (CrossModeLevel::Sl12Baseline, "QPSK500", FecMode::RsStrong),
                (CrossModeLevel::Sl13, "QPSK500", FecMode::Rs),
                (CrossModeLevel::Sl14, "QPSK500", FecMode::None),
            ],
        ),
        (
            "64QAM",
            &[
                (CrossModeLevel::Sl12Baseline, "64QAM500", FecMode::RsStrong),
                (CrossModeLevel::Sl13, "64QAM1000", FecMode::Rs),
                (CrossModeLevel::Sl14, "64QAM2000-RRC", FecMode::None),
            ],
        ),
        (
            "SCFDMA52",
            &[
                (CrossModeLevel::Sl12Baseline, "SCFDMA52", FecMode::RsStrong),
                (CrossModeLevel::Sl13, "SCFDMA52-16QAM", FecMode::Rs),
                (CrossModeLevel::Sl14, "SCFDMA52-64QAM", FecMode::None),
            ],
        ),
    ];

    let full_channels = [
        ChannelSpec::Awgn {
            snr_db: 20.0,
            seed: 42,
        },
        ChannelSpec::WattersonGoodF1,
        ChannelSpec::WattersonGoodF2,
        ChannelSpec::GilbertElliottLight,
    ];
    let quick_channels = [ChannelSpec::Awgn {
        snr_db: 20.0,
        seed: 42,
    }];
    let channels = if tier == Tier::Full {
        &full_channels[..]
    } else {
        &quick_channels[..]
    };

    let mut cases = Vec::new();
    for (family, levels) in families {
        for &(level, mode, fec_mode) in *levels {
            for channel in channels {
                cases.push(CrossModeBenchCase {
                    family: (*family).to_string(),
                    level,
                    case: TestCase {
                        use_case: UseCase::RawModem,
                        mode: mode.to_string(),
                        fec_mode,
                        compression: CompressionAlgorithm::None,
                        channel: channel.clone(),
                        payload_len,
                        tier,
                    },
                });
            }
        }
    }
    cases
}

pub fn evaluate_cross_mode_consistency_gate(
    current: &[CrossModeBenchResult],
    baseline: &[CrossModeBenchResult],
) -> CrossModeGateResult {
    use std::collections::BTreeMap;

    let mut checks = Vec::new();
    let mut passed = true;

    let baseline_map: BTreeMap<(String, CrossModeLevel, String), &CrossModeBenchResult> = baseline
        .iter()
        .map(|row| {
            (
                (row.family.clone(), row.level, row.result.channel.clone()),
                row,
            )
        })
        .collect();

    for row in current {
        let key = (row.family.clone(), row.level, row.result.channel.clone());
        if let Some(prev) = baseline_map.get(&key) {
            let baseline_bps = prev.result.measured_bps;
            // A zero baseline means the prior run itself was broken for this case;
            // treat that as a regression (ratio 0%) rather than silently passing.
            let ratio = if baseline_bps > 0.0 {
                row.result.measured_bps / baseline_bps
            } else {
                0.0
            };
            let ok = ratio >= 0.95;
            if !ok {
                passed = false;
            }
            checks.push(format!(
                "{} throughput {} {} {}: {:.1}% of baseline ({:.1} vs {:.1} bps)",
                if ok { "PASS" } else { "FAIL" },
                row.family,
                row.level.label(),
                row.result.channel,
                ratio * 100.0,
                row.result.measured_bps,
                baseline_bps,
            ));
        } else if !baseline_map.is_empty() {
            // Baseline exists but is missing this key — treat as a gate failure so
            // a partial baseline cannot silently under-check the regression gate.
            passed = false;
            checks.push(format!(
                "FAIL throughput {} {} {}: key missing from baseline",
                row.family,
                row.level.label(),
                row.result.channel,
            ));
        }

        let median_ok = row.result.median_frame_time_ms <= 1500;
        let p95_ok = row.result.p95_frame_time_ms <= 2000;
        if !median_ok || !p95_ok {
            passed = false;
        }
        checks.push(format!(
            "{} latency {} {} {}: median={} ms, p95={} ms",
            if median_ok && p95_ok { "PASS" } else { "FAIL" },
            row.family,
            row.level.label(),
            row.result.channel,
            row.result.median_frame_time_ms,
            row.result.p95_frame_time_ms,
        ));
    }

    let mut grouped: BTreeMap<(String, String), Vec<&CrossModeBenchResult>> = BTreeMap::new();
    for row in current {
        grouped
            .entry((row.family.clone(), row.result.channel.clone()))
            .or_default()
            .push(row);
    }

    for ((family, channel), mut rows) in grouped {
        rows.sort_by_key(|row| row.level);
        for pair in rows.windows(2) {
            let prev = pair[0];
            let next = pair[1];
            let prev_bps = prev.result.measured_bps;
            // A lower level at 0 bps means the ladder is already broken;
            // treat as regression rather than silently passing.
            let ratio = if prev_bps > 0.0 {
                next.result.measured_bps / prev_bps
            } else {
                0.0
            };
            let ok = ratio >= 0.97;
            if !ok {
                passed = false;
            }
            checks.push(format!(
                "{} ladder {} {} {}->{}: {:.1}% ({:.1} vs {:.1} bps)",
                if ok { "PASS" } else { "FAIL" },
                family,
                channel,
                prev.level.label(),
                next.level.label(),
                ratio * 100.0,
                next.result.measured_bps,
                prev_bps,
            ));
        }
    }

    CrossModeGateResult { passed, checks }
}

pub fn load_cross_mode_results(dir: &Path) -> Option<Vec<CrossModeBenchResult>> {
    let path = dir.join("cross_mode.json");
    if !path.exists() {
        return None;
    }
    let json = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "[cross-mode-gate] warning: could not read {}: {}",
                path.display(),
                e
            );
            return None;
        }
    };
    match serde_json::from_str(&json) {
        Ok(results) => Some(results),
        Err(e) => {
            eprintln!(
                "[cross-mode-gate] warning: baseline {} is present but failed to parse ({}); treating as missing — regenerate with a clean run",
                path.display(),
                e
            );
            None
        }
    }
}

pub fn write_cross_mode_report(
    results: &[CrossModeBenchResult],
    gate: &CrossModeGateResult,
    dir: &Path,
    meta: &RunMeta,
    n_frames: usize,
    payload_len: usize,
    elapsed_s: f64,
) {
    fs::create_dir_all(dir).expect("create cross-mode report directory");

    let mut md = String::new();
    md.push_str("---\n");
    md.push_str(&format!(
        "title: \"OpenPulseHF Cross-Mode Gate\"\ndate: \"{}\"\ngit_commit: \"{}\"\n",
        meta.date.format("%Y-%m-%dT%H:%M:%SZ"),
        meta.git_commit,
    ));
    md.push_str("---\n\n");
    md.push_str("# OpenPulseHF Cross-Mode Gate\n\n");
    md.push_str(&format!("**Run:** {}\n\n", meta.identity_line()));
    md.push_str(&format!(
        "**Methodology:** {} rows, {n_frames} frames/case, payload {payload_len} B, elapsed {elapsed_s:.1}s.\n\n",
        results.len(),
    ));
    md.push_str(&format!(
        "**Verdict:** {}\n\n",
        if gate.passed { "PASS" } else { "FAIL" }
    ));
    md.push_str("| Family | Level | Mode | Channel | Measured bps | Median ms | p95 ms |\n");
    md.push_str("|---|---|---|---|---:|---:|---:|\n");
    for row in results {
        md.push_str(&format!(
            "| {} | {} | {} | {} | {:.1} | {} | {} |\n",
            row.family,
            row.level.label(),
            row.result.mode,
            row.result.channel,
            row.result.measured_bps,
            row.result.median_frame_time_ms,
            row.result.p95_frame_time_ms,
        ));
    }
    md.push_str("\n## Checks\n\n");
    for check in &gate.checks {
        md.push_str(&format!("- {}\n", check));
    }
    fs::write(dir.join("cross_mode.md"), md).expect("write cross_mode.md");

    let json = serde_json::to_string_pretty(results).expect("serialize cross-mode results");
    fs::write(dir.join("cross_mode.json"), json).expect("write cross_mode.json");
}

// ── Report writers ────────────────────────────────────────────────────────────

pub fn write_bench_report(
    results: &[BenchResult],
    dir: &Path,
    meta: &RunMeta,
    n_frames: usize,
    payload_len: usize,
    elapsed_s: f64,
) {
    fs::create_dir_all(dir).expect("create benchmark report directory");
    write_bench_markdown(results, dir, meta, n_frames, payload_len, elapsed_s);
    write_bench_csv(results, dir, meta);
    write_bench_json(results, dir);
}

fn write_bench_markdown(
    results: &[BenchResult],
    dir: &Path,
    meta: &RunMeta,
    n_frames: usize,
    payload_len: usize,
    elapsed_s: f64,
) {
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&format!(
        "title: \"OpenPulseHF Throughput Benchmark\"\ndate: \"{}\"\ngit_commit: \"{}\"\n\
         n_frames: {n_frames}\npayload_bytes: {payload_len}\n",
        meta.date.format("%Y-%m-%dT%H:%M:%SZ"),
        meta.git_commit,
    ));
    out.push_str("---\n\n");
    out.push_str("# OpenPulseHF Throughput Benchmark\n\n");
    out.push_str(&format!("**Run:** {}\n\n", meta.identity_line()));
    out.push_str(&format!(
        "**Methodology:** {n_frames} frames per configuration. \
         Payload: {payload_len}-byte repeating ASCII pattern (highly compressible with LZ4). \
         Channel model reused across frames (stateful fading/burst evolution). \
         Fresh modem harness per frame (independent timing recovery). \
         Elapsed: {elapsed_s:.1}s.\n\n"
    ));
    out.push_str(
        "**Columns:** `OK/N` = frames decoded correctly / frames transmitted. \
         `Meas. bps` = payload bits delivered / total on-air time (accounts for frame loss). \
         `Theor. bps` = symbol rate × bits/symbol (no loss, no overhead). \
         `Eff.%` = Meas. / Theor. × 100.\n\n",
    );

    // Group by mode
    let mut modes: Vec<&str> = results
        .iter()
        .map(|r| r.mode.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    modes.sort();

    for mode in modes {
        out.push_str(&format!("## {mode}\n\n"));
        out.push_str(
            "| Channel | FEC | Comp | OK/N | Success | Meas. bps | Theor. bps | Eff.% |\n",
        );
        out.push_str("|---|---|---|---|---|---|---|---|\n");

        for r in results.iter().filter(|r| r.mode == mode) {
            out.push_str(&format!(
                "| {} | {} | {} | {}/{} | {:.1}% | {} | {} | {:.1}% |\n",
                r.channel,
                r.fec,
                r.compression,
                r.frames_ok,
                r.n_frames,
                r.success_rate_pct,
                fmt_bps(r.measured_bps),
                fmt_bps(r.theoretical_gross_bps),
                r.efficiency_pct,
            ));
        }
        out.push('\n');
    }

    fs::write(dir.join("throughput.md"), out).expect("write throughput.md");
}

fn write_bench_csv(results: &[BenchResult], dir: &Path, meta: &RunMeta) {
    let mut out = String::new();
    out.push_str(
        "run_date,run_commit,mode,channel,fec,compression,payload_bytes,\
         n_frames,frames_ok,bytes_delivered,total_tx_samples,on_air_s,\
         success_rate_pct,measured_bps,theoretical_gross_bps,efficiency_pct\n",
    );
    let run_date = meta.date.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let dirty = if meta.git_dirty { "*" } else { "" };
    let run_commit = format!("{}{dirty}", meta.git_commit);

    for r in results {
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{:.4},{:.2},{:.2},{:.2},{:.2}\n",
            run_date,
            run_commit,
            r.mode,
            r.channel,
            r.fec,
            r.compression,
            r.payload_len,
            r.n_frames,
            r.frames_ok,
            r.bytes_delivered,
            r.total_tx_samples,
            r.on_air_s,
            r.success_rate_pct,
            r.measured_bps,
            r.theoretical_gross_bps,
            r.efficiency_pct,
        ));
    }
    fs::write(dir.join("throughput.csv"), out).expect("write throughput.csv");
}

fn write_bench_json(results: &[BenchResult], dir: &Path) {
    let json = serde_json::to_string_pretty(results).expect("serialize bench results");
    fs::write(dir.join("throughput.json"), json).expect("write throughput.json");
}

fn fmt_bps(bps: f64) -> String {
    if bps >= 1000.0 {
        format!("{:.2} kbps", bps / 1000.0)
    } else {
        format!("{:.1} bps", bps)
    }
}

pub fn write_pilot_density_report(
    results: &[BenchResult],
    dir: &Path,
    meta: &RunMeta,
    n_frames: usize,
    payload_len: usize,
    elapsed_s: f64,
) {
    fs::create_dir_all(dir).expect("create pilot-density report directory");

    let mut rows = std::collections::BTreeMap::<
        (String, String),
        (Vec<&BenchResult>, Vec<&BenchResult>),
    >::new();

    for r in results {
        if r.mode != PILOT_DENSITY_BASELINE_MODE && r.mode != PILOT_DENSITY_DENSE_MODE {
            continue;
        }
        let key = (r.channel.clone(), r.fec.clone());
        let entry = rows.entry(key).or_insert((Vec::new(), Vec::new()));
        if r.mode == PILOT_DENSITY_BASELINE_MODE {
            entry.0.push(r);
        } else if r.mode == PILOT_DENSITY_DENSE_MODE {
            entry.1.push(r);
        }
    }

    let mut md = String::new();
    md.push_str("---\n");
    md.push_str(&format!(
        "title: \"BL-TP-7 Pilot Density Sweep\"\ndate: \"{}\"\ngit_commit: \"{}\"\n",
        meta.date.format("%Y-%m-%dT%H:%M:%SZ"),
        meta.git_commit,
    ));
    md.push_str("---\n\n");
    md.push_str("# BL-TP-7 Pilot Density Sweep\n\n");
    md.push_str(&format!("**Run:** {}\n\n", meta.identity_line()));
    md.push_str(&format!(
        "**Methodology:** {} vs {} with {n_frames} frames/case, payload {payload_len} B, compression none, elapsed {elapsed_s:.1}s.\n\n",
        PILOT_DENSITY_BASELINE_MODE, PILOT_DENSITY_DENSE_MODE
    ));
    md.push_str("| Channel | FEC | Seeds | Baseline Success | Dense Success | Delta Success | Baseline bps | Dense bps | Delta bps |\n");
    md.push_str("|---|---|---:|---:|---:|---:|---:|---:|---:|\n");

    let mut csv = String::new();
    csv.push_str(
        "run_date,run_commit,channel,fec,seeds,baseline_success_pct,dense_success_pct,delta_success_pct,baseline_bps,dense_bps,delta_bps\n",
    );

    let mut policy_md = String::new();
    policy_md.push_str("---\n");
    policy_md.push_str(&format!(
        "title: \"BL-TP-7 Pilot Density Policy\"\ndate: \"{}\"\ngit_commit: \"{}\"\n",
        meta.date.format("%Y-%m-%dT%H:%M:%SZ"),
        meta.git_commit,
    ));
    policy_md.push_str("---\n\n");
    policy_md.push_str("# BL-TP-7 Pilot Density Policy\n\n");
    policy_md.push_str(
        "Policy rule: prefer dense pilots at crossover/edge unless baseline is already high-margin.\n\n",
    );
    policy_md.push_str("| Channel | FEC | Seeds | Baseline Success | Dense Success | Delta Success | Delta bps | Recommended Mode | Rationale |\n");
    policy_md.push_str("|---|---|---:|---:|---:|---:|---:|---|---|\n");

    let mut policy_csv = String::new();
    policy_csv.push_str(
        "run_date,run_commit,channel,fec,seeds,baseline_success_pct,dense_success_pct,delta_success_pct,delta_bps,recommended_mode,rationale\n",
    );
    let run_date = meta.date.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let dirty = if meta.git_dirty { "*" } else { "" };
    let run_commit = format!("{}{dirty}", meta.git_commit);

    for ((channel, fec), (base, dense)) in rows {
        if base.is_empty() || dense.is_empty() {
            continue;
        }

        let base_success_mean =
            base.iter().map(|r| r.success_rate_pct).sum::<f64>() / base.len() as f64;
        let dense_success_mean =
            dense.iter().map(|r| r.success_rate_pct).sum::<f64>() / dense.len() as f64;
        let base_bps_mean = base.iter().map(|r| r.measured_bps).sum::<f64>() / base.len() as f64;
        let dense_bps_mean = dense.iter().map(|r| r.measured_bps).sum::<f64>() / dense.len() as f64;
        let delta_success = dense_success_mean - base_success_mean;
        let delta_bps = dense_bps_mean - base_bps_mean;
        let seeds = base.len().min(dense.len());

        let (recommended_mode, rationale) = if base_success_mean >= 95.0
            && dense_success_mean >= 95.0
            && delta_success.abs() <= 1.0
        {
            (
                PILOT_DENSITY_BASELINE_MODE,
                "high-margin region (both stable)",
            )
        } else if delta_success > 0.0 || delta_bps > 0.0 {
            (PILOT_DENSITY_DENSE_MODE, "crossover/edge gain")
        } else {
            (PILOT_DENSITY_BASELINE_MODE, "dense gain not observed")
        };

        md.push_str(&format!(
            "| {} | {} | {} | {:.1}% | {:.1}% | {:+.1}% | {:.1} | {:.1} | {:+.1} |\n",
            channel,
            fec,
            seeds,
            base_success_mean,
            dense_success_mean,
            delta_success,
            base_bps_mean,
            dense_bps_mean,
            delta_bps,
        ));

        csv.push_str(&format!(
            "{},{},{},{},{},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2}\n",
            run_date,
            run_commit,
            channel,
            fec,
            seeds,
            base_success_mean,
            dense_success_mean,
            delta_success,
            base_bps_mean,
            dense_bps_mean,
            delta_bps,
        ));

        policy_md.push_str(&format!(
            "| {} | {} | {} | {:.1}% | {:.1}% | {:+.1}% | {:+.1} | {} | {} |\n",
            channel,
            fec,
            seeds,
            base_success_mean,
            dense_success_mean,
            delta_success,
            delta_bps,
            recommended_mode,
            rationale,
        ));

        policy_csv.push_str(&format!(
            "{},{},{},{},{},{:.2},{:.2},{:.2},{:.2},{},{}\n",
            run_date,
            run_commit,
            channel,
            fec,
            seeds,
            base_success_mean,
            dense_success_mean,
            delta_success,
            delta_bps,
            recommended_mode,
            rationale,
        ));
    }

    fs::write(dir.join("pilot_density.md"), md).expect("write pilot_density.md");
    fs::write(dir.join("pilot_density.csv"), csv).expect("write pilot_density.csv");
    fs::write(dir.join("pilot_density_policy.md"), policy_md)
        .expect("write pilot_density_policy.md");
    fs::write(dir.join("pilot_density_policy.csv"), policy_csv)
        .expect("write pilot_density_policy.csv");
}

/// Evaluate a BL-TP-7 crossover regression gate from raw sweep results.
///
/// Gate conditions (mean across seeds):
/// - `awgn_22dB` + `rs`: dense must improve success by at least 10 percentage points.
/// - `awgn_24dB` + `none`: dense must improve success by at least 3 percentage points.
/// - `watterson_good_f1_24p00dB` + `rs`: dense must be non-degrading (delta ≥ 0).
pub fn evaluate_pilot_density_crossover_gate(results: &[BenchResult]) -> PilotDensityGateResult {
    fn mean_for(
        results: &[BenchResult],
        mode: &str,
        channel: &str,
        fec: &str,
    ) -> Option<(f64, f64, usize)> {
        let rows: Vec<&BenchResult> = results
            .iter()
            .filter(|r| r.mode == mode && r.channel == channel && r.fec == fec)
            .collect();
        if rows.is_empty() {
            return None;
        }
        let success = rows.iter().map(|r| r.success_rate_pct).sum::<f64>() / rows.len() as f64;
        let bps = rows.iter().map(|r| r.measured_bps).sum::<f64>() / rows.len() as f64;
        Some((success, bps, rows.len()))
    }

    let checks_cfg = [
        ("awgn_22dB", "rs", 10.0_f64, "AWGN 22 dB + RS"),
        ("awgn_24dB", "none", 3.0_f64, "AWGN 24 dB + none"),
        (
            "watterson_good_f1_24p00dB",
            "rs",
            0.0_f64,
            "Watterson Good F1 24 dB + RS",
        ),
    ];

    let mut checks = Vec::new();
    let mut passed = true;

    for (channel, fec, min_delta_success, label) in checks_cfg {
        let Some((base_success, _base_bps, base_n)) =
            mean_for(results, PILOT_DENSITY_BASELINE_MODE, channel, fec)
        else {
            passed = false;
            checks.push(format!("FAIL {label}: missing baseline rows"));
            continue;
        };
        let Some((dense_success, _dense_bps, dense_n)) =
            mean_for(results, PILOT_DENSITY_DENSE_MODE, channel, fec)
        else {
            passed = false;
            checks.push(format!("FAIL {label}: missing dense rows"));
            continue;
        };

        let delta_success = dense_success - base_success;
        let ok = delta_success >= min_delta_success;
        if !ok {
            passed = false;
        }
        checks.push(format!(
            "{} {}: delta_success={:+.2}% (baseline={:.2}%, dense={:.2}%, samples={}/{}) threshold={:+.2}%",
            if ok { "PASS" } else { "FAIL" },
            label,
            delta_success,
            base_success,
            dense_success,
            base_n,
            dense_n,
            min_delta_success,
        ));
    }

    PilotDensityGateResult { passed, checks }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_cross_mode_result(
        family: &str,
        level: CrossModeLevel,
        mode: &str,
        channel: &str,
        measured_bps: f64,
        median_frame_time_ms: u64,
        p95_frame_time_ms: u64,
    ) -> CrossModeBenchResult {
        CrossModeBenchResult {
            family: family.to_string(),
            level,
            result: BenchResult {
                mode: mode.to_string(),
                channel: channel.to_string(),
                fec: "rs".to_string(),
                compression: "none".to_string(),
                payload_len: 128,
                n_frames: 50,
                frames_ok: 50,
                bytes_delivered: 6400,
                total_tx_samples: 8000,
                on_air_s: 1.0,
                success_rate_pct: 100.0,
                measured_bps,
                theoretical_gross_bps: measured_bps,
                efficiency_pct: 100.0,
                median_frame_time_ms,
                p95_frame_time_ms,
            },
        }
    }

    #[test]
    fn cross_mode_case_builder_full_produces_48_cases() {
        let cases = build_cross_mode_cases(128, Tier::Full);
        assert_eq!(cases.len(), 48);

        let scfdma_sl13 = cases.iter().find(|case| {
            case.family == "SCFDMA52"
                && case.level == CrossModeLevel::Sl13
                && case.case.channel.label() == "watterson_good_f2"
        });
        let scfdma_sl13 = scfdma_sl13.expect("missing SCFDMA52 SL13 Watterson Good F2 case");
        assert_eq!(scfdma_sl13.case.mode, "SCFDMA52-16QAM");
        assert_eq!(scfdma_sl13.case.fec_mode, FecMode::Rs);
    }

    #[test]
    fn cross_mode_case_builder_quick_produces_12_cases() {
        let cases = build_cross_mode_cases(128, Tier::Quick);
        assert_eq!(cases.len(), 12);
        assert!(cases
            .iter()
            .all(|case| case.case.channel.label() == "awgn_20dB"));
    }

    #[test]
    fn cross_mode_gate_flags_regression_and_latency_failures() {
        let baseline = vec![
            synthetic_cross_mode_result(
                "SCFDMA52",
                CrossModeLevel::Sl12Baseline,
                "SCFDMA52",
                "awgn_20dB",
                1000.0,
                1100,
                1400,
            ),
            synthetic_cross_mode_result(
                "SCFDMA52",
                CrossModeLevel::Sl13,
                "SCFDMA52-16QAM",
                "awgn_20dB",
                1400.0,
                1200,
                1500,
            ),
            synthetic_cross_mode_result(
                "SCFDMA52",
                CrossModeLevel::Sl14,
                "SCFDMA52-64QAM",
                "awgn_20dB",
                1800.0,
                1300,
                1600,
            ),
        ];

        let current = vec![
            synthetic_cross_mode_result(
                "SCFDMA52",
                CrossModeLevel::Sl12Baseline,
                "SCFDMA52",
                "awgn_20dB",
                980.0,
                1200,
                1500,
            ),
            synthetic_cross_mode_result(
                "SCFDMA52",
                CrossModeLevel::Sl13,
                "SCFDMA52-16QAM",
                "awgn_20dB",
                900.0,
                1600,
                2100,
            ),
            synthetic_cross_mode_result(
                "SCFDMA52",
                CrossModeLevel::Sl14,
                "SCFDMA52-64QAM",
                "awgn_20dB",
                870.0,
                1400,
                1700,
            ),
        ];

        let result = evaluate_cross_mode_consistency_gate(&current, &baseline);
        assert!(!result.passed);
        assert!(result
            .checks
            .iter()
            .any(|check| check.contains("FAIL throughput SCFDMA52 sl13 awgn_20dB")));
        assert!(result
            .checks
            .iter()
            .any(|check| check.contains("FAIL latency SCFDMA52 sl13 awgn_20dB")));
        assert!(result
            .checks
            .iter()
            .any(|check| check.contains("FAIL ladder SCFDMA52 awgn_20dB sl12->sl13")));
    }
}
