//! HPX session-state conformance tests at modem integration layer.

use openpulse_audio::LoopbackBackend;
use openpulse_core::hpx::{HpxEvent, HpxReasonCode, HpxState};
use openpulse_core::trust::{CertificateSource, PublicKeyTrustLevel, SigningMode};
use openpulse_modem::engine::SecureSessionParams;
use openpulse_modem::ModemEngine;

fn make_engine() -> ModemEngine {
    ModemEngine::new(Box::new(LoopbackBackend::new()))
}

fn start_active_transfer(engine: &mut ModemEngine, timestamp_ms: u64) {
    engine
        .begin_secure_session(
            SecureSessionParams {
                local_minimum_mode: SigningMode::Normal,
                peer_supported_modes: vec![SigningMode::Normal, SigningMode::Psk],
                key_trust: PublicKeyTrustLevel::Full,
                certificate_source: CertificateSource::OutOfBand,
                psk_validated: false,
            },
            timestamp_ms,
        )
        .expect("secure session should enter active transfer");
}

#[test]
fn conformance_happy_path_idle_to_idle() {
    let mut engine = make_engine();

    engine
        .hpx_apply_event(HpxEvent::StartSession, 1_000)
        .unwrap();
    engine
        .hpx_apply_event(HpxEvent::DiscoveryOk, 1_001)
        .unwrap();
    engine.hpx_apply_event(HpxEvent::TrainingOk, 1_002).unwrap();
    engine
        .hpx_apply_event(HpxEvent::TransferComplete, 1_003)
        .unwrap();
    engine
        .hpx_apply_event(HpxEvent::TransferComplete, 1_004)
        .unwrap();

    assert_eq!(engine.hpx_state(), HpxState::Idle);
}

#[test]
fn conformance_discovery_timeout_fails() {
    let mut engine = make_engine();

    engine
        .hpx_apply_event(HpxEvent::StartSession, 2_000)
        .unwrap();
    let transition = engine
        .hpx_apply_event(HpxEvent::DiscoveryTimeout, 8_000)
        .unwrap();

    assert_eq!(transition.reason_code, HpxReasonCode::Timeout);
    assert_eq!(engine.hpx_state(), HpxState::Failed);
}

#[test]
fn conformance_training_timeout_fails() {
    let mut engine = make_engine();

    engine
        .hpx_apply_event(HpxEvent::StartSession, 3_000)
        .unwrap();
    engine
        .hpx_apply_event(HpxEvent::DiscoveryOk, 3_001)
        .unwrap();
    let transition = engine
        .hpx_apply_event(HpxEvent::TrainingTimeout, 13_001)
        .unwrap();

    assert_eq!(transition.reason_code, HpxReasonCode::Timeout);
    assert_eq!(engine.hpx_state(), HpxState::Failed);
}

#[test]
fn conformance_signature_rejection_during_discovery_fails() {
    let mut engine = make_engine();

    engine
        .hpx_apply_event(HpxEvent::StartSession, 4_000)
        .unwrap();
    let transition = engine
        .hpx_apply_event(HpxEvent::SignatureVerificationFailed, 4_001)
        .unwrap();

    assert_eq!(transition.reason_code, HpxReasonCode::SignatureFailure);
    assert_eq!(engine.hpx_state(), HpxState::Failed);
}

#[test]
fn conformance_signature_rejection_during_active_transfer_enters_recovery() {
    let mut engine = make_engine();

    start_active_transfer(&mut engine, 5_000);
    let transition = engine
        .hpx_apply_event(HpxEvent::SignatureVerificationFailed, 5_010)
        .unwrap();

    assert_eq!(transition.reason_code, HpxReasonCode::SignatureFailure);
    assert_eq!(engine.hpx_state(), HpxState::Recovery);
}

#[test]
fn conformance_quality_drop_then_recovery_returns_to_active_transfer() {
    let mut engine = make_engine();

    start_active_transfer(&mut engine, 6_000);
    engine
        .hpx_apply_event(HpxEvent::QualityDrop, 6_010)
        .unwrap();
    engine.hpx_apply_event(HpxEvent::RecoveryOk, 6_020).unwrap();

    assert_eq!(engine.hpx_state(), HpxState::ActiveTransfer);
}

#[test]
fn conformance_recovery_exhaustion_fails_after_four_attempts() {
    let mut engine = make_engine();

    start_active_transfer(&mut engine, 7_000);
    for i in 0..4 {
        engine
            .hpx_apply_event(HpxEvent::QualityDrop, 7_010 + i)
            .unwrap();
        engine
            .hpx_apply_event(HpxEvent::RecoveryOk, 7_020 + i)
            .unwrap();
    }

    let transition = engine
        .hpx_apply_event(HpxEvent::QualityDrop, 7_100)
        .unwrap();

    assert_eq!(
        transition.reason_code,
        HpxReasonCode::RecoveryAttemptsExhausted
    );
    assert_eq!(engine.hpx_state(), HpxState::Failed);
}

#[test]
fn conformance_local_cancel_moves_to_teardown_then_idle() {
    let mut engine = make_engine();

    engine
        .hpx_apply_event(HpxEvent::StartSession, 8_000)
        .unwrap();
    engine
        .hpx_apply_event(HpxEvent::DiscoveryOk, 8_001)
        .unwrap();
    engine
        .hpx_apply_event(HpxEvent::LocalCancel, 8_002)
        .unwrap();
    assert_eq!(engine.hpx_state(), HpxState::Teardown);

    engine
        .hpx_apply_event(HpxEvent::TransferComplete, 8_003)
        .unwrap();
    assert_eq!(engine.hpx_state(), HpxState::Idle);
}

#[test]
fn conformance_remote_teardown_from_active_transfer_moves_to_teardown() {
    let mut engine = make_engine();

    start_active_transfer(&mut engine, 9_000);
    engine
        .hpx_apply_event(HpxEvent::RemoteTeardown, 9_010)
        .unwrap();

    assert_eq!(engine.hpx_state(), HpxState::Teardown);
}

#[test]
fn conformance_relay_activation_path_reaches_active_transfer() {
    let mut engine = make_engine();

    engine
        .hpx_apply_event(HpxEvent::StartSession, 10_000)
        .unwrap();
    engine
        .hpx_apply_event(HpxEvent::DiscoveryOk, 10_001)
        .unwrap();
    engine
        .hpx_apply_event(HpxEvent::RelayRouteFound, 10_002)
        .unwrap();
    assert_eq!(engine.hpx_state(), HpxState::RelayActive);

    engine
        .hpx_apply_event(HpxEvent::TrainingOk, 10_003)
        .unwrap();
    assert_eq!(engine.hpx_state(), HpxState::ActiveTransfer);
}
