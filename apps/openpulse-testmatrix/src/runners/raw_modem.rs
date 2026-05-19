use std::time::Instant;

use openpulse_core::compression::{compress_if_smaller, decompress, CompressionAlgorithm};
use openpulse_core::fec::FecMode;
use openpulse_modem::channel_sim::ChannelSimHarness;

use crate::channels::build as build_channel;
use crate::matrix::{TestCase, TestResult};
use crate::runners::register_all;

pub fn run(case: &TestCase) -> TestResult {
    let mode = &case.mode;
    let payload: Vec<u8> = (0..case.payload_len).map(|i| i as u8).collect();

    let mut h = ChannelSimHarness::new();
    register_all(&mut h.tx_engine);
    register_all(&mut h.rx_engine);

    let mut channel = build_channel(&case.channel);

    let start = Instant::now();

    // Apply compression if requested
    let (wire_payload, actual_algo) = match case.compression {
        CompressionAlgorithm::None => (payload.clone(), CompressionAlgorithm::None),
        CompressionAlgorithm::Lz4 | CompressionAlgorithm::Zstd(_) => compress_if_smaller(&payload),
    };

    // Transmit — dispatch on FecMode
    let tx_result = match case.fec_mode {
        FecMode::None => h.tx_engine.transmit(&wire_payload, mode, None),
        FecMode::Rs => h.tx_engine.transmit_with_fec(&wire_payload, mode, None),
        FecMode::RsInterleaved => {
            h.tx_engine
                .transmit_with_fec_interleaved(&wire_payload, mode, None, 5)
        }
        FecMode::Concatenated => {
            h.tx_engine
                .transmit_with_concatenated_fec(&wire_payload, mode, None)
        }
        FecMode::ShortRs => {
            // ShortRs is for ACK frames only — skip with a note
            return skip(
                case,
                "ShortRs is ACK-frame-only; not applicable to raw modem",
            );
        }
        FecMode::RsStrong => h
            .tx_engine
            .transmit_with_strong_fec(&wire_payload, mode, None),
        FecMode::SoftConcatenated => {
            h.tx_engine
                .transmit_with_soft_viterbi_fec(&wire_payload, mode, None)
        }
        FecMode::Ldpc => {
            // Single-block limit: user payload + ~8 bytes frame header must fit in LDPC_MAX_INFO_BYTES (128).
            if wire_payload.len() > 112 {
                return skip(
                    case,
                    "LDPC payload too large for single block (max 112 bytes user data; LDPC_MAX_INFO_BYTES=128)",
                );
            }
            h.tx_engine.transmit_with_ldpc(&wire_payload, mode, None)
        }
    };

    if let Err(e) = tx_result {
        return fail(case, 0, start, format!("TX error: {e}"));
    }

    // Route samples through channel model; capture TX sample count for on-air duration.
    let tx_samples = h.route(channel.as_mut());

    // Receive — dispatch on FecMode
    let rx_raw = match case.fec_mode {
        FecMode::None => h.rx_engine.receive(mode, None),
        FecMode::Rs => h.rx_engine.receive_with_fec(mode, None),
        FecMode::RsInterleaved => h.rx_engine.receive_with_fec_interleaved(mode, None, 5),
        FecMode::Concatenated => h.rx_engine.receive_with_concatenated_fec(mode, None),
        FecMode::ShortRs => unreachable!("skipped in TX branch"),
        FecMode::Ldpc => h.rx_engine.receive_with_ldpc(mode, None),
        FecMode::RsStrong => h.rx_engine.receive_with_strong_fec(mode, None),
        FecMode::SoftConcatenated => h.rx_engine.receive_with_soft_viterbi_fec(mode, None),
    };

    let rx_raw = match rx_raw {
        Ok(r) => r,
        Err(e) => return fail(case, 0, start, format!("RX error: {e}")),
    };

    // Decompress
    let rx_data = match decompress(&rx_raw, actual_algo) {
        Ok(d) => d,
        Err(e) => return fail(case, rx_raw.len(), start, format!("decompress error: {e}")),
    };

    let duration_ms = start.elapsed().as_millis() as u64;
    let bytes_rx = rx_data.len();
    // On-air duration = TX samples / sample rate; wall-clock time is not meaningful in simulation.
    let effective_bps = if tx_samples > 0 {
        let on_air_s = tx_samples as f64 / 8000.0;
        Some(payload.len() as f64 * 8.0 / on_air_s)
    } else {
        None
    };

    if rx_data != payload {
        return TestResult {
            case: case.clone(),
            passed: false,
            skipped: false,
            ber: Some(byte_ber(&payload, &rx_data)),
            bytes_rx,
            duration_ms,
            effective_bps,
            note: Some("payload mismatch".into()),
        };
    }

    TestResult {
        case: case.clone(),
        passed: true,
        skipped: false,
        ber: Some(0.0),
        bytes_rx,
        duration_ms,
        effective_bps,
        note: None,
    }
}

fn fail(case: &TestCase, bytes_rx: usize, start: Instant, note: String) -> TestResult {
    let duration_ms = start.elapsed().as_millis() as u64;
    TestResult {
        case: case.clone(),
        passed: false,
        skipped: false,
        ber: None,
        bytes_rx,
        duration_ms,
        effective_bps: None,
        note: Some(note),
    }
}

fn skip(case: &TestCase, note: &str) -> TestResult {
    TestResult {
        case: case.clone(),
        passed: false,
        skipped: true,
        ber: None,
        bytes_rx: 0,
        duration_ms: 0,
        effective_bps: None,
        note: Some(format!("SKIP: {note}")),
    }
}

fn byte_ber(expected: &[u8], actual: &[u8]) -> f64 {
    let len = expected.len().min(actual.len());
    if len == 0 {
        return 1.0;
    }
    let bit_errors: u32 = expected[..len]
        .iter()
        .zip(actual[..len].iter())
        .map(|(a, b)| (a ^ b).count_ones())
        .sum();
    bit_errors as f64 / (len * 8) as f64
}
