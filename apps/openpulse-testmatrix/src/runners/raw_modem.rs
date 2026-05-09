use std::time::Instant;

use openpulse_core::compression::{compress_if_smaller, decompress, CompressionAlgorithm};
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

    // Transmit
    let tx_result = if case.fec {
        h.tx_engine
            .transmit_with_fec_interleaved(&wire_payload, mode, None, 5)
    } else {
        h.tx_engine.transmit(&wire_payload, mode, None)
    };
    if let Err(e) = tx_result {
        return fail(case, 0, start, format!("TX error: {e}"));
    }

    // Route samples through channel model
    h.route(channel.as_mut());

    // Receive
    let rx_raw = if case.fec {
        h.rx_engine.receive_with_fec_interleaved(mode, None, 5)
    } else {
        h.rx_engine.receive(mode, None)
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

    if rx_data != payload {
        return TestResult {
            case: case.clone(),
            passed: false,
            ber: Some(byte_ber(&payload, &rx_data)),
            bytes_rx,
            duration_ms,
            note: Some("payload mismatch".into()),
        };
    }

    TestResult {
        case: case.clone(),
        passed: true,
        ber: Some(0.0),
        bytes_rx,
        duration_ms,
        note: None,
    }
}

fn fail(case: &TestCase, bytes_rx: usize, start: Instant, note: String) -> TestResult {
    TestResult {
        case: case.clone(),
        passed: false,
        ber: None,
        bytes_rx,
        duration_ms: start.elapsed().as_millis() as u64,
        note: Some(note),
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
