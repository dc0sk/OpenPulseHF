use openpulse_core::compression::CompressionAlgorithm;

use crate::channels::channel_suite;
use crate::matrix::{ChannelSpec, TestCase, Tier, UseCase};

const FAST_MODES: &[&str] = &[
    "BPSK250", "QPSK125", "QPSK250", "QPSK500", "QPSK1000", "8PSK500", "8PSK1000",
];
const SLOW_MODES: &[&str] = &["BPSK31", "BPSK63", "BPSK100"];

/// Build all test cases for the given tier.
pub fn build_cases(tier: Tier) -> Vec<TestCase> {
    let channels = channel_suite(tier);
    let mut cases = Vec::new();

    // — Smoke tests: all modes × clean × no FEC × {no comp, lz4} × 32 bytes ——————
    let smoke_modes: Vec<&str> = SLOW_MODES
        .iter()
        .chain(FAST_MODES.iter())
        .copied()
        .collect();
    for mode in &smoke_modes {
        for &compression in &[CompressionAlgorithm::None, CompressionAlgorithm::Lz4] {
            cases.push(raw_case(
                mode,
                false,
                compression,
                ChannelSpec::Clean,
                32,
                tier,
            ));
        }
    }

    // FSK4-ACK smoke: fixed 5-byte payload, no FEC, no compression, all channels
    for channel in &channels {
        cases.push(raw_case(
            "FSK4-ACK",
            false,
            CompressionAlgorithm::None,
            channel.clone(),
            5,
            tier,
        ));
    }

    // — Fast-mode AWGN matrix: fast modes × AWGN channels × {no FEC, FEC} × 128 bytes
    let awgn_channels: Vec<_> = channels
        .iter()
        .filter(|c| c.is_awgn_family())
        .cloned()
        .collect();
    for mode in FAST_MODES {
        for channel in &awgn_channels {
            for &fec in &[false, true] {
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

    // — Adaptive: clean + AWGN channels ——————————————————————————————————————————
    for channel in &awgn_channels {
        cases.push(TestCase {
            use_case: UseCase::AdaptiveHpx500,
            mode: "HPX500".into(),
            fec: false,
            compression: CompressionAlgorithm::None,
            channel: channel.clone(),
            payload_len: 64,
            tier,
        });
        cases.push(TestCase {
            use_case: UseCase::AdaptiveHpx2300,
            mode: "HPX2300".into(),
            fec: false,
            compression: CompressionAlgorithm::None,
            channel: channel.clone(),
            payload_len: 64,
            tier,
        });
    }

    // — ARDOP protocol loopback (BPSK250, clean only) ————————————————————————————
    cases.push(TestCase {
        use_case: UseCase::Ardop,
        mode: "BPSK250".into(),
        fec: false,
        compression: CompressionAlgorithm::None,
        channel: ChannelSpec::Clean,
        payload_len: 64,
        tier,
    });

    // — KISS protocol loopback (BPSK250, clean only) —————————————————————————————
    cases.push(TestCase {
        use_case: UseCase::Kiss,
        mode: "BPSK250".into(),
        fec: false,
        compression: CompressionAlgorithm::None,
        channel: ChannelSpec::Clean,
        payload_len: 64,
        tier,
    });

    // — B2F end-to-end: BPSK250 through channel —————————————————————————————————
    for channel in &awgn_channels {
        cases.push(TestCase {
            use_case: UseCase::B2f,
            mode: "BPSK250".into(),
            fec: false,
            compression: CompressionAlgorithm::None,
            channel: channel.clone(),
            payload_len: 64,
            tier,
        });
    }

    // — Full-tier additions ——————————————————————————————————————————————————————
    if tier == Tier::Full {
        // Slow modes × AWGN × FEC × 32 bytes
        for mode in SLOW_MODES {
            for channel in &awgn_channels {
                cases.push(raw_case(
                    mode,
                    true,
                    CompressionAlgorithm::None,
                    channel.clone(),
                    32,
                    tier,
                ));
            }
        }
        // Fast modes × propagation channels × FEC × 32 bytes
        let prop_channels: Vec<_> = channels
            .iter()
            .filter(|c| !c.is_awgn_family())
            .cloned()
            .collect();
        for mode in FAST_MODES {
            for channel in &prop_channels {
                cases.push(raw_case(
                    mode,
                    true,
                    CompressionAlgorithm::None,
                    channel.clone(),
                    32,
                    tier,
                ));
                cases.push(raw_case(
                    mode,
                    false,
                    CompressionAlgorithm::None,
                    channel.clone(),
                    32,
                    tier,
                ));
            }
        }
        // Fast modes × propagation channels × compression × 128 bytes
        for mode in FAST_MODES {
            for channel in &awgn_channels {
                cases.push(raw_case(
                    mode,
                    true,
                    CompressionAlgorithm::Lz4,
                    channel.clone(),
                    128,
                    tier,
                ));
            }
        }
    }

    cases
}

fn raw_case(
    mode: &str,
    fec: bool,
    compression: CompressionAlgorithm,
    channel: ChannelSpec,
    payload_len: usize,
    tier: Tier,
) -> TestCase {
    TestCase {
        use_case: UseCase::RawModem,
        mode: mode.to_string(),
        fec,
        compression,
        channel,
        payload_len,
        tier,
    }
}
