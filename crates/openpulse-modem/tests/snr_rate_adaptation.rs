use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::ack::AckType;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::{RateTrigger, SpeedLevel};
use openpulse_modem::{EngineEvent, ModemEngine};

/// An engine on `profile`, climbed by ACK-UP to `level`, with its event queue drained.
fn engine_at(
    profile: SessionProfile,
    level: SpeedLevel,
) -> (ModemEngine, tokio::sync::broadcast::Receiver<EngineEvent>) {
    let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
    engine.register_plugin(Box::new(BpskPlugin::new())).ok();
    engine.start_adaptive_session(profile);
    while engine.current_tx_level() < Some(level) {
        let before = engine.current_tx_level();
        let _ = engine.apply_ack(AckType::AckUp);
        assert_ne!(engine.current_tx_level(), before, "stuck below {level:?}");
    }
    assert_eq!(engine.current_tx_level(), Some(level));
    let mut rx = engine.subscribe();
    while rx.try_recv().is_ok() {}
    (engine, rx)
}

/// `fast` at SL8 (floor 10 dB); SNR well below it must step down before any NACK.
#[test]
fn snr_floor_breach_steps_down_before_nack() {
    let (mut engine, _rx) = engine_at(SessionProfile::fast(), SpeedLevel::Sl8);
    engine.apply_snr_hint(-10.0);
    let level_after = engine.current_tx_level().expect("session");
    assert!(
        level_after < SpeedLevel::Sl8,
        "TX level should have stepped down from SL8; got {level_after:?}"
    );
}

/// The emitted RateChange event carries trigger = SnrFloor.
#[test]
fn snr_floor_breach_emits_snr_floor_trigger() {
    let (mut engine, mut rx) = engine_at(SessionProfile::fast(), SpeedLevel::Sl8);
    engine.apply_snr_hint(-10.0);
    match rx
        .try_recv()
        .expect("a RateChange event must be emitted on SNR floor breach")
    {
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

/// SNR above the floor but below the ceiling — no action, level unchanged.
#[test]
fn snr_in_range_has_no_effect() {
    let (mut engine, mut rx) = engine_at(SessionProfile::fast(), SpeedLevel::Sl8);
    // SL8 floor = 10 dB, ceiling = 14 dB; 12 dB is in range.
    engine.apply_snr_hint(12.0);
    assert!(rx.try_recv().is_err(), "no event when SNR is in range");
    assert_eq!(engine.current_tx_level(), Some(SpeedLevel::Sl8));
}

/// SNR above the ceiling sets the upgrade candidate; the level moves only on the ACK-UP.
#[test]
fn snr_ceiling_sets_upgrade_candidate_without_level_change() {
    let (mut engine, mut rx) = engine_at(SessionProfile::fast(), SpeedLevel::Sl8);
    engine.apply_snr_hint(25.0);
    assert!(
        rx.try_recv().is_err(),
        "no RateChange on a ceiling hint alone"
    );
    assert_eq!(engine.current_tx_level(), Some(SpeedLevel::Sl8));
    let _ = engine.apply_ack(AckType::AckUp);
    assert_eq!(engine.current_tx_level(), Some(SpeedLevel::Sl9));
}

/// SL13 (floor 19 dB): 18 dB steps down to SL12 with an SnrFloor trigger.
#[test]
fn sl13_floor_breach_steps_to_sl12() {
    let (mut engine, mut rx) = engine_at(SessionProfile::fast(), SpeedLevel::Sl13);
    engine.apply_snr_hint(18.0);
    assert_eq!(engine.current_tx_level(), Some(SpeedLevel::Sl12));
    let saw_snr_floor = std::iter::from_fn(|| rx.try_recv().ok()).any(|e| {
        matches!(
            e,
            EngineEvent::RateChange {
                trigger: Some(RateTrigger::SnrFloor),
                speed_level: SpeedLevel::Sl12,
                ..
            }
        )
    });
    assert!(
        saw_snr_floor,
        "must observe RateChange with SnrFloor at SL12"
    );
}

/// `robust` at its cap: a ceiling hint and an ACK-UP leave it at SL6.
#[test]
fn robust_at_its_cap_does_not_climb_on_a_ceiling_hint() {
    let (mut engine, _rx) = engine_at(SessionProfile::robust(), SpeedLevel::Sl6);
    engine.apply_snr_hint(40.0);
    let _ = engine.apply_ack(AckType::AckUp);
    assert_eq!(engine.current_tx_level(), Some(SpeedLevel::Sl6));
}
