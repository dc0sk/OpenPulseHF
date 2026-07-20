//! Soft-FEC activation for the pilot family.
//!
//! Now that the pilot plugin emits genuine LLRs (`supports_soft_demod` = true),
//! the engine's soft-FEC paths — including the rate-8/9 high-rate PEG LDPC and
//! rate-1/2 LDPC — work on the pilot dense rungs instead of falling back to the
//! ±1.0 hard LLRs. These clean-loopback round-trips prove the soft path is wired
//! end to end through the engine FEC dispatch.

use openpulse_audio::LoopbackBackend;
use openpulse_core::fec::FecMode;
use openpulse_core::plugin::ModulationPlugin;
use openpulse_modem::engine::ModemEngine;
use pilot_plugin::PilotPlugin;

fn engine() -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(LoopbackBackend::new()));
    e.register_plugin(Box::new(PilotPlugin::new()))
        .expect("pilot registration");
    e
}

#[test]
fn pilot_plugin_is_soft_capable() {
    assert!(
        PilotPlugin::new().supports_soft_demod("PILOT-QPSK500"),
        "pilot plugin must advertise soft demod so the engine feeds it to soft FEC"
    );
}

#[test]
fn high_rate_ldpc_over_pilot_8psk500() {
    let mut e = engine();
    let payload = b"pilot soft-FEC: high-rate LDPC";
    e.transmit_with_ldpc_high_rate(payload, "PILOT-8PSK500", None)
        .expect("transmit high-rate LDPC over PILOT-8PSK500");
    let got = e
        .receive_with_ldpc_high_rate("PILOT-8PSK500", None)
        .expect("receive high-rate LDPC over PILOT-8PSK500");
    assert_eq!(&got[..payload.len()], payload);
}

#[test]
fn rate_half_ldpc_over_pilot_16qam500() {
    let mut e = engine();
    let payload = b"pilot soft-FEC: rate-1/2 LDPC";
    e.transmit_with_fec_mode(payload, "PILOT-16QAM500", FecMode::Ldpc, None)
        .expect("transmit rate-1/2 LDPC over PILOT-16QAM500");
    let got = e
        .receive_with_fec_mode("PILOT-16QAM500", FecMode::Ldpc, None)
        .expect("receive rate-1/2 LDPC over PILOT-16QAM500");
    assert_eq!(&got[..payload.len()], payload);
}

#[test]
fn soft_fec_over_pilot_rrc() {
    // The RRC variants are also soft-capable (recover_symbols feeds the matched
    // RRC filter output to the same per-bit LLR demapper): narrowband + coding.
    let mut e = engine();
    let payload = b"pilot soft-FEC over the narrowband RRC variant";
    e.transmit_with_fec_mode(payload, "PILOT-16QAM500-RRC", FecMode::Ldpc, None)
        .expect("transmit rate-1/2 LDPC over PILOT-16QAM500-RRC");
    let got = e
        .receive_with_fec_mode("PILOT-16QAM500-RRC", FecMode::Ldpc, None)
        .expect("receive rate-1/2 LDPC over PILOT-16QAM500-RRC");
    assert_eq!(&got[..payload.len()], payload);
}
