//! Default-on CE-SSB must not break OFDM+FEC on a clean channel for low-entropy / RS-padded payloads.
//!
//! A zero-run or repeated-byte payload maps every OFDM subcarrier to the same point → an impulse-train
//! symbol whose PAPR the CE-SSB peak-stretch conditioner crushes, failing the decode on a PERFECT channel.
//! The OFDM plugin's bit-stream whitening (`ofdm_plugin::scramble`) decorrelates the subcarriers so no
//! payload can produce that symbol. This pins that a padded/low-entropy frame still decodes with CE-SSB on.

use ofdm_plugin::OfdmPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::fec::FecMode;
use openpulse_modem::ModemEngine;

fn roundtrip_ok(payload: &[u8], cessb: bool) -> bool {
    let mut e = ModemEngine::new(Box::new(LoopbackBackend::new()));
    e.register_plugin(Box::new(OfdmPlugin::new())).unwrap();
    e.set_cessb_enabled(cessb);
    if e.transmit_with_fec_mode(payload, "OFDM52", FecMode::Rs, None)
        .is_err()
    {
        return false;
    }
    match e.receive_with_fec_mode("OFDM52", FecMode::Rs, None) {
        Ok(rx) => rx.len() >= payload.len() && &rx[..payload.len()] == payload,
        Err(_) => false,
    }
}

#[test]
fn ofdm_low_entropy_payloads_decode_with_default_cessb() {
    // These all collapse the OFDM subcarriers to (near-)identical points pre-whitening.
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("64B all-zero", vec![0u8; 64]),
        ("213B zeros (pads a block)", vec![0u8; 213]),
        ("64B repeated 0x5A", vec![0x5Au8; 64]),
        ("128B repeated 0xFF", vec![0xFFu8; 128]),
    ];
    for (name, p) in &cases {
        assert!(
            roundtrip_ok(p, true),
            "{name}: OFDM52+Rs must decode on a clean channel with CE-SSB ON (whitening scrambler)"
        );
    }
}

/// The whitening must not disturb the normal high-entropy path either (both CE-SSB states).
#[test]
fn ofdm_high_entropy_still_roundtrips_both_cessb_states() {
    let payload: Vec<u8> = (0..200u32)
        .map(|i| (i.wrapping_mul(37) & 0xff) as u8)
        .collect();
    assert!(roundtrip_ok(&payload, true), "CE-SSB on");
    assert!(roundtrip_ok(&payload, false), "CE-SSB off");
}

/// Exercise the SOFT descramble path end-to-end: OFDM52 + SoftConcatenated with a low-entropy payload
/// and CE-SSB on must still decode (this routes through `descramble_llrs`, not the hard byte XOR).
#[test]
fn ofdm_low_entropy_soft_fec_decodes_with_default_cessb() {
    let mut e = ModemEngine::new(Box::new(LoopbackBackend::new()));
    e.register_plugin(Box::new(OfdmPlugin::new())).unwrap();
    e.set_cessb_enabled(true);
    let payload = vec![0u8; 96];
    e.transmit_with_fec_mode(&payload, "OFDM52", FecMode::SoftConcatenated, None)
        .unwrap();
    let rx = e
        .receive_with_fec_mode("OFDM52", FecMode::SoftConcatenated, None)
        .unwrap_or_default();
    assert!(
        rx.len() >= payload.len() && rx[..payload.len()] == payload[..],
        "low-entropy OFDM52 soft-FEC frame must decode with CE-SSB on"
    );
}
