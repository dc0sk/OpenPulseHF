//! Item 6 vertical-slice gate: deterministic HARQ rate/FEC selection on Watterson F1.
//!
//! This test validates the first acceptance slice:
//! - SNR + fading-depth inputs map to deterministic FEC selections.
//! - Retry attempts never reduce coding strength.
//! - ACK timeout follows the configured SNR curve.

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_channel::{watterson::WattersonChannel, ChannelModel, WattersonConfig};
use openpulse_core::fec::FecMode;
use openpulse_modem::harq::ack_timeout_ms_for_snr;
use openpulse_modem::ModemEngine;

fn fading_depth_db(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut env: Vec<f32> = samples.iter().map(|s| s.abs()).collect();
    env.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = env.len();
    let lo = env[((n as f32 * 0.10) as usize).min(n - 1)];
    let hi = env[((n as f32 * 0.90) as usize).min(n - 1)];
    let lo = lo.max(1e-6);
    20.0 * (hi / lo).log10()
}

#[test]
fn harq_rate_selection_watterson_f1_mapping_and_retry_escalation() {
    let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
    engine.register_plugin(Box::new(BpskPlugin::new())).unwrap();

    let payload = b"item6 harq selection probe";
    engine.transmit_with_fec(payload, "BPSK250", None).unwrap();

    // Use one deterministic Watterson sample for fading-depth estimation.
    let mut ch = WattersonChannel::new(WattersonConfig::good_f1(Some(0x6611))).unwrap();
    let probe = ch.apply(&vec![0.25_f32; 8000]);
    let fade_db = fading_depth_db(&probe);

    // Deterministic mapping checks with controlled fading inputs.
    let high = engine.select_harq_decision(25.0, 2.0, 0);
    assert_eq!(high.fec_mode, FecMode::Rs);
    assert_eq!(high.ack_timeout_ms, ack_timeout_ms_for_snr(25.0));

    let mid = engine.select_harq_decision(18.0, 2.0, 0);
    assert_eq!(mid.fec_mode, FecMode::SoftConcatenated);
    assert_eq!(mid.ack_timeout_ms, ack_timeout_ms_for_snr(18.0));

    let low = engine.select_harq_decision(12.0, 2.0, 0);
    assert_eq!(low.fec_mode, FecMode::RsStrong);
    assert_eq!(low.ack_timeout_ms, ack_timeout_ms_for_snr(12.0));

    // Watterson-derived fading sanity: policy consumes measured fading and
    // chooses a non-weaker mode than plain RS at 20 dB when fading is deep.
    let from_watterson = engine.select_harq_decision(20.0, fade_db, 0);
    assert!(
        from_watterson.fec_mode.strength() >= FecMode::Rs.strength(),
        "Watterson-derived decision should be at least RS; fade={fade_db:.2} dB"
    );

    // Retry strength must be monotonic non-decreasing.
    let r0 = engine.select_harq_decision(24.0, 2.0, 0);
    let r1 = engine.select_harq_decision(24.0, 2.0, 1);
    let r2 = engine.select_harq_decision(24.0, 2.0, 2);
    assert!(r1.fec_mode.strength() >= r0.fec_mode.strength());
    assert!(r2.fec_mode.strength() >= r1.fec_mode.strength());

    // Item 6 timeout anchors.
    assert_eq!(ack_timeout_ms_for_snr(15.0), 800);
    assert_eq!(ack_timeout_ms_for_snr(20.0), 600);
    assert_eq!(ack_timeout_ms_for_snr(25.0), 400);
}
