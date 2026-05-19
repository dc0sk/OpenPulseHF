//! Integration tests for `transmit_arq` — the multi-attempt ARQ retry loop.
//!
//! Strategy: pre-load the ISS receive buffer (via `LoopbackBackend::push_frame`)
//! with FSK4-ACK encoded frames produced by a separate IRS engine.  Each call to
//! `receive_ack_with_short_fec` inside `transmit_arq` pops one pre-queued frame,
//! letting us exercise the retry logic without needing concurrent threads.

use bpsk_plugin::BpskPlugin;
use fsk4_plugin::Fsk4Plugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::ack::{AckFrame, AckType};
use openpulse_core::error::ModemError;
use openpulse_core::rate::RateEvent;
use openpulse_modem::engine::ModemEngine;

fn make_engine() -> (ModemEngine, LoopbackBackend) {
    let backend = LoopbackBackend::new();
    let mut engine = ModemEngine::new(Box::new(backend.clone_shared()));
    engine.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    engine.register_plugin(Box::new(Fsk4Plugin::new())).unwrap();
    (engine, backend)
}

/// Encode an `AckFrame` via a spare IRS engine and return the raw FSK4 samples.
fn encode_ack(ack_type: AckType) -> Vec<f32> {
    let (mut irs, irs_lb) = make_engine();
    let frame = AckFrame::new(ack_type, "arq-test");
    irs.transmit_ack_with_short_fec(&frame, None)
        .expect("IRS ACK encode should succeed");
    irs_lb.drain_samples()
}

/// `transmit_arq` succeeds immediately when the very first ACK reply is `AckOk`.
#[test]
fn transmit_arq_succeeds_on_first_attempt() {
    let (mut iss, iss_lb) = make_engine();

    iss_lb.push_frame(&encode_ack(AckType::AckOk));

    let rate_event = iss
        .transmit_arq(b"hello arq", "BPSK250", None, 3)
        .expect("transmit_arq should succeed on first attempt");

    assert_eq!(
        rate_event,
        RateEvent::Maintained,
        "no adaptive session → always Maintained"
    );
}

/// `transmit_arq` retries after a Nack and succeeds on the second attempt.
#[test]
fn transmit_arq_retries_on_nack_then_succeeds() {
    let (mut iss, iss_lb) = make_engine();

    // Attempt 1 → Nack; attempt 2 → AckOk.
    iss_lb.push_frame(&encode_ack(AckType::Nack));
    iss_lb.push_frame(&encode_ack(AckType::AckOk));

    let rate_event = iss
        .transmit_arq(b"retry payload", "BPSK250", None, 1)
        .expect("transmit_arq should succeed on second attempt");

    assert_eq!(rate_event, RateEvent::Maintained);
}

/// `transmit_arq` returns `ArqMaxRetries` when every attempt is met with a Nack.
#[test]
fn transmit_arq_returns_max_retries_error_on_persistent_nack() {
    let (mut iss, iss_lb) = make_engine();

    // max_retries = 1 → 2 total attempts; pre-load two Nack frames.
    for _ in 0..2 {
        iss_lb.push_frame(&encode_ack(AckType::Nack));
    }

    let err = iss
        .transmit_arq(b"exhausted payload", "BPSK250", None, 1)
        .expect_err("transmit_arq should fail after exhausting all retries");

    assert!(
        matches!(err, ModemError::ArqMaxRetries(2)),
        "expected ArqMaxRetries(2), got {err:?}"
    );
}

/// `transmit_arq` with `max_retries = 0` makes exactly one attempt and returns
/// the Nack error immediately without retrying.
#[test]
fn transmit_arq_zero_retries_fails_immediately_on_nack() {
    let (mut iss, iss_lb) = make_engine();

    iss_lb.push_frame(&encode_ack(AckType::Nack));

    let err = iss
        .transmit_arq(b"no retry", "BPSK250", None, 0)
        .expect_err("transmit_arq with zero retries should fail on Nack");

    assert!(
        matches!(err, ModemError::ArqMaxRetries(1)),
        "expected ArqMaxRetries(1), got {err:?}"
    );
}
