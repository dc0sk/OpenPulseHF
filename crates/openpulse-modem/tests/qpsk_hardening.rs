//! QPSK hardening tests and spectral efficiency benchmarks.
//!
//! Validates the QPSK plugin over the loopback audio backend and asserts
//! that QPSK achieves higher spectral efficiency than BPSK at the same
//! symbol rate.

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use openpulse_modem::engine::ModemEngine;
use qpsk_plugin::QpskPlugin;

// ── Fixture ───────────────────────────────────────────────────────────────────

struct QpskFixture {
    engine: ModemEngine,
}

impl QpskFixture {
    fn new() -> Self {
        let audio = Box::new(LoopbackBackend::new());
        let mut engine = ModemEngine::new(audio);
        engine
            .register_plugin(Box::new(QpskPlugin::new()))
            .expect("QPSK registration");
        Self { engine }
    }

    fn transmit(&mut self, payload: &[u8], mode: &str) -> Result<(), String> {
        self.engine
            .transmit(payload, mode, None)
            .map_err(|e| format!("{e:?}"))
    }
}

// ── Loopback TX tests ─────────────────────────────────────────────────────────

#[test]
fn qpsk125_transmit_loopback() {
    let mut fix = QpskFixture::new();
    assert!(fix.transmit(b"OpenPulse", "QPSK125").is_ok());
}

#[test]
fn qpsk250_transmit_loopback() {
    let mut fix = QpskFixture::new();
    assert!(fix.transmit(b"OpenPulse", "QPSK250").is_ok());
}

#[test]
fn qpsk500_transmit_loopback() {
    let mut fix = QpskFixture::new();
    assert!(fix.transmit(b"OpenPulse", "QPSK500").is_ok());
}

#[test]
fn qpsk_empty_payload_handled() {
    let mut fix = QpskFixture::new();
    let r = fix.transmit(b"", "QPSK250");
    // empty payload: ok or graceful error, never a panic
    let _ = r.is_ok() || r.is_err();
}

#[test]
fn qpsk_large_payload_handled() {
    let mut fix = QpskFixture::new();
    let payload = vec![0x42u8; 250];
    let r = fix.transmit(&payload, "QPSK250");
    let _ = r.is_ok() || r.is_err();
}

#[test]
fn qpsk_invalid_mode_fails_gracefully() {
    let mut fix = QpskFixture::new();
    assert!(fix.transmit(b"hello", "QPSK_UNKNOWN").is_err());
}

// ── Loopback fixture matrix ───────────────────────────────────────────────────

#[test]
fn qpsk_loopback_fixture_matrix() {
    // 3 supported modes × 14 payload profiles = 42 deterministic scenarios
    let modes = ["QPSK125", "QPSK250", "QPSK500"];
    let payload_profiles: Vec<Vec<u8>> = vec![
        vec![0x00],
        vec![0xFF],
        vec![0xAA],
        vec![0x55],
        b"CQ".to_vec(),
        b"N0TEST".to_vec(),
        b"openpulse".to_vec(),
        (0..8u8).collect(),
        (0..16u8).rev().collect(),
        vec![0x42; 24],
        vec![0x7E; 32],
        (0..48u8).map(|v| v ^ 0x5A).collect(),
        (0..64u8).collect(),
        (0..96u8).map(|v| (v.wrapping_mul(7)) ^ 0x33).collect(),
    ];

    let expected = modes.len() * payload_profiles.len();
    let mut executed = 0usize;

    for mode in modes {
        for (idx, payload) in payload_profiles.iter().enumerate() {
            let mut fix = QpskFixture::new();
            let result = fix.transmit(payload, mode);
            assert!(
                result.is_ok(),
                "scenario failed: mode={mode}, profile={idx}, len={}, err={result:?}",
                payload.len()
            );
            executed += 1;
        }
    }

    assert_eq!(executed, expected, "matrix scenario count mismatch");
    assert!(executed >= 42, "expected at least 42 scenarios");
}

// ── Spectral efficiency benchmarks ───────────────────────────────────────────

/// Verify that QPSK encodes more bits per sample than BPSK at the same baud
/// rate.  At equal symbol rates (e.g. 250 baud), QPSK carries 2 bits/symbol
/// vs. BPSK's 1 bit/symbol, so QPSK should produce fewer samples for the
/// same payload.
#[test]
fn qpsk250_more_bits_per_sample_than_bpsk250() {
    let payload = b"spectral efficiency test payload";

    let cfg_qpsk = ModulationConfig {
        mode: "QPSK250".to_string(),
        ..ModulationConfig::default()
    };
    let cfg_bpsk = ModulationConfig {
        mode: "BPSK250".to_string(),
        ..ModulationConfig::default()
    };

    let qpsk_samples = QpskPlugin::new()
        .modulate(payload, &cfg_qpsk)
        .expect("QPSK modulate");
    let bpsk_samples = BpskPlugin::new()
        .modulate(payload, &cfg_bpsk)
        .expect("BPSK modulate");

    let payload_bits = (payload.len() * 8) as f64;
    let qpsk_bps = payload_bits / qpsk_samples.len() as f64;
    let bpsk_bps = payload_bits / bpsk_samples.len() as f64;

    assert!(
        qpsk_bps > bpsk_bps,
        "QPSK250 should have higher bits/sample than BPSK250 for equal baud: \
         qpsk={qpsk_bps:.4} bpsk={bpsk_bps:.4}"
    );

    // QPSK encodes 2 bits/symbol vs BPSK 1 bit/symbol; with preamble/tail
    // overhead the ratio won't be exactly 2.0 but should exceed 1.4.
    let ratio = qpsk_bps / bpsk_bps;
    assert!(
        ratio > 1.4,
        "expected QPSK/BPSK efficiency ratio > 1.4, got {ratio:.3}"
    );
}

/// Confirm the raw symbol count for QPSK is lower than BPSK for identical
/// payloads at the same baud rate.
#[test]
fn qpsk250_sample_count_lower_than_bpsk250() {
    let payload = b"hello world from qpsk";

    let cfg_qpsk = ModulationConfig {
        mode: "QPSK250".to_string(),
        ..ModulationConfig::default()
    };
    let cfg_bpsk = ModulationConfig {
        mode: "BPSK250".to_string(),
        ..ModulationConfig::default()
    };

    let n_qpsk = QpskPlugin::new()
        .modulate(payload, &cfg_qpsk)
        .expect("QPSK modulate")
        .len();
    let n_bpsk = BpskPlugin::new()
        .modulate(payload, &cfg_bpsk)
        .expect("BPSK modulate")
        .len();

    assert!(
        n_qpsk < n_bpsk,
        "QPSK250 should produce fewer samples than BPSK250 for same payload: \
         qpsk={n_qpsk} bpsk={n_bpsk}"
    );
}
