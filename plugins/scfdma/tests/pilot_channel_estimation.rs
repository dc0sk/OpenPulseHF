//! Pilot-aided channel-estimation and soft-demodulation tests.

use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use scfdma_plugin::ScFdmaPlugin;

fn cfg(mode: &str) -> ModulationConfig {
    ModulationConfig {
        mode: mode.into(),
        center_frequency: 1500.0,
        sample_rate: 8000,
        ..ModulationConfig::default()
    }
}

fn bytes_to_bits(bytes: &[u8]) -> Vec<bool> {
    let mut bits = Vec::with_capacity(bytes.len() * 8);
    for &b in bytes {
        for shift in 0..8u8 {
            bits.push((b >> shift) & 1 == 1);
        }
    }
    bits
}

#[test]
fn soft_demod_returns_payload_llrs_for_qpsk_mode() {
    let plugin = ScFdmaPlugin::new();
    let payload: Vec<u8> = (0u8..32).collect();

    let samples = plugin.modulate(&payload, &cfg("SCFDMA52")).unwrap();
    let llrs = plugin.demodulate_soft(&samples, &cfg("SCFDMA52")).unwrap();

    assert_eq!(llrs.len(), payload.len() * 8);
    assert!(llrs.iter().all(|v| v.is_finite()));
}

#[test]
fn soft_llr_sign_matches_payload_bits_on_clean_channel() {
    let plugin = ScFdmaPlugin::new();
    let payload: Vec<u8> = (0u8..48).collect();

    let samples = plugin
        .modulate(&payload, &cfg("SCFDMA52-64QAM-P4"))
        .unwrap();
    let llrs = plugin
        .demodulate_soft(&samples, &cfg("SCFDMA52-64QAM-P4"))
        .unwrap();

    let bits = bytes_to_bits(&payload);
    assert_eq!(llrs.len(), bits.len());

    let mut matches = 0usize;
    for (llr, bit) in llrs.iter().zip(bits.iter()) {
        let hard_bit_is_one = llr.is_sign_negative();
        if hard_bit_is_one == *bit {
            matches += 1;
        }
    }

    let agreement = matches as f32 / bits.len() as f32;
    assert!(
        agreement > 0.95,
        "LLR sign should track payload bits on clean channel; agreement={agreement:.3}"
    );
}
