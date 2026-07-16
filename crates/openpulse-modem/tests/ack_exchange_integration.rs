//! Integration tests for the full ACK exchange loop.
//!
//! Verifies `receive_with_ack_hint` → `transmit_ack_with_short_fec` →
//! `receive_ack_with_short_fec` → `apply_ack_frame` round-trip at the
//! engine level.

use bpsk_plugin::BpskPlugin;
use fsk4_plugin::Fsk4Plugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::ack::{AckFrame, AckType};
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::RateEvent;
use openpulse_modem::engine::ModemEngine;

fn make_engine() -> (ModemEngine, LoopbackBackend) {
    let backend = LoopbackBackend::new();
    let mut engine = ModemEngine::new(Box::new(backend.clone_shared()));
    engine.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    engine.register_plugin(Box::new(Fsk4Plugin::new())).unwrap();
    (engine, backend)
}

/// Route samples from `src` loopback into `dst` loopback (clean, no channel distortion).
fn route(src: &LoopbackBackend, dst: &LoopbackBackend) {
    dst.fill_samples(&src.drain_samples());
}

/// `receive_with_ack_hint` decodes the same payload as `receive` on a clean channel.
#[test]
fn receive_with_ack_hint_returns_correct_payload() {
    let payload = b"hello ack exchange";

    let (mut iss, iss_lb) = make_engine();
    let (mut irs, irs_lb) = make_engine();

    iss.transmit(payload, "BPSK250", None).unwrap();
    route(&iss_lb, &irs_lb);

    let (rx_payload, _ack_type) = irs
        .receive_with_ack_hint("BPSK250", None)
        .expect("IRS receive_with_ack_hint should succeed on clean channel");

    assert_eq!(&rx_payload, payload);
}

/// Without an adaptive session the returned ACK type is always `AckOk`.
#[test]
fn receive_with_ack_hint_returns_ack_ok_without_session() {
    let payload = b"no session ack ok";

    let (mut iss, iss_lb) = make_engine();
    let (mut irs, irs_lb) = make_engine();

    iss.transmit(payload, "BPSK250", None).unwrap();
    route(&iss_lb, &irs_lb);

    let (_rx, ack_type) = irs
        .receive_with_ack_hint("BPSK250", None)
        .expect("receive should succeed");

    assert_eq!(
        ack_type,
        AckType::AckOk,
        "no adaptive session → AckOk always"
    );
}

/// E7: with a shared session ACK-MAC key, an authenticated ACK round-trips; an ACK from a peer with a
/// different key (a forger) is rejected by the receiver's keyed verification.
#[test]
fn authenticated_ack_round_trips_and_forgery_is_rejected() {
    let key = [0x5Au8; 32];
    let (mut iss, iss_lb) = make_engine();
    let (mut irs, irs_lb) = make_engine();
    iss.set_ack_mac_key(Some(key));
    irs.set_ack_mac_key(Some(key));
    assert!(iss.has_ack_mac_key());

    // IRS → ISS: a rate-down recommendation, authenticated with the shared key.
    let ack = AckFrame::new(AckType::AckDown, "sess-e7");
    irs.transmit_ack_with_short_fec(&ack, None)
        .expect("authenticated ACK transmit");
    route(&irs_lb, &iss_lb);
    let got = iss
        .receive_ack_with_short_fec(None)
        .expect("ISS verifies and accepts the authenticated ACK");
    assert_eq!(got.ack_type, AckType::AckDown);

    // A forger with the wrong key transmits an ACK; the ISS (real key) must reject it.
    let (mut forger, forger_lb) = make_engine();
    forger.set_ack_mac_key(Some([0x11u8; 32]));
    let forged = AckFrame::new(AckType::Nack, "sess-e7");
    forger
        .transmit_ack_with_short_fec(&forged, None)
        .expect("forger transmit");
    route(&forger_lb, &iss_lb);
    assert!(
        iss.receive_ack_with_short_fec(None).is_err(),
        "an ACK under a different session key must fail keyed verification"
    );
}

/// Full round-trip: ISS transmits data; IRS receives and replies with an ACK
/// frame; ISS receives and processes the ACK frame without error.
#[test]
fn full_ack_exchange_round_trip_no_session() {
    let payload = b"full ack round trip";
    let session_id = "test-session-001";

    let (mut iss, iss_lb) = make_engine();
    let (mut irs, irs_lb) = make_engine();

    // ISS → IRS (data)
    iss.transmit(payload, "BPSK250", None).unwrap();
    route(&iss_lb, &irs_lb);

    let (rx_payload, ack_type) = irs
        .receive_with_ack_hint("BPSK250", None)
        .expect("IRS receive should succeed");
    assert_eq!(&rx_payload, payload);

    // IRS → ISS (ACK)
    let ack_frame = AckFrame::new(ack_type, session_id);
    irs.transmit_ack_with_short_fec(&ack_frame, None)
        .expect("IRS ACK transmit should succeed");
    route(&irs_lb, &iss_lb);

    let received_ack = iss
        .receive_ack_with_short_fec(None)
        .expect("ISS should receive the ACK frame");

    assert_eq!(received_ack.ack_type, ack_type);

    // Processing the ACK (no session → always Maintained)
    let rate_event = iss.apply_ack_frame(&received_ack);
    assert_eq!(rate_event, RateEvent::Maintained);
}

/// With an active adaptive session an `AckUp` from the IRS causes the ISS to
/// step up its TX speed level.
#[test]
fn ack_up_from_irs_increases_iss_tx_speed() {
    let payload = b"adaptive ack up test";
    let session_id = "test-session-adaptive";

    let (mut iss, iss_lb) = make_engine();
    let (mut irs, irs_lb) = make_engine();

    // Start adaptive sessions on both ends (profile gives SNR thresholds).
    iss.start_adaptive_session(SessionProfile::hpx500());
    irs.start_adaptive_session(SessionProfile::hpx500());

    let initial_level = iss
        .current_tx_level()
        .expect("adaptive session should be active");

    // ISS → IRS (data)
    iss.transmit(payload, "BPSK250", None).unwrap();
    route(&iss_lb, &irs_lb);

    let (_rx, _ack_type) = irs
        .receive_with_ack_hint("BPSK250", None)
        .expect("IRS receive should succeed");

    // Force AckUp (as if IRS observed excellent SNR) and send it back.
    let ack_frame = AckFrame::new(AckType::AckUp, session_id);
    irs.transmit_ack_with_short_fec(&ack_frame, None)
        .expect("IRS ACK transmit should succeed");
    route(&irs_lb, &iss_lb);

    let received_ack = iss
        .receive_ack_with_short_fec(None)
        .expect("ISS should receive the AckUp frame");
    assert_eq!(received_ack.ack_type, AckType::AckUp);

    // prime the upgrade candidate on the ISS TX adapter so AckUp is admitted.
    let ceiling = SessionProfile::hpx500()
        .snr_ceiling_for_level(initial_level)
        .unwrap_or(f32::INFINITY);
    iss.apply_snr_hint(ceiling + 5.0);

    let rate_event = iss.apply_ack_frame(&received_ack);
    assert!(
        matches!(rate_event, RateEvent::Increased(_)),
        "AckUp after SNR ceiling hint should increase the TX speed level; got {rate_event:?}"
    );
    assert!(
        iss.current_tx_level() > Some(initial_level),
        "ISS TX level should have risen after AckUp"
    );
}

/// `AckDown` from the IRS decreases the ISS TX speed level.
#[test]
fn ack_down_from_irs_decreases_iss_tx_speed() {
    let payload = b"ack down test";
    let session_id = "test-session-ack-down";

    let (mut iss, iss_lb) = make_engine();
    let (mut irs, irs_lb) = make_engine();

    iss.start_adaptive_session(SessionProfile::hpx500());
    // Step ISS up so there is room to step down.
    let _ = iss.apply_ack(AckType::AckUp);
    let _ = iss.apply_ack(AckType::AckUp);
    let level_before = iss
        .current_tx_level()
        .expect("adaptive session should be active");

    iss.transmit(payload, "BPSK250", None).unwrap();
    route(&iss_lb, &irs_lb);

    // IRS receives data (no session needed; ack type ignored here)
    let _ = irs
        .receive("BPSK250", None)
        .expect("IRS receive should succeed");

    // IRS forcibly sends AckDown.
    let ack_frame = AckFrame::new(AckType::AckDown, session_id);
    irs.transmit_ack_with_short_fec(&ack_frame, None)
        .expect("IRS ACK transmit should succeed");
    route(&irs_lb, &iss_lb);

    let received_ack = iss
        .receive_ack_with_short_fec(None)
        .expect("ISS should receive the AckDown frame");
    assert_eq!(received_ack.ack_type, AckType::AckDown);

    let rate_event = iss.apply_ack_frame(&received_ack);
    assert!(
        matches!(
            rate_event,
            RateEvent::Decreased(_) | RateEvent::ChirpFallback
        ),
        "AckDown should decrease ISS TX speed; got {rate_event:?}"
    );
    assert!(
        iss.current_tx_level() < Some(level_before),
        "ISS TX level should have fallen after AckDown"
    );
}
