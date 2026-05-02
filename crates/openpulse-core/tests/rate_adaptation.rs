use openpulse_core::ack::AckType;
use openpulse_core::rate::{RateAdapter, RateEvent, SpeedLevel};

// ── Rate stepping ──────────────────────────────────────────────────────────────

#[test]
fn rate_increases_on_consecutive_ack_up() {
    let mut a = RateAdapter::new(SpeedLevel::Sl2);
    assert_eq!(
        a.apply_ack(AckType::AckUp),
        RateEvent::Increased(SpeedLevel::Sl3)
    );
    assert_eq!(
        a.apply_ack(AckType::AckUp),
        RateEvent::Increased(SpeedLevel::Sl4)
    );
    assert_eq!(
        a.apply_ack(AckType::AckUp),
        RateEvent::Increased(SpeedLevel::Sl5)
    );
    assert_eq!(a.speed_level(), SpeedLevel::Sl5);
}

#[test]
fn rate_clamps_at_sl11_on_ack_up() {
    let mut a = RateAdapter::new(SpeedLevel::Sl10);
    assert_eq!(
        a.apply_ack(AckType::AckUp),
        RateEvent::Increased(SpeedLevel::Sl11)
    );
    assert_eq!(a.apply_ack(AckType::AckUp), RateEvent::Maintained);
    assert_eq!(a.speed_level(), SpeedLevel::Sl11);
}

#[test]
fn rate_decreases_on_ack_down() {
    let mut a = RateAdapter::new(SpeedLevel::Sl6);
    assert_eq!(
        a.apply_ack(AckType::AckDown),
        RateEvent::Decreased(SpeedLevel::Sl5)
    );
    assert_eq!(
        a.apply_ack(AckType::AckDown),
        RateEvent::Decreased(SpeedLevel::Sl4)
    );
    assert_eq!(a.speed_level(), SpeedLevel::Sl4);
}

#[test]
fn ack_down_floors_at_sl2_not_sl1() {
    let mut a = RateAdapter::new(SpeedLevel::Sl3);
    a.apply_ack(AckType::AckDown); // → SL2
    assert_eq!(a.speed_level(), SpeedLevel::Sl2);
    assert_eq!(a.apply_ack(AckType::AckDown), RateEvent::Maintained);
    assert_eq!(a.speed_level(), SpeedLevel::Sl2);
}

#[test]
fn ack_ok_maintains_current_rate() {
    let mut a = RateAdapter::new(SpeedLevel::Sl5);
    assert_eq!(a.apply_ack(AckType::AckOk), RateEvent::Maintained);
    assert_eq!(a.speed_level(), SpeedLevel::Sl5);
}

// ── NACK handling ──────────────────────────────────────────────────────────────

#[test]
fn nack_below_threshold_requests_retransmit() {
    let mut a = RateAdapter::new(SpeedLevel::Sl4);
    assert_eq!(a.apply_ack(AckType::Nack), RateEvent::Retransmit);
    assert_eq!(a.apply_ack(AckType::Nack), RateEvent::Retransmit);
    assert_eq!(a.speed_level(), SpeedLevel::Sl4); // unchanged
}

#[test]
fn three_nack_decrements_speed_level() {
    let mut a = RateAdapter::new(SpeedLevel::Sl5);
    a.apply_ack(AckType::Nack);
    a.apply_ack(AckType::Nack);
    assert_eq!(
        a.apply_ack(AckType::Nack),
        RateEvent::NackDecrement(SpeedLevel::Sl4)
    );
    assert_eq!(a.speed_level(), SpeedLevel::Sl4);
}

#[test]
fn nack_exhaustion_at_sl2_falls_back_to_sl1_chirp() {
    let mut a = RateAdapter::new(SpeedLevel::Sl2);
    a.apply_ack(AckType::Nack);
    a.apply_ack(AckType::Nack);
    assert_eq!(a.apply_ack(AckType::Nack), RateEvent::ChirpFallback);
    assert_eq!(a.speed_level(), SpeedLevel::Sl1);
}

#[test]
fn nack_at_sl1_always_retransmits_without_further_decrease() {
    let mut a = RateAdapter::new(SpeedLevel::Sl1);
    for _ in 0..9 {
        assert_eq!(a.apply_ack(AckType::Nack), RateEvent::Retransmit);
        assert_eq!(a.speed_level(), SpeedLevel::Sl1);
    }
}

#[test]
fn ack_ok_resets_nack_counter_preventing_spurious_decrement() {
    let mut a = RateAdapter::new(SpeedLevel::Sl4);
    // Two NACKs (below threshold of 3)…
    a.apply_ack(AckType::Nack);
    a.apply_ack(AckType::Nack);
    // …then an ACK-OK resets the counter.
    a.apply_ack(AckType::AckOk);
    // Now it takes 3 fresh NACKs to trigger decrement.
    a.apply_ack(AckType::Nack);
    a.apply_ack(AckType::Nack);
    assert_eq!(a.speed_level(), SpeedLevel::Sl4); // not yet decremented
    assert!(matches!(
        a.apply_ack(AckType::Nack),
        RateEvent::NackDecrement(_)
    ));
}

// ── Simulated SNR improvement scenario ────────────────────────────────────────

#[test]
fn snr_improvement_scenario_rate_climbs_and_stabilises() {
    let mut a = RateAdapter::new(SpeedLevel::Sl2);
    // Channel improves: ACK-UP until SL5.
    for _ in 0..3 {
        let ev = a.apply_ack(AckType::AckUp);
        assert!(matches!(ev, RateEvent::Increased(_)));
    }
    assert_eq!(a.speed_level(), SpeedLevel::Sl5);
    // Channel stable: ACK-OK keeps rate.
    for _ in 0..5 {
        assert_eq!(a.apply_ack(AckType::AckOk), RateEvent::Maintained);
    }
    // Channel degrades: two NACKs then ACK-DOWN settles at SL4.
    a.apply_ack(AckType::Nack);
    a.apply_ack(AckType::Nack);
    a.apply_ack(AckType::AckOk); // recovered, resets counter
    a.apply_ack(AckType::AckDown);
    assert_eq!(a.speed_level(), SpeedLevel::Sl4);
}

// ── Control ACK types ─────────────────────────────────────────────────────────

#[test]
fn break_req_qrt_abort_pass_through_without_rate_change() {
    let mut a = RateAdapter::new(SpeedLevel::Sl5);
    assert_eq!(a.apply_ack(AckType::Break), RateEvent::BreakRequested);
    assert_eq!(a.apply_ack(AckType::Req), RateEvent::Req);
    assert_eq!(a.apply_ack(AckType::Qrt), RateEvent::Qrt);
    assert_eq!(a.apply_ack(AckType::Abort), RateEvent::Abort);
    assert_eq!(a.speed_level(), SpeedLevel::Sl5);
}
