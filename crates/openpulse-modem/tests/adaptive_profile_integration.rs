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
fn hpx_pilot_climbs_and_descends_the_rungs() {
    let mut engine = make_engine();
    engine.start_adaptive_session(SessionProfile::hpx_pilot());
    assert_eq!(engine.current_adaptive_mode(), Some("PILOT-QPSK500"));

    assert_eq!(
        engine.apply_ack(AckType::AckUp),
        RateEvent::Increased(SpeedLevel::Sl3)
    );
    assert_eq!(engine.current_adaptive_mode(), Some("PILOT-8PSK500"));

    assert_eq!(
        engine.apply_ack(AckType::AckUp),
        RateEvent::Increased(SpeedLevel::Sl4)
    );
    assert_eq!(engine.current_adaptive_mode(), Some("PILOT-16QAM500"));

    assert_eq!(
        engine.apply_ack(AckType::AckDown),
        RateEvent::Decreased(SpeedLevel::Sl3)
    );
    assert_eq!(engine.current_adaptive_mode(), Some("PILOT-8PSK500"));
}

#[test]
fn arq_max_tx_level_caps_the_adaptive_ladder() {
    // hpx500: SL2 BPSK31 → SL3 BPSK63 → SL4 BPSK250 → SL5 QPSK250 → SL6 QPSK500.
    let mut engine = make_engine();
    engine.start_adaptive_session(SessionProfile::hpx500());
    assert_eq!(engine.current_adaptive_mode(), Some("BPSK31"));

    // Cap the ladder at SL4 (BPSK250) — an ARQBW host limit.
    engine.set_arq_max_tx_level(Some(SpeedLevel::Sl4));

    // No amount of AckUp may climb past the cap.
    for _ in 0..8 {
        engine.apply_ack(AckType::AckUp);
    }
    assert_eq!(
        engine.current_adaptive_mode(),
        Some("BPSK250"),
        "the ladder must not climb above the SL4 cap"
    );
    assert_eq!(engine.current_tx_level(), Some(SpeedLevel::Sl4));

    // Clearing the cap restores upward mobility.
    engine.set_arq_max_tx_level(None);
    engine.apply_ack(AckType::AckUp);
    assert_eq!(
        engine.current_adaptive_mode(),
        Some("QPSK250"),
        "clearing the cap lets the ladder climb again"
    );
}

#[test]
fn arq_max_tx_level_clamps_an_already_high_session() {
    // A cap set after the ladder has already climbed must drag it back down immediately.
    let mut engine = make_engine();
    engine.start_adaptive_session(SessionProfile::hpx500());
    for _ in 0..6 {
        engine.apply_ack(AckType::AckUp); // climb toward the top (SL6 QPSK500)
    }
    assert_eq!(engine.current_adaptive_mode(), Some("QPSK500"));

    engine.set_arq_max_tx_level(Some(SpeedLevel::Sl3));
    assert_eq!(
        engine.current_adaptive_mode(),
        Some("BPSK63"),
        "setting a cap below the current level clamps immediately"
    );
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
fn hpx_hf_starts_at_bpsk31() {
    let mut engine = make_engine();
    engine.start_adaptive_session(SessionProfile::hpx_hf());
    assert_eq!(engine.current_adaptive_mode(), Some("BPSK31"));
}

#[test]
fn hpx_hf_ack_up_seven_times_reaches_ofdm52_16qam() {
    // Fade-aware ladder: the coherent single-carrier mid rungs (QPSK250/QPSK500/8PSK500) decoded ~0 %
    // on a moderate_f1 fade at any SNR and were removed, so above SL6 the ladder is OFDM.
    let mut engine = make_engine();
    engine.start_adaptive_session(SessionProfile::hpx_hf());
    engine.apply_ack(AckType::AckUp); // SL2 → SL3 (BPSK63)
    engine.apply_ack(AckType::AckUp); // SL3 → SL4 (BPSK100)
    engine.apply_ack(AckType::AckUp); // SL4 → SL5 (BPSK250)
    engine.apply_ack(AckType::AckUp); // SL5 → SL6 (QPSK250-D + Rs)
    engine.apply_ack(AckType::AckUp); // SL6 → SL7 (OFDM52)
    engine.apply_ack(AckType::AckUp); // SL7 → SL8 (OFDM52-8PSK)
    engine.apply_ack(AckType::AckUp); // SL8 → SL9 (OFDM52-16QAM)
    assert_eq!(engine.current_adaptive_mode(), Some("OFDM52-16QAM"));
}

#[test]
fn hpx_wideband_starts_at_qpsk500() {
    let mut engine = make_engine();
    engine.start_adaptive_session(SessionProfile::hpx_wideband());
    assert_eq!(engine.current_adaptive_mode(), Some("QPSK500"));
}

#[test]
fn hpx_wideband_ack_up_reaches_8psk1000() {
    let mut engine = make_engine();
    engine.start_adaptive_session(SessionProfile::hpx_wideband());
    engine.apply_ack(AckType::AckUp); // SL8 → SL9 (QPSK1000)
    assert_eq!(engine.current_adaptive_mode(), Some("QPSK1000"));
    engine.apply_ack(AckType::AckUp); // SL9 → SL11 (skip reserved SL10)
    assert_eq!(engine.current_adaptive_mode(), Some("8PSK1000"));
}
