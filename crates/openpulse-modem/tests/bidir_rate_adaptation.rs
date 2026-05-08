use openpulse_core::ack::{AckFrame, AckType};
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::{BiDirRateAdapter, SpeedLevel};

/// Simulates bidirectional ACK exchange where the A→B path is good (AckUp)
/// but the B→A path (reverse_ack) is bad (Nack).  After enough frames the
/// TX and RX speed levels of node A's adapter must diverge.
#[test]
fn asymmetric_paths_converge_to_different_speed_levels() {
    let mut adapter = BiDirRateAdapter::new(SpeedLevel::Sl5, 3);

    // TX path: peer keeps sending AckUp (good A→B link).
    // RX path: peer's reverse_ack is Nack (bad B→A link).
    for i in 0..30 {
        // Build an AckFrame with good forward ack + bad reverse_ack.
        let frame = AckFrame::new_with_reverse(AckType::AckUp, "sess", AckType::Nack);
        adapter.apply_ack(frame.ack_type);
        if let Some(rev) = frame.reverse_ack {
            adapter.apply_reverse_ack(rev);
        }
        let _ = i; // used for loop control
    }

    assert!(
        adapter.tx_level() > adapter.rx_level(),
        "TX should have climbed while RX fell; tx={:?} rx={:?}",
        adapter.tx_level(),
        adapter.rx_level()
    );
}

/// Round-tripping AckFrame with reverse_ack through the engine's apply_ack_frame.
#[test]
fn ack_frame_with_reverse_ack_updates_both_directions() {
    use bpsk_plugin::BpskPlugin;
    use openpulse_audio::LoopbackBackend;
    use openpulse_modem::ModemEngine;

    let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
    engine.register_plugin(Box::new(BpskPlugin::new())).ok();
    engine.start_adaptive_session(SessionProfile::hpx500());

    // Both directions start at SL2 (hpx500 initial).
    // Apply AckUp for TX path and Nack for RX path.
    let frame = AckFrame::new_with_reverse(AckType::AckUp, "sess", AckType::Nack);
    engine.apply_ack_frame(&frame);
    engine.apply_ack_frame(&frame);
    engine.apply_ack_frame(&frame);

    // TX should have stepped up (to SL3); RX still waiting for NACK threshold.
    let tx_mode = engine.current_adaptive_mode();
    let rx_mode = engine.current_rx_mode();
    assert_ne!(
        tx_mode, rx_mode,
        "TX and RX modes should diverge after asymmetric ACKs"
    );
}

/// Backward-compatible AckFrame (no reverse_ack) only updates TX direction.
#[test]
fn legacy_ack_frame_only_updates_tx() {
    use bpsk_plugin::BpskPlugin;
    use openpulse_audio::LoopbackBackend;
    use openpulse_modem::ModemEngine;

    let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
    engine.register_plugin(Box::new(BpskPlugin::new())).ok();
    engine.start_adaptive_session(SessionProfile::hpx500());

    let frame = AckFrame::new(AckType::AckUp, "sess");
    assert!(frame.reverse_ack.is_none());

    engine.apply_ack_frame(&frame);
    engine.apply_ack_frame(&frame);

    // RX direction should remain at initial level since no reverse_ack was sent.
    assert_eq!(
        engine.current_rx_mode(),
        Some("BPSK31"),
        "RX should stay at SL2 initial"
    );
}
