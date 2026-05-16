//! BL-TP-7 pilot-density review against measured Doppler spread.
//!
//! This test compares sparse pilots (spacing=5) vs dense pilots (spacing=4)
//! for SC-FDMA 64QAM under identical Watterson channels.

use openpulse_channel::{watterson::WattersonChannel, ChannelModel, WattersonConfig};
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use scfdma_plugin::ScFdmaPlugin;

fn cfg(mode: &str) -> ModulationConfig {
    ModulationConfig {
        mode: mode.into(),
        sample_rate: 8000,
        center_frequency: 1500.0,
        ..ModulationConfig::default()
    }
}

fn bytes_to_bits(bytes: &[u8]) -> Vec<bool> {
    bytes
        .iter()
        .flat_map(|b| (0..8).map(move |i| (b >> i) & 1 == 1))
        .collect()
}

fn bit_agreement(payload: &[u8], llrs: &[f32]) -> f32 {
    let expected = bytes_to_bits(payload);
    if llrs.len() < expected.len() {
        return 0.0;
    }

    let mut matches = 0usize;
    for (idx, bit) in expected.iter().enumerate() {
        let hard_one = llrs[idx].is_sign_negative();
        if hard_one == *bit {
            matches += 1;
        }
    }
    matches as f32 / expected.len() as f32
}

fn run_mean_agreement(mode: &str, doppler_hz: f32, frames: usize) -> f32 {
    let plugin = ScFdmaPlugin::new();
    let payload: Vec<u8> = (0u8..96).collect();
    let tx = plugin.modulate(&payload, &cfg(mode)).unwrap();

    let mut agreement_sum = 0.0f32;
    for frame in 0..frames {
        let mut wc = WattersonConfig::poor_f1(Some(0x7200 + frame as u64));
        wc.doppler_spread_hz = doppler_hz;
        wc.snr_db = 16.0;
        let mut ch = WattersonChannel::new(wc).unwrap();

        let faded = ch.apply(&tx);
        let llrs = plugin.demodulate_soft(&faded, &cfg(mode)).unwrap();
        agreement_sum += bit_agreement(&payload, &llrs);
    }

    agreement_sum / frames as f32
}

#[test]
fn dense_pilot_profile_degrades_less_under_higher_doppler() {
    let frames = 24usize;

    // Lower Doppler (close to Good F2).
    let sparse_low = run_mean_agreement("SCFDMA52-64QAM", 0.5, frames);
    let dense_low = run_mean_agreement("SCFDMA52-64QAM-P4", 0.5, frames);

    // Higher Doppler (Poor F1 region).
    let sparse_high = run_mean_agreement("SCFDMA52-64QAM", 2.0, frames);
    let dense_high = run_mean_agreement("SCFDMA52-64QAM-P4", 2.0, frames);

    println!(
        "pilot-density-review: sparse(low={:.3}, high={:.3}) dense(low={:.3}, high={:.3})",
        sparse_low, sparse_high, dense_low, dense_high
    );

    // BL-TP-7 gate: dense pilots should not underperform sparse pilots in
    // absolute bit agreement at either low or high Doppler.
    assert!(
        dense_low + 0.005 >= sparse_low,
        "dense-pilot low-Doppler agreement regressed: dense_low={dense_low:.3} sparse_low={sparse_low:.3}"
    );
    assert!(
        dense_high + 0.005 >= sparse_high,
        "dense-pilot high-Doppler agreement regressed: dense_high={dense_high:.3} sparse_high={sparse_high:.3}"
    );
}
