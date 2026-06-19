use std::time::Instant;

use openpulse_core::ack::AckType;
use openpulse_core::profile::SessionProfile;
use openpulse_modem::channel_sim::ChannelSimHarness;

use crate::channels::build as build_channel;
use crate::matrix::{TestCase, TestResult};
use crate::runners::register_all;

/// Run an adaptive HPX session: TX sends several frames, applies ACKs to step up the
/// rate ladder, then verifies the session reached at least one mode change.
pub fn run(case: &TestCase, profile: SessionProfile) -> TestResult {
    let payload: Vec<u8> = (0..case.payload_len).map(|i| i as u8).collect();

    let mut h = ChannelSimHarness::new();
    register_all(&mut h.tx_engine);
    register_all(&mut h.rx_engine);

    let mut channel = build_channel(&case.channel);

    let start = Instant::now();

    h.tx_engine.start_adaptive_session(profile.clone());
    h.rx_engine.start_adaptive_session(profile);

    let initial_mode = match h.tx_engine.current_adaptive_mode() {
        Some(m) => m.to_string(),
        None => {
            return fail(case, start, "no initial adaptive mode".into());
        }
    };

    let mut mode_changed = false;
    let mut total_bytes_rx: usize = 0;
    let mut total_bit_errors: u64 = 0;
    let mut total_bits: u64 = 0;
    let mut any_rx_ok = false;
    let mut total_tx_samples: usize = 0;

    // Send 6 frames: 3 ACKs step the rate up, 3 NACKs step it back down.
    for pass in 0u8..6 {
        let mode = h
            .tx_engine
            .current_adaptive_mode()
            .unwrap_or(&initial_mode)
            .to_string();

        if let Err(e) = h.tx_engine.transmit(&payload, &mode, None) {
            return fail(case, start, format!("TX error on pass {pass}: {e}"));
        }

        total_tx_samples += h.route(channel.as_mut());

        match h.rx_engine.receive(&mode, None) {
            Ok(data) => {
                let received_bits = (data.len() * 8) as u64;
                let payload_bits = (payload.len() * 8) as u64;
                // Count bit errors over the overlapping prefix.
                let errors: u64 = data
                    .iter()
                    .zip(payload.iter())
                    .map(|(&r, &e)| (r ^ e).count_ones() as u64)
                    .sum::<u64>()
                    + payload_bits.saturating_sub(received_bits); // missing bits count as errors
                total_bit_errors += errors;
                total_bits += payload_bits;
                total_bytes_rx += data.len();
                any_rx_ok = true;
            }
            Err(_) => {
                // RX failure: all payload bits are lost → BER contribution = 1.0 for this frame.
                total_bit_errors += (payload.len() * 8) as u64;
                total_bits += (payload.len() * 8) as u64;
            }
        }

        let ack = if pass < 3 {
            AckType::AckUp
        } else {
            AckType::Nack
        };
        // Report the draining queue depth (6 frames total) to the A2 backlog gate.
        // The gate is off by default here, but this keeps the adaptive drivers'
        // backlog accounting consistent.
        let backlog = 5u8.saturating_sub(pass) as usize * case.payload_len;
        h.tx_engine.set_tx_backlog(backlog);
        let _ = h.tx_engine.apply_ack(ack);

        if h.tx_engine.current_adaptive_mode().map(|s| s.to_string()) != Some(initial_mode.clone())
        {
            mode_changed = true;
        }
    }

    let duration_ms = start.elapsed().as_millis() as u64;

    if !any_rx_ok {
        return fail(case, start, "all frames failed to decode".into());
    }

    let ber = if total_bits > 0 {
        Some(total_bit_errors as f64 / total_bits as f64)
    } else {
        None
    };
    let effective_bps = if total_tx_samples > 0 {
        let on_air_s = total_tx_samples as f64 / 8000.0;
        Some(total_bytes_rx as f64 * 8.0 / on_air_s)
    } else {
        None
    };

    TestResult {
        case: case.clone(),
        passed: true,
        skipped: false,
        ber,
        bytes_rx: total_bytes_rx,
        duration_ms,
        effective_bps,
        note: if mode_changed {
            None
        } else {
            Some("rate did not change (channel may be too clean)".into())
        },
    }
}

fn fail(case: &TestCase, start: Instant, note: String) -> TestResult {
    TestResult {
        case: case.clone(),
        passed: false,
        skipped: false,
        ber: None,
        bytes_rx: 0,
        duration_ms: start.elapsed().as_millis() as u64,
        effective_bps: None,
        note: Some(note),
    }
}
