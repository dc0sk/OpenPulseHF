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
    let mut last_passed = false;

    // Send 6 frames: 3 ACKs step the rate up, 3 NACKs step it back down
    for pass in 0u8..6 {
        let mode = h
            .tx_engine
            .current_adaptive_mode()
            .unwrap_or(&initial_mode)
            .to_string();

        let tx_result = h.tx_engine.transmit(&payload, &mode, None);
        if let Err(e) = tx_result {
            return fail(case, start, format!("TX error on pass {pass}: {e}"));
        }

        h.route(channel.as_mut());

        let rx = h.rx_engine.receive(&mode, None);
        let _ok = rx.is_ok_and(|data| data == payload);

        // First 3 passes: send ACK to step up, last 3: send NACK to step down
        let ack = if pass < 3 {
            AckType::AckUp
        } else {
            AckType::Nack
        };
        let event = h.tx_engine.apply_ack(ack);

        let new_mode = h.tx_engine.current_adaptive_mode().map(|s| s.to_string());
        if new_mode.as_deref() != Some(&initial_mode) {
            mode_changed = true;
        }

        last_passed = true;
        let _ = event;
    }

    let duration_ms = start.elapsed().as_millis() as u64;

    if !last_passed {
        return fail(case, start, "all passes failed".into());
    }

    TestResult {
        case: case.clone(),
        passed: true,
        ber: None,
        bytes_rx: payload.len(),
        duration_ms,
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
        ber: None,
        bytes_rx: 0,
        duration_ms: start.elapsed().as_millis() as u64,
        note: Some(note),
    }
}
