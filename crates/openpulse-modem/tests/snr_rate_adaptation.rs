use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::ack::AckType;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::{RateEvent, RateTrigger, SpeedLevel};
use openpulse_modem::{EngineEvent, ModemEngine};

/// Engine at SL8 (hpx_wideband); inject SNR well below the SL8 floor.
/// Must step down before any NACK is processed — within a single hint call.
#[test]
fn snr_floor_breach_steps_down_before_nack() {
    let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
    engine.register_plugin(Box::new(BpskPlugin::new())).ok();
    engine.start_adaptive_session(SessionProfile::hpx_wideband());

    // Verify we start at SL8.
    assert_eq!(engine.current_tx_level(), Some(SpeedLevel::Sl8));

    // SNR = −10 dB — far below the SL8 floor of 11 dB.  No ACK/NACK applied yet.
    engine.apply_snr_hint(-10.0);

    let level_after = engine.current_tx_level().unwrap();
    assert!(
        level_after < SpeedLevel::Sl8,
        "TX level should have stepped down from SL8; got {level_after:?}"
    );
}

/// The emitted RateChange event carries trigger = SnrFloor.
#[test]
fn snr_floor_breach_emits_snr_floor_trigger() {
    let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
    let mut rx = engine.subscribe();
    engine.register_plugin(Box::new(BpskPlugin::new())).ok();
    engine.start_adaptive_session(SessionProfile::hpx_wideband());

    engine.apply_snr_hint(-10.0);

    let event = rx
        .try_recv()
        .expect("a RateChange event must be emitted on SNR floor breach");
    match event {
        EngineEvent::RateChange { trigger, .. } => {
            assert_eq!(
                trigger,
                Some(RateTrigger::SnrFloor),
                "trigger must be SnrFloor"
            );
        }
        other => panic!("expected RateChange, got {other:?}"),
    }
}

/// SNR above floor but below ceiling — no action, level unchanged.
#[test]
fn snr_in_range_has_no_effect() {
    let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
    let mut rx = engine.subscribe();
    engine.register_plugin(Box::new(BpskPlugin::new())).ok();
    engine.start_adaptive_session(SessionProfile::hpx_wideband());

    // SL8 floor=11 dB, ceiling=18 dB; 14 dB is in range.
    engine.apply_snr_hint(14.0);

    assert!(
        rx.try_recv().is_err(),
        "no event should be emitted when SNR is in range"
    );
    assert_eq!(engine.current_tx_level(), Some(SpeedLevel::Sl8));
}

/// SNR above the ceiling sets the upgrade-candidate flag; no level change.
#[test]
fn snr_ceiling_sets_upgrade_candidate_without_level_change() {
    let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
    let mut rx = engine.subscribe();
    engine.register_plugin(Box::new(BpskPlugin::new())).ok();
    engine.start_adaptive_session(SessionProfile::hpx_wideband());

    // SL8 ceiling = 18 dB; 25 dB is above it.
    engine.apply_snr_hint(25.0);

    // No event emitted (upgrade not confirmed yet).
    assert!(
        rx.try_recv().is_err(),
        "no RateChange should fire on ceiling hint alone"
    );
    // Level unchanged — still waiting for ACK-UP.
    assert_eq!(engine.current_tx_level(), Some(SpeedLevel::Sl8));
}

/// Engine at SL12 (hpx_wideband_hd); inject SNR below SL12 floor.
/// Must step down immediately with SnrFloor trigger.
#[test]
fn wideband_hd_sl12_floor_breach_steps_down_immediately() {
    let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
    let mut rx = engine.subscribe();
    engine.start_adaptive_session(SessionProfile::hpx_wideband_hd());

    assert_eq!(engine.current_tx_level(), Some(SpeedLevel::Sl12));

    // SL12 floor = 16 dB; 14 dB should force immediate step-down.
    engine.apply_snr_hint(14.0);

    assert_eq!(
        engine.current_tx_level(),
        Some(SpeedLevel::Sl11),
        "SL12 floor breach must step down; no mode below SL12, so falls to Sl11 (undefined = step past)"
    );

    let event = rx
        .try_recv()
        .expect("a RateChange event must be emitted on wideband-hd floor breach");
    match event {
        EngineEvent::RateChange {
            trigger,
            speed_level,
            ..
        } => {
            assert_eq!(trigger, Some(RateTrigger::SnrFloor));
            assert_eq!(speed_level, SpeedLevel::Sl11);
        }
        other => panic!("expected RateChange, got {other:?}"),
    }
}

/// Engine at SL13 (hpx_wideband_hd); SNR below SL13 floor must step down to SL12.
#[test]
fn wideband_hd_sl13_floor_breach_steps_to_sl12() {
    let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
    let mut rx = engine.subscribe();
    engine.start_adaptive_session(SessionProfile::hpx_wideband_hd());

    assert_eq!(engine.current_tx_level(), Some(SpeedLevel::Sl12));
    // SL12 -> SL13 by ACK-UP.
    let _ = engine.apply_ack(AckType::AckUp);
    assert_eq!(engine.current_tx_level(), Some(SpeedLevel::Sl13));

    // SL13 floor = 20 dB; 18 dB should force immediate step-down.
    engine.apply_snr_hint(18.0);
    assert_eq!(engine.current_tx_level(), Some(SpeedLevel::Sl12));

    let mut saw_snr_floor = false;
    while let Ok(event) = rx.try_recv() {
        if let EngineEvent::RateChange {
            trigger,
            speed_level,
            ..
        } = event
        {
            if trigger == Some(RateTrigger::SnrFloor) {
                assert_eq!(speed_level, SpeedLevel::Sl12);
                saw_snr_floor = true;
                break;
            }
        }
    }
    assert!(
        saw_snr_floor,
        "must observe RateChange with SnrFloor trigger at SL12"
    );
}

/// SL13 ceiling breach should only mark upgrade-candidate; no immediate level change.
#[test]
fn wideband_hd_sl13_ceiling_sets_candidate_without_level_change() {
    let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
    let mut rx = engine.subscribe();
    engine.start_adaptive_session(SessionProfile::hpx_wideband_hd());

    assert_eq!(engine.current_tx_level(), Some(SpeedLevel::Sl12));
    assert_eq!(
        engine.apply_ack(AckType::AckUp),
        RateEvent::Increased(SpeedLevel::Sl13)
    );
    assert_eq!(engine.current_tx_level(), Some(SpeedLevel::Sl13));

    // Drain ACK-UP event so we can assert that ceiling hint itself emits nothing.
    while rx.try_recv().is_ok() {}

    // SL13 ceiling = 30 dB; 31 dB should only set upgrade-candidate.
    engine.apply_snr_hint(31.0);

    assert!(
        rx.try_recv().is_err(),
        "ceiling hint should not emit immediate RateChange"
    );
    assert_eq!(engine.current_tx_level(), Some(SpeedLevel::Sl13));
}

/// Without SL14 ceiling hint, ACK-UP must not admit SL15 in wideband-HD.
/// SL13 → SL14 is freely admitted (no gate there); only SL14 → SL15 is gated.
#[test]
fn wideband_hd_sl14_ack_up_without_ceiling_stays_sl14() {
    let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
    engine.start_adaptive_session(SessionProfile::hpx_wideband_hd());

    assert_eq!(engine.current_tx_level(), Some(SpeedLevel::Sl12));
    assert_eq!(
        engine.apply_ack(AckType::AckUp),
        RateEvent::Increased(SpeedLevel::Sl13)
    );
    assert_eq!(engine.current_tx_level(), Some(SpeedLevel::Sl13));

    // SL13 → SL14: no gate at SL13, so ACK-UP is freely admitted.
    assert_eq!(
        engine.apply_ack(AckType::AckUp),
        RateEvent::Increased(SpeedLevel::Sl14)
    );
    assert_eq!(engine.current_tx_level(), Some(SpeedLevel::Sl14));

    // Without a prior ceiling hint at SL14, ACK-UP must not admit SL15.
    assert_eq!(engine.apply_ack(AckType::AckUp), RateEvent::Maintained);
    assert_eq!(engine.current_tx_level(), Some(SpeedLevel::Sl14));
}

/// Non-wideband-HD profiles must remain unaffected by the SL13->SL14 wideband-HD gate.
#[test]
fn hpx_wideband_sl9_ack_up_skips_reserved_rung_and_advances() {
    let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
    engine.start_adaptive_session(SessionProfile::hpx_wideband());

    // SL8 -> SL9
    assert_eq!(
        engine.apply_ack(AckType::AckUp),
        RateEvent::Increased(SpeedLevel::Sl9)
    );
    assert_eq!(engine.current_tx_level(), Some(SpeedLevel::Sl9));

    // No wideband-HD gate applies here: ACK-UP should advance to next mapped rung.
    assert_eq!(
        engine.apply_ack(AckType::AckUp),
        RateEvent::Increased(SpeedLevel::Sl11)
    );
    assert_eq!(engine.current_tx_level(), Some(SpeedLevel::Sl11));
}

/// Narrowband profile must also remain ungated by SL13-only wideband-HD admission checks.
#[test]
fn hpx_narrowband_sl9_ack_up_still_advances() {
    let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
    engine.start_adaptive_session(SessionProfile::hpx_narrowband());

    // SL8 -> SL9
    assert_eq!(
        engine.apply_ack(AckType::AckUp),
        RateEvent::Increased(SpeedLevel::Sl9)
    );
    assert_eq!(engine.current_tx_level(), Some(SpeedLevel::Sl9));

    // No wideband-HD gate applies here.
    assert_eq!(
        engine.apply_ack(AckType::AckUp),
        RateEvent::Increased(SpeedLevel::Sl10)
    );
    assert_eq!(engine.current_tx_level(), Some(SpeedLevel::Sl10));
}

/// After SL13 ceiling hint, next ACK-UP should admit SL14.
#[test]
fn wideband_hd_sl13_ceiling_then_ack_up_reaches_sl14() {
    let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
    engine.start_adaptive_session(SessionProfile::hpx_wideband_hd());

    assert_eq!(engine.current_tx_level(), Some(SpeedLevel::Sl12));
    assert_eq!(
        engine.apply_ack(AckType::AckUp),
        RateEvent::Increased(SpeedLevel::Sl13)
    );
    assert_eq!(engine.current_tx_level(), Some(SpeedLevel::Sl13));

    // Prime upgrade-candidate at SL13.
    engine.apply_snr_hint(31.0);

    // ACK-UP should now advance to SL14.
    assert_eq!(
        engine.apply_ack(AckType::AckUp),
        RateEvent::Increased(SpeedLevel::Sl14)
    );
    assert_eq!(engine.current_tx_level(), Some(SpeedLevel::Sl14));
}
