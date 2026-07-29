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
fn receive_populates_last_rx_snr_db() {
    let mut engine = make_engine();

    // Transmit so the loopback backend has samples queued.
    engine.transmit(b"hello", "BPSK100", None).unwrap();
    engine.receive("BPSK100", None).unwrap();

    assert!(
        engine.last_rx_snr_db().is_some(),
        "last_rx_snr_db() should be Some(_) after receive() with a plugin that supports demodulate_soft"
    );
}

/// The same, on a mode with **no** soft path — which is the case the test above cannot reach.
///
/// `record_rx_snr` used to sit inside the `supports_soft_demod` arm, so the predicate gating it was
/// the demodulator's soft capability rather than whether an SNR estimate exists. Those are unrelated:
/// `QPSK250-D` implements `estimate_snr_db` and deliberately reports `supports_soft_demod = false`
/// (differential detection has no calibrated soft path, #923), so every hard-FEC rung — all of
/// `hpx_hf`'s lower half — recorded nothing. Its own assertion message named the gap
/// ("a plugin that supports demodulate_soft") without anyone noticing it was one
/// (archetype scan 2026-07-29, finding 10).
///
/// Consumers this starved: the QSY frequency scan, which scored every candidate channel on
/// `unwrap_or(0.0)`, and the ADIF logbook's `rx_snr` field.
#[test]
fn receive_populates_last_rx_snr_db_on_a_hard_only_mode() {
    let mut engine = make_engine();
    engine
        .register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))
        .unwrap();

    const MODE: &str = "QPSK250-D";
    // Anti-vacuity: if this mode ever gains a soft path it takes the OTHER branch and this test
    // silently stops covering the one it exists for.
    assert!(
        !openpulse_core::plugin::ModulationPlugin::supports_soft_demod(
            &qpsk_plugin::QpskPlugin::new(),
            MODE
        ),
        "{MODE} is expected to be hard-only; if it gained a soft path this test no longer covers \
         the branch it exists for and needs a different mode"
    );

    engine.transmit(b"hello", MODE, None).unwrap();
    engine.receive(MODE, None).unwrap();

    assert!(
        engine.last_rx_snr_db().is_some(),
        "last_rx_snr_db() is None after receiving {MODE}. RX SNR must be recorded whenever the mode \
         can estimate it, not only when the demodulator happens to emit soft decisions."
    );
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

#[test]
fn emits_dcd_change() {
    let mut engine = make_engine();

    // Transmit to put samples in the loopback buffer.
    engine.transmit(b"dcd test", "BPSK100", None).unwrap();

    let mut rx = engine.subscribe();
    // receive() drives DCD update; a non-empty signal should flip DCD to busy.
    engine.receive("BPSK100", None).unwrap();

    let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    let has_dcd = events
        .iter()
        .any(|e| matches!(e, EngineEvent::DcdChange { .. }));
    assert!(has_dcd, "no DcdChange event; got: {events:?}");
}

#[test]
fn emits_afc_update() {
    let mut engine = make_engine();

    // Transmit to put samples in the loopback buffer.
    engine.transmit(b"afc test", "BPSK100", None).unwrap();

    let mut rx = engine.subscribe();
    engine.receive("BPSK100", None).unwrap();

    let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    let has_afc = events
        .iter()
        .any(|e| matches!(e, EngineEvent::AfcUpdate { .. }));
    assert!(has_afc, "no AfcUpdate event; got: {events:?}");
}
