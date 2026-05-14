//! Item 5 acceptance gate: weighted LLR combining ≥2 dB vs equal-weight combining.
//!
//! Verifies that `receive_with_llr_combining` (inverse-noise-variance weighted)
//! recovers the payload at a noticeably lower SNR than equal-weight sample
//! combining (`receive_with_soft_combining`), demonstrating the ≥2 dB gain
//! acceptance criterion.

use openpulse_audio::LoopbackBackend;
use openpulse_channel::{awgn::AwgnChannel, AwgnConfig, ChannelModel};
use openpulse_core::fec::SoftCombiner;
use openpulse_modem::engine::ModemEngine;
use scfdma_plugin::ScFdmaPlugin;

fn make_modem() -> (ModemEngine, LoopbackBackend) {
    let backend = LoopbackBackend::new();
    let mut engine = ModemEngine::new(Box::new(backend.clone_shared()));
    engine
        .register_plugin(Box::new(ScFdmaPlugin::new()))
        .expect("plugin registration");
    (engine, backend)
}

/// Transmit one frame and produce a clean TX sample buffer.
fn tx_samples(payload: &[u8]) -> Vec<f32> {
    let (mut tx, tx_backend) = make_modem();
    tx.transmit_with_fec(payload, "SCFDMA52", None)
        .expect("TX with FEC");
    tx_backend.drain_samples()
}

fn awgn(samples: &[f32], snr_db: f32, seed: u64) -> Vec<f32> {
    let mut ch = AwgnChannel::new(AwgnConfig::new(snr_db, Some(seed))).expect("awgn");
    ch.apply(samples)
}

/// Test that `receive_with_llr_combining` (weighted) achieves at least 2 dB
/// lower SNR threshold than equal-weight sample combining under heterogeneous
/// per-frame SNR — the scenario where weighted combining pays off.
///
/// Scenario: 3 retransmissions, two at `snr_good` and one at `snr_good − 8 dB`
/// (simulating a deeply-faded retransmit).  Equal-weight sample averaging is
/// pulled down by the bad frame; weighted LLR combining suppresses it.
///
/// Sweep `snr_good` upward.  Find the threshold where each method first
/// succeeds.  The weighted threshold must be ≥ 2 dB lower.
#[test]
fn weighted_llr_combining_at_least_2_db_gain_over_equal_weight() {
    let payload = b"Item5 weighted LLR combining gain gate payload!";
    let tx = tx_samples(payload);

    const FADE_OFFSET_DB: f32 = 8.0; // bad-frame is this many dB below good frames
    const SEEDS: [(u64, bool); 3] = [(0xAA01, true), (0xAA02, true), (0xAA03, false)];
    // true = good SNR, false = faded

    let n_attempts = 3usize;

    // ── Equal-weight threshold ──────────────────────────────────────────────
    let mut equal_threshold_db = f32::NAN;
    let mut snr = 0.0_f32;
    while snr <= 25.0 {
        let mut combiner = SoftCombiner::new();
        for &(seed, good) in &SEEDS {
            let frame_snr = if good { snr } else { snr - FADE_OFFSET_DB };
            combiner.push(&awgn(&tx, frame_snr, seed));
        }
        let combined = combiner.combine();

        let (mut rx, rx_backend) = make_modem();
        rx_backend.fill_samples(&combined);
        if rx
            .receive_with_soft_combining("SCFDMA52", None, 1)
            .map(|b| b == payload)
            .unwrap_or(false)
        {
            equal_threshold_db = snr;
            break;
        }
        snr += 0.5;
    }
    assert!(
        equal_threshold_db.is_finite(),
        "equal-weight combining never succeeded in 0–25 dB range"
    );

    // ── Weighted LLR threshold ──────────────────────────────────────────────
    let mut weighted_threshold_db = f32::NAN;
    let mut snr = 0.0_f32;
    while snr <= 25.0 {
        let (mut rx, rx_backend) = make_modem();
        for &(seed, good) in &SEEDS {
            let frame_snr = if good { snr } else { snr - FADE_OFFSET_DB };
            rx_backend.push_frame(&awgn(&tx, frame_snr, seed));
        }
        if rx
            .receive_with_llr_combining("SCFDMA52", None, n_attempts)
            .map(|b| b == payload)
            .unwrap_or(false)
        {
            weighted_threshold_db = snr;
            break;
        }
        snr += 0.5;
    }
    assert!(
        weighted_threshold_db.is_finite(),
        "weighted LLR combining never succeeded in 0–25 dB range"
    );

    let gain_db = equal_threshold_db - weighted_threshold_db;
    assert!(
        gain_db >= 2.0,
        "weighted LLR combining gain {:.1} dB < 2.0 dB required \
         (weighted threshold {:.1} dB, equal-weight threshold {:.1} dB)",
        gain_db,
        weighted_threshold_db,
        equal_threshold_db,
    );
}
