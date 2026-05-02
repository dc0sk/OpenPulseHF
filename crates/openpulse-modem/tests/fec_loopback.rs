//! FEC loopback hardening tests.
//!
//! Exercises:
//!  1. FEC round-trips through the modem engine (BPSK + QPSK modes).
//!  2. Pure codec correctness: encode → inject byte errors → decode.
//!  3. Overhead assertion: FEC output is strictly larger than the raw input.
//!  4. Interleaved FEC loopback and Gilbert-Elliott burst scenario.

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::fec::{FecCodec, Interleaver, DEFAULT_INTERLEAVER_DEPTH};
use openpulse_modem::engine::ModemEngine;
use qpsk_plugin::QpskPlugin;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn engine_with_both_plugins() -> ModemEngine {
    let audio = Box::new(LoopbackBackend::new());
    let mut engine = ModemEngine::new(audio);
    engine
        .register_plugin(Box::new(BpskPlugin::new()))
        .expect("BPSK registration");
    engine
        .register_plugin(Box::new(QpskPlugin::new()))
        .expect("QPSK registration");
    engine
}

// ── Engine loopback (transmit_with_fec / receive_with_fec) ───────────────────

#[test]
fn fec_bpsk100_loopback() {
    let mut engine = engine_with_both_plugins();
    engine
        .transmit_with_fec(b"FEC over BPSK100", "BPSK100", None)
        .unwrap();
    let received = engine.receive_with_fec("BPSK100", None).unwrap();
    assert_eq!(received, b"FEC over BPSK100");
}

#[test]
fn fec_bpsk250_loopback() {
    let mut engine = engine_with_both_plugins();
    engine
        .transmit_with_fec(b"FEC over BPSK250", "BPSK250", None)
        .unwrap();
    let received = engine.receive_with_fec("BPSK250", None).unwrap();
    assert_eq!(received, b"FEC over BPSK250");
}

#[test]
fn fec_qpsk250_loopback() {
    let mut engine = engine_with_both_plugins();
    engine
        .transmit_with_fec(b"FEC over QPSK250", "QPSK250", None)
        .unwrap();
    let received = engine.receive_with_fec("QPSK250", None).unwrap();
    assert_eq!(received, b"FEC over QPSK250");
}

#[test]
fn fec_qpsk500_loopback() {
    let mut engine = engine_with_both_plugins();
    engine
        .transmit_with_fec(b"FEC over QPSK500", "QPSK500", None)
        .unwrap();
    let received = engine.receive_with_fec("QPSK500", None).unwrap();
    assert_eq!(received, b"FEC over QPSK500");
}

#[test]
fn fec_loopback_large_payload() {
    let mut engine = engine_with_both_plugins();
    // 200 bytes — exercises multi-block FEC path.
    let payload: Vec<u8> = (0..200u8).collect();
    engine.transmit_with_fec(&payload, "BPSK250", None).unwrap();
    let received = engine.receive_with_fec("BPSK250", None).unwrap();
    assert_eq!(received, payload);
}

// ── Codec-level BER injection ─────────────────────────────────────────────────

/// Verify that the codec recovers when ≤ 16 bytes per block are corrupted.
#[test]
fn fec_codec_corrects_up_to_16_errors_per_block() {
    let codec = FecCodec::new();
    let original = b"bit error recovery test 1234567890";
    let mut encoded = codec.encode(original);

    // Flip 16 bytes spread across the first 255-byte block.
    for i in 0..16 {
        encoded[i * 3] ^= 0xA5;
    }

    let recovered = codec
        .decode(&encoded)
        .expect("should correct ≤16 byte errors");
    assert_eq!(recovered, original);
}

/// Verify that the codec returns an error when more than 16 bytes per block
/// are corrupted (beyond the error-correction capacity).
#[test]
fn fec_codec_fails_beyond_capacity() {
    let codec = FecCodec::new();
    let original = b"uncorrectable damage";
    let mut encoded = codec.encode(original);

    // Corrupt 20 consecutive bytes — exceeds the 16-byte correction limit.
    for byte in encoded.iter_mut().take(20) {
        *byte ^= 0xFF;
    }

    assert!(
        codec.decode(&encoded).is_err(),
        "should fail when errors exceed correction capacity"
    );
}

/// Verify FEC round-trip with zero errors (sanity check for the codec alone,
/// independent of the modem engine).
#[test]
fn fec_codec_round_trip_no_errors() {
    let codec = FecCodec::new();
    let payloads: &[&[u8]] = &[
        b"",
        b"a",
        b"hello",
        b"OpenPulse FEC phase 1",
        &[0xFF; 100],
        &[0x00; 219], // exactly fills one block (219 = BLOCK_DATA - PREFIX_LEN)
        &[0xAB; 220], // spills into two blocks
    ];

    for payload in payloads {
        let enc = codec.encode(payload);
        let dec = codec.decode(&enc).unwrap();
        assert_eq!(
            &dec,
            payload,
            "round-trip failed for payload of {} bytes",
            payload.len()
        );
    }
}

/// Verify that FEC-encoded output is always larger than raw input (overhead
/// sanity check — ensures FEC is actually being applied).
#[test]
fn fec_encode_overhead_is_positive() {
    let codec = FecCodec::new();
    let payload = b"overhead check";
    let encoded = codec.encode(payload);
    assert!(
        encoded.len() > payload.len(),
        "FEC output ({} bytes) should be larger than raw input ({} bytes)",
        encoded.len(),
        payload.len()
    );
    assert_eq!(
        encoded.len() % 255,
        0,
        "FEC output must be a multiple of 255 bytes"
    );
}

// ── Fixture matrix ────────────────────────────────────────────────────────────

/// 2 modes × 10 payload profiles = 20 deterministic FEC loopback scenarios.
#[test]
fn fec_loopback_fixture_matrix() {
    let modes = ["BPSK250", "QPSK250"];
    let profiles: Vec<Vec<u8>> = vec![
        b"CQ DE N0TEST".to_vec(),
        vec![0x00; 1],
        vec![0xFF; 1],
        (0..16u8).collect(),
        (0..32u8).rev().collect(),
        vec![0x42; 50],
        (0..100u8).map(|v| v ^ 0x5A).collect(),
        vec![0xAA; 128],
        (0..160u8).map(|v| v.wrapping_mul(3)).collect(),
        (0..200u8).collect(),
    ];

    let expected = modes.len() * profiles.len();
    let mut executed = 0usize;

    for mode in modes {
        for (idx, payload) in profiles.iter().enumerate() {
            let mut engine = engine_with_both_plugins();
            engine
                .transmit_with_fec(payload, mode, None)
                .unwrap_or_else(|e| panic!("TX failed: mode={mode} idx={idx}: {e:?}"));
            let received = engine
                .receive_with_fec(mode, None)
                .unwrap_or_else(|e| panic!("RX failed: mode={mode} idx={idx}: {e:?}"));
            assert_eq!(
                received, *payload,
                "payload mismatch: mode={mode} idx={idx}"
            );
            executed += 1;
        }
    }

    assert_eq!(executed, expected);
}

// ── Interleaved FEC ───────────────────────────────────────────────────────────

/// Basic engine loopback with FEC + interleaving — no injected errors.
#[test]
fn fec_interleaved_bpsk250_loopback() {
    let mut engine = engine_with_both_plugins();
    let payload = b"interleaved FEC loopback test";
    engine
        .transmit_with_fec_interleaved(payload, "BPSK250", None, DEFAULT_INTERLEAVER_DEPTH)
        .unwrap();
    let received = engine
        .receive_with_fec_interleaved("BPSK250", None, DEFAULT_INTERLEAVER_DEPTH)
        .unwrap();
    assert_eq!(received, payload);
}

/// Gilbert-Elliott moderate-burst scenario: five 20-byte bursts spread across
/// the interleaved buffer (matching the GE moderate-burst profile mean burst
/// length of 20 symbols).  After deinterleaving and RS correction the original
/// payload must be recovered.
///
/// Each burst of 20 bytes distributes to ≈ 2 errors per RS block across the
/// 10-block encoded buffer — within the 16-byte RS correction capacity.
#[test]
fn fec_interleaved_ge_moderate_burst_scenario() {
    let codec = FecCodec::new();
    let il = Interleaver::new(DEFAULT_INTERLEAVER_DEPTH);
    // 10 RS blocks so burst distribution stays safely under the correction limit.
    let payload: Vec<u8> = (0..2190u16).map(|v| (v & 0xFF) as u8).collect();

    let encoded = codec.encode(&payload);
    assert_eq!(encoded.len(), 2550, "expected 10 RS blocks");
    let interleaved = il.interleave(&encoded);

    // Inject 5 bursts of 20 bytes (GE moderate-burst mean burst length),
    // evenly spaced to simulate multiple independent burst events.
    let mut corrupted = interleaved.clone();
    let burst_len = 20;
    let spacing = encoded.len() / 5; // 510-byte gap between bursts
    for b in 0..5 {
        let offset = b * spacing + 10;
        for i in offset..offset + burst_len {
            corrupted[i] ^= 0xFF;
        }
    }

    let deinterleaved = il.deinterleave(&corrupted);
    let recovered = codec.decode(&deinterleaved).unwrap();
    assert_eq!(recovered, payload);
}
