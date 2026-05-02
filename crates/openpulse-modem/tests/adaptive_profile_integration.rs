use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::ack::AckType;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::{RateEvent, SpeedLevel};
use openpulse_modem::ModemEngine;

fn make_engine() -> ModemEngine {
    let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
    engine
        .register_plugin(Box::new(BpskPlugin::new()))
        .expect("register BPSK");
    engine
}

#[test]
fn no_profile_apply_ack_returns_maintained() {
    let mut engine = make_engine();
    assert_eq!(engine.apply_ack(AckType::AckUp), RateEvent::Maintained);
}

#[test]
fn no_profile_current_mode_is_none() {
    let engine = make_engine();
    assert_eq!(engine.current_adaptive_mode(), None);
}

#[test]
fn hpx500_starts_at_bpsk31() {
    let mut engine = make_engine();
    engine.start_adaptive_session(SessionProfile::hpx500());
    assert_eq!(engine.current_adaptive_mode(), Some("BPSK31"));
}

#[test]
fn ack_up_three_times_reaches_bpsk250() {
    let mut engine = make_engine();
    engine.start_adaptive_session(SessionProfile::hpx500());
    assert_eq!(
        engine.apply_ack(AckType::AckUp),
        RateEvent::Increased(SpeedLevel::Sl3)
    );
    assert_eq!(
        engine.apply_ack(AckType::AckUp),
        RateEvent::Increased(SpeedLevel::Sl4)
    );
    assert_eq!(engine.current_adaptive_mode(), Some("BPSK250"));
}

#[test]
fn ack_down_from_sl4_returns_to_sl3() {
    let mut engine = make_engine();
    engine.start_adaptive_session(SessionProfile::hpx500());
    engine.apply_ack(AckType::AckUp);
    engine.apply_ack(AckType::AckUp);
    assert_eq!(engine.current_adaptive_mode(), Some("BPSK250"));
    assert_eq!(
        engine.apply_ack(AckType::AckDown),
        RateEvent::Decreased(SpeedLevel::Sl3)
    );
    assert_eq!(engine.current_adaptive_mode(), Some("BPSK63"));
}

#[test]
fn three_nacks_at_sl3_decrement_to_sl2() {
    let mut engine = make_engine();
    engine.start_adaptive_session(SessionProfile::hpx500());
    engine.apply_ack(AckType::AckUp); // SL2 → SL3
    assert_eq!(engine.current_adaptive_mode(), Some("BPSK63"));
    engine.apply_ack(AckType::Nack);
    engine.apply_ack(AckType::Nack);
    let ev = engine.apply_ack(AckType::Nack);
    assert_eq!(ev, RateEvent::NackDecrement(SpeedLevel::Sl2));
    assert_eq!(engine.current_adaptive_mode(), Some("BPSK31"));
}

#[test]
fn hpx2300_starts_at_qpsk500() {
    let mut engine = make_engine();
    engine.start_adaptive_session(SessionProfile::hpx2300());
    assert_eq!(engine.current_adaptive_mode(), Some("QPSK500"));
}

#[test]
fn hpx2300_ack_up_reaches_8psk1000() {
    let mut engine = make_engine();
    engine.start_adaptive_session(SessionProfile::hpx2300());
    engine.apply_ack(AckType::AckUp); // SL8 → SL9 (QPSK1000)
    assert_eq!(engine.current_adaptive_mode(), Some("QPSK1000"));
    engine.apply_ack(AckType::AckUp); // SL9 → SL10 (reserved, None)
    assert_eq!(engine.current_adaptive_mode(), None);
    engine.apply_ack(AckType::AckUp); // SL10 → SL11 (8PSK1000)
    assert_eq!(engine.current_adaptive_mode(), Some("8PSK1000"));
}
