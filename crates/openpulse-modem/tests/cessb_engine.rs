//! Engine wiring for the CE-SSB TX envelope conditioner.
//!
//! The DSP-level power/EVM measurement lives in `cessb_power_evm.rs`; here we
//! verify the engine toggle, the per-mode benefit predicate, and that enabling
//! CE-SSB does not break a clean loopback round-trip (decode integrity).

use bpsk_plugin::BpskPlugin;
use ofdm_plugin::OfdmPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_modem::ModemEngine;

fn engine_with(plugin: impl openpulse_core::plugin::ModulationPlugin + 'static) -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(LoopbackBackend::new()));
    e.register_plugin(Box::new(plugin)).unwrap();
    e
}

#[test]
fn benefits_only_low_order_ofdm_modes() {
    // Only QPSK-subcarrier OFDM: CE-SSB is genuinely zero-EVM-cost there (peak-fair BER 0→0).
    assert!(ModemEngine::cessb_benefits("OFDM52"));
    assert!(ModemEngine::cessb_benefits("ofdm16"));

    // Every higher-order OFDM constellation (8PSK and up) is gated off — the clip EVM exceeds
    // the tighter decision regions and real-path decode breaks at the operating SNR. 8PSK: a
    // marginal-SNR AWGN sweep goes 12/12 → 0/12 with CE-SSB on (peak-fair BER 0.0000→0.0026);
    // 16QAM on Watterson Good-F1 0/16; 32QAM 0/20; 64QAM 3/20 vs ≥20/20 off.
    assert!(!ModemEngine::cessb_benefits("OFDM52-8PSK"));
    assert!(!ModemEngine::cessb_benefits("OFDM52-16QAM"));
    assert!(!ModemEngine::cessb_benefits("OFDM52-32QAM"));
    assert!(!ModemEngine::cessb_benefits("OFDM52-64QAM"));

    // SC-FDMA is single-carrier-FDM (low-PAPR by construction): CE-SSB buys little
    // power but its EVM collapses the dense rungs, so it must be excluded.
    assert!(!ModemEngine::cessb_benefits("SCFDMA52"));
    assert!(!ModemEngine::cessb_benefits("SCFDMA52-64QAM"));

    assert!(!ModemEngine::cessb_benefits("BPSK250"));
    assert!(!ModemEngine::cessb_benefits("QPSK500"));
    assert!(!ModemEngine::cessb_benefits("64QAM500"));
    assert!(!ModemEngine::cessb_benefits("8PSK1000"));
}

#[test]
fn enabled_by_default_and_toggles() {
    let mut e = engine_with(BpskPlugin::new());
    assert!(e.cessb_enabled());
    e.set_cessb_enabled(false);
    assert!(!e.cessb_enabled());
    e.set_cessb_enabled(true);
    assert!(e.cessb_enabled());
}

#[test]
fn ofdm_roundtrip_decodes_with_cessb_enabled() {
    let mut e = engine_with(OfdmPlugin::new());
    e.set_cessb_enabled(true);
    let payload = b"ce-ssb conditioned multicarrier";
    e.transmit(payload, "OFDM52", None).unwrap();
    let rx = e.receive("OFDM52", None).unwrap();
    assert_eq!(rx, payload);
}

#[test]
fn bpsk_roundtrip_is_noop_with_cessb_enabled() {
    let mut e = engine_with(BpskPlugin::new());
    e.set_cessb_enabled(true);
    let payload = b"single carrier untouched";
    e.transmit(payload, "BPSK250", None).unwrap();
    let rx = e.receive("BPSK250", None).unwrap();
    assert_eq!(rx, payload);
}
