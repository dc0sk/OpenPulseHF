use openpulse_audio::LoopbackBackend;
use openpulse_core::ack::AckType;
use openpulse_core::hpx::HpxEvent;
use openpulse_core::profile::SessionProfile;
use openpulse_core::trust::{CertificateSource, PolicyProfile, PublicKeyTrustLevel, SigningMode};
use openpulse_modem::engine::SecureSessionParams;
use openpulse_modem::{EngineEvent, ModemEngine};

fn make_engine() -> ModemEngine {
    use bpsk_plugin::BpskPlugin;
    let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
    engine.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    engine
}

#[test]
fn emits_frame_transmitted() {
    let mut engine = make_engine();
    let mut rx = engine.subscribe();

    engine.transmit(b"hello", "BPSK100", None).unwrap();

    let event = rx.try_recv().expect("expected FrameTransmitted event");
    assert!(
        matches!(event, EngineEvent::FrameTransmitted { ref mode, .. } if mode == "BPSK100"),
        "unexpected event: {event:?}"
    );
}

#[test]
fn emits_frame_received() {
    let mut engine = make_engine();

    // Transmit first so the loopback backend has samples queued for receive.
    engine.transmit(b"world", "BPSK100", None).unwrap();

    let mut rx = engine.subscribe();
    engine.receive("BPSK100", None).unwrap();

    let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    let has_received = events
        .iter()
        .any(|e| matches!(e, EngineEvent::FrameReceived { mode, .. } if mode == "BPSK100"));
    assert!(has_received, "no FrameReceived event; got: {events:?}");
}

#[test]
fn emits_hpx_transition() {
    let mut engine = make_engine();
    let mut rx = engine.subscribe();

    engine.hpx_apply_event(HpxEvent::StartSession, 0).unwrap();

    let event = rx.try_recv().expect("expected HpxTransition event");
    assert!(
        matches!(
            event,
            EngineEvent::HpxTransition {
                event: HpxEvent::StartSession,
                ..
            }
        ),
        "unexpected event: {event:?}"
    );
}

#[test]
fn emits_rate_change() {
    let mut engine = make_engine();
    engine.start_adaptive_session(SessionProfile::hpx500());

    let mut rx = engine.subscribe();
    engine.apply_ack(AckType::AckOk);

    let event = rx.try_recv().expect("expected RateChange event");
    assert!(
        matches!(event, EngineEvent::RateChange { .. }),
        "unexpected event: {event:?}"
    );
}

#[test]
fn emits_session_started() {
    let mut engine = make_engine();
    let mut rx = engine.subscribe();

    let params = SecureSessionParams {
        local_minimum_mode: SigningMode::Normal,
        peer_supported_modes: vec![SigningMode::Normal],
        key_trust: PublicKeyTrustLevel::Full,
        certificate_source: CertificateSource::OutOfBand,
        psk_validated: false,
    };
    engine.set_trust_policy_profile(PolicyProfile::Permissive);
    engine.begin_secure_session(params, 0).unwrap();

    let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    let has_started = events
        .iter()
        .any(|e| matches!(e, EngineEvent::SessionStarted { .. }));
    assert!(has_started, "no SessionStarted event; got: {events:?}");
}
