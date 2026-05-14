//! HARQ retry-path integration test under deterministic Watterson routing.
//!
//! Verifies that engine-level HARQ attempt helpers execute TX/RX with selected
//! FEC modes and recover payload across retry indices.

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_channel::{
    awgn::AwgnChannel, watterson::WattersonChannel, AwgnConfig, ChannelModel, WattersonConfig,
};
use openpulse_core::fec::FecMode;
use openpulse_modem::ModemEngine;

fn route_watterson_awgn(samples: &[f32], snr_db: f32, seed: u64) -> Vec<f32> {
    let mut w = WattersonChannel::new(WattersonConfig::good_f1(Some(seed))).unwrap();
    let faded = w.apply(samples);
    let mut n = AwgnChannel::new(AwgnConfig::new(snr_db, Some(seed ^ 0x77AA_19D3))).unwrap();
    n.apply(&faded)
}

#[test]
fn harq_retry_path_dispatches_fec_and_decodes() {
    let backend = LoopbackBackend::new();
    let shared = backend.clone_shared();
    let mut engine = ModemEngine::new(Box::new(backend));
    engine.register_plugin(Box::new(BpskPlugin::new())).unwrap();

    let payload = b"item6 harq retry integration payload";
    let snr_db = 25.0_f32;
    let fade_db = 2.0_f32;

    // Attempt 0: policy should pick RS.
    let tx0 = engine
        .transmit_with_harq_attempt(payload, "BPSK250", snr_db, fade_db, 0, None)
        .unwrap();
    assert_eq!(tx0.fec_mode, FecMode::Rs);

    let raw0 = shared.drain_samples();
    assert!(!raw0.is_empty());
    shared.fill_samples(&route_watterson_awgn(&raw0, 20.0, 0xA101));

    let (rx0, dec0) = engine
        .receive_with_harq_attempt("BPSK250", snr_db, fade_db, 0, None)
        .unwrap();
    assert_eq!(dec0.fec_mode, FecMode::Rs);
    assert_eq!(rx0, payload);

    // Attempt 1: same channel estimate, retry escalation should pick strong RS.
    let tx1 = engine
        .transmit_with_harq_attempt(payload, "BPSK250", snr_db, fade_db, 1, None)
        .unwrap();
    assert_eq!(tx1.fec_mode, FecMode::RsStrong);

    let raw1 = shared.drain_samples();
    assert!(!raw1.is_empty());
    shared.fill_samples(&route_watterson_awgn(&raw1, 20.0, 0xA102));

    let (rx1, dec1) = engine
        .receive_with_harq_attempt("BPSK250", snr_db, fade_db, 1, None)
        .unwrap();
    assert_eq!(dec1.fec_mode, FecMode::RsStrong);
    assert_eq!(rx1, payload);

    // Timeout policy remains deterministic by SNR across attempts.
    assert_eq!(tx0.ack_timeout_ms, tx1.ack_timeout_ms);
}
