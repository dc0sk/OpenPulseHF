use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::ack::AckType;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::{RateTrigger, SpeedLevel};
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
    engine.register_plugin(Box::new(BpskPlugin::new())).ok();
    engine.start_adaptive_session(SessionProfile::hpx_wideband_hd());

    assert_eq!(engine.current_tx_level(), Some(SpeedLevel::Sl12));

    // SL12 floor = 22 dB; 20 dB should force immediate step-down.
    engine.apply_snr_hint(20.0);

    assert!(
        engine.current_tx_level().unwrap() < SpeedLevel::Sl12,
        "SL12 floor breach must step down immediately"
    );

    let event = rx
        .try_recv()
        .expect("a RateChange event must be emitted on wideband-hd floor breach");
    match event {
        EngineEvent::RateChange { trigger, .. } => {
            assert_eq!(trigger, Some(RateTrigger::SnrFloor));
        }
        other => panic!("expected RateChange, got {other:?}"),
    }
}

/// Engine at SL13 (hpx_wideband_hd); SNR below SL13 floor must step down to SL12.
#[test]
fn wideband_hd_sl13_floor_breach_steps_to_sl12() {
    let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
    engine.register_plugin(Box::new(BpskPlugin::new())).ok();
    engine.start_adaptive_session(SessionProfile::hpx_wideband_hd());

    assert_eq!(engine.current_tx_level(), Some(SpeedLevel::Sl12));
    // SL12 -> SL13 by ACK-UP.
    let _ = engine.apply_ack(AckType::AckUp);
    assert_eq!(engine.current_tx_level(), Some(SpeedLevel::Sl13));

    // SL13 floor = 24 dB; 23 dB should force immediate step-down.
    engine.apply_snr_hint(23.0);
    assert_eq!(engine.current_tx_level(), Some(SpeedLevel::Sl12));
}
