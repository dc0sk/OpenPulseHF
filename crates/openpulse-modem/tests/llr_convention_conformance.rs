//! Cross-plugin LLR convention conformance (review A21).
//!
//! For every soft-capable plugin and mode: hard-slicing the demodulate_soft
//! LLRs (`bit = llr <= 0`, LSB-first) must reproduce exactly the byte stream
//! returned by the hard demodulate() on the same input.  This pins the sign
//! convention and bit ordering that the engine's soft path and the FEC
//! decoders rely on.

use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};

fn llrs_to_bytes(llrs: &[f32]) -> Vec<u8> {
    llrs.chunks(8)
        .map(|byte_llrs| {
            byte_llrs
                .iter()
                .enumerate()
                .fold(0u8, |acc, (i, &llr)| acc | (u8::from(llr <= 0.0) << i))
        })
        .collect()
}

fn assert_conformance(plugin: &dyn ModulationPlugin, mode: &str, fc: f32) {
    assert!(
        plugin.supports_soft_demod(),
        "{mode}: test only applies to soft-capable plugins"
    );
    let cfg = ModulationConfig {
        mode: mode.to_string(),
        center_frequency: fc,
        sample_rate: 8000,
        ..ModulationConfig::default()
    };
    let payload: Vec<u8> = (0..48u8).map(|v| v.wrapping_mul(37) ^ 0x5C).collect();
    let tx = plugin.modulate(&payload, &cfg).expect("modulate");

    let hard = plugin.demodulate(&tx, &cfg).expect("hard demodulate");
    let llrs = plugin.demodulate_soft(&tx, &cfg).expect("soft demodulate");
    let soft_bytes = llrs_to_bytes(&llrs);

    let n = hard.len().min(soft_bytes.len()).min(payload.len());
    assert!(n >= payload.len(), "{mode}: decoded only {n} bytes");
    assert_eq!(
        &soft_bytes[..n],
        &hard[..n],
        "{mode}: hard-sliced LLRs must reproduce the hard demodulate output"
    );
    assert_eq!(
        &hard[..payload.len()],
        &payload[..],
        "{mode}: clean loopback must recover the payload"
    );
}

#[test]
fn llr_convention_conformance() {
    let cases: Vec<(Box<dyn ModulationPlugin>, &str, f32)> = vec![
        (Box::new(bpsk_plugin::BpskPlugin::new()), "BPSK250", 1500.0),
        (
            Box::new(bpsk_plugin::BpskPlugin::new()),
            "BPSK250-RRC",
            1500.0,
        ),
        (Box::new(qpsk_plugin::QpskPlugin::new()), "QPSK250", 1500.0),
        (Box::new(qpsk_plugin::QpskPlugin::new()), "QPSK500", 1500.0),
        (Box::new(psk8_plugin::Psk8Plugin::new()), "8PSK500", 1500.0),
        (
            Box::new(qam64_plugin::Qam64Plugin::new()),
            "64QAM500",
            1500.0,
        ),
        (Box::new(ofdm_plugin::OfdmPlugin::new()), "OFDM16", 1500.0),
        (Box::new(ofdm_plugin::OfdmPlugin::new()), "OFDM52", 1500.0),
        (
            Box::new(scfdma_plugin::ScFdmaPlugin::new()),
            "SCFDMA16",
            1500.0,
        ),
        (
            Box::new(scfdma_plugin::ScFdmaPlugin::new()),
            "SCFDMA52",
            1500.0,
        ),
    ];
    for (plugin, mode, fc) in &cases {
        assert_conformance(plugin.as_ref(), mode, *fc);
    }
}
