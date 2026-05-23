//! FEC loopback hardening tests.
#![allow(clippy::needless_range_loop)]
//!
//! Exercises:
//!  1. FEC round-trips through the modem engine (BPSK + QPSK modes).
//!  2. Pure codec correctness: encode → inject byte errors → decode.
//!  3. Overhead assertion: FEC output is strictly larger than the raw input.
//!  4. Interleaved FEC loopback and Gilbert-Elliott burst scenario.
//!  5. Strong RS (t=32) codec and engine loopback (BL-FEC-2).
//!  6. Memory-ARQ soft combining engine loopback (BL-FEC-4).

use bpsk_plugin::BpskPlugin;
use fsk4_plugin::Fsk4Plugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::ack::{AckFrame, AckType};
use openpulse_core::conv::ConvCodec;
use openpulse_core::fec::{
    FecCodec, Interleaver, ShortFecCodec, SoftCombiner, DEFAULT_INTERLEAVER_DEPTH,
};
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
        &[0x00; 219], // exactly fills one block (219 = BLOCK_DATA_STANDARD - PREFIX_LEN)
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

// ── Concatenated Conv + RS FEC ────────────────────────────────────────────────

#[test]
fn concatenated_fec_bpsk250_loopback() {
    let mut engine = engine_with_both_plugins();
    let payload = b"concatenated FEC over BPSK250";
    engine
        .transmit_with_concatenated_fec(payload, "BPSK250", None)
        .unwrap();
    let received = engine
        .receive_with_concatenated_fec("BPSK250", None)
        .unwrap();
    assert_eq!(received, payload);
}

#[test]
fn concatenated_fec_qpsk250_loopback() {
    let mut engine = engine_with_both_plugins();
    let payload = b"concatenated FEC over QPSK250";
    engine
        .transmit_with_concatenated_fec(payload, "QPSK250", None)
        .unwrap();
    let received = engine
        .receive_with_concatenated_fec("QPSK250", None)
        .unwrap();
    assert_eq!(received, payload);
}

/// Concatenated codec: errors injected into the RS layer (simulating residual
/// Viterbi output errors) are corrected by the outer RS code.
#[test]
fn concatenated_codec_corrects_random_errors() {
    let payload = b"residual error correction test";
    let rs_bytes = FecCodec::new().encode(payload);

    // Inject 8 byte errors into the RS-encoded data (≤16 per block = within
    // RS correction capacity). Conv encodes them faithfully; after Conv
    // decode the corrupted RS bytes come back; RS then corrects them.
    let mut rs_corrupted = rs_bytes.clone();
    for i in 0..8usize {
        rs_corrupted[i * 5] ^= 0xA5;
    }

    let conv_bytes = ConvCodec::new().encode(&rs_corrupted);
    let conv_decoded = ConvCodec::new().decode(&conv_bytes).unwrap();
    let recovered = FecCodec::new().decode(&conv_decoded).unwrap();
    assert_eq!(recovered, payload);
}

/// Concatenated overhead must be strictly larger than RS-only overhead.
/// Conv rate-1/2 approximately doubles the byte count (plus a small K-1 tail).
#[test]
fn concatenated_fec_overhead_is_positive() {
    let payload = b"overhead sanity check for concatenated FEC";
    let rs_only = FecCodec::new().encode(payload);
    let concatenated = ConvCodec::new().encode(&rs_only);
    assert!(
        concatenated.len() > rs_only.len(),
        "concatenated output ({} bytes) must be larger than RS-only output ({} bytes)",
        concatenated.len(),
        rs_only.len()
    );
    // Conv rate-1/2 approximately doubles; allow a small tail (≤16 extra bytes).
    assert!(
        concatenated.len() >= rs_only.len() * 2,
        "concatenated output should be at least 2× the RS-only size"
    );
}

// ── Short-block RS (ShortFecCodec) ────────────────────────────────────────────

/// 5-byte ACK frame encodes to 13 bytes (5 + 8 ECC), not 255.
#[test]
fn short_fec_overhead_is_13_bytes_for_ack_frame() {
    let codec = ShortFecCodec::new();
    let payload = [0u8; 5];
    let encoded = codec.encode(&payload).unwrap();
    assert_eq!(
        encoded.len(),
        13,
        "5-byte ACK frame should encode to 13 bytes, got {}",
        encoded.len()
    );
}

/// ShortFecCodec round-trip with zero injected errors.
#[test]
fn short_fec_codec_round_trip() {
    let codec = ShortFecCodec::new();
    let payloads: &[&[u8]] = &[
        b"hello",
        &[0x00; 5],
        &[0xFF; 5],
        b"short FEC test payload 12345678",
    ];
    for payload in payloads {
        let enc = codec.encode(payload).unwrap();
        let dec = codec.decode(&enc).unwrap();
        assert_eq!(
            &dec,
            payload,
            "round-trip failed for {} bytes",
            payload.len()
        );
    }
}

/// ShortFecCodec corrects up to 4 byte errors (t = 4).
#[test]
fn short_fec_codec_corrects_4_byte_errors() {
    let codec = ShortFecCodec::new();
    let payload = b"hello";
    let mut encoded = codec.encode(payload).unwrap();
    // Flip 4 bytes spread across the encoded buffer.
    for i in 0..4 {
        encoded[i * 3] ^= 0xA5;
    }
    let recovered = codec
        .decode(&encoded)
        .expect("should correct ≤4 byte errors");
    assert_eq!(recovered, payload);
}

fn engine_with_fsk4() -> ModemEngine {
    let audio = Box::new(LoopbackBackend::new());
    let mut engine = ModemEngine::new(audio);
    engine
        .register_plugin(Box::new(Fsk4Plugin::new()))
        .expect("FSK4 registration");
    engine
}

/// AckFrame survives FSK4-ACK engine loopback with ShortFEC applied.
#[test]
fn ack_frame_short_fec_engine_loopback() {
    let mut engine = engine_with_fsk4();
    let ack = AckFrame::new(AckType::AckOk, "test-session");
    engine.transmit_ack_with_short_fec(&ack, None).unwrap();
    let received = engine.receive_ack_with_short_fec(None).unwrap();
    assert_eq!(received.ack_type, ack.ack_type);
    assert_eq!(received.session_hash, ack.session_hash);
}

/// ShortRs data-frame engine loopback: a 32-byte payload survives BPSK250
/// round-trip via `FecMode::ShortRs`, and the wire size is `payload + 32`
/// rather than a full 255-byte block.
#[test]
fn short_fec_data_frame_engine_loopback() {
    use openpulse_core::fec::FecMode;

    let mut engine = engine_with_both_plugins();
    let payload: Vec<u8> = (0..32u8).collect();

    engine
        .transmit_with_fec_mode(&payload, "BPSK250", FecMode::ShortRs, None)
        .expect("ShortRs transmit");
    let received = engine
        .receive_with_fec_mode("BPSK250", FecMode::ShortRs, None)
        .expect("ShortRs receive");
    assert_eq!(received, payload);

    // Wire-size sanity: ShortRs(32 ECC) of 32 B payload = 64 B, well under
    // the 255 B that `FecMode::Rs` would emit.
    let encoded = ShortFecCodec::with_ecc_len(32).encode(&payload).unwrap();
    assert_eq!(encoded.len(), payload.len() + 32);
    assert!(encoded.len() < 255);
}

/// ShortRs data-frame path rejects payloads larger than 223 B.
#[test]
fn short_fec_data_frame_rejects_oversized_payload() {
    use openpulse_core::fec::FecMode;

    let mut engine = engine_with_both_plugins();
    let oversized = vec![0u8; 224];
    let err = engine
        .transmit_with_fec_mode(&oversized, "BPSK250", FecMode::ShortRs, None)
        .expect_err("224-byte payload must be rejected");
    assert!(format!("{err}").contains("exceeds maximum"));
}

// ── Strong RS (BL-FEC-2) ──────────────────────────────────────────────────────

/// Strong RS(255,191) encodes to multiples of 255 bytes with 25% ECC overhead.
#[test]
fn strong_fec_encode_produces_255_byte_blocks() {
    let codec = FecCodec::strong();
    let payload = b"strong RS overhead test";
    let encoded = codec.encode(payload);
    assert_eq!(
        encoded.len() % 255,
        0,
        "strong FEC output must be a multiple of 255 bytes"
    );
    assert!(
        encoded.len() > payload.len(),
        "strong FEC output must be larger than input"
    );
}

/// Strong RS codec round-trip with no injected errors.
#[test]
fn strong_fec_codec_round_trip() {
    let codec = FecCodec::strong();
    let payloads: &[&[u8]] = &[
        b"",
        b"hello",
        &[0xFF; 100],
        &[0x00; 187], // exactly fills one strong block (191 - 4 prefix = 187 bytes)
        &[0xAB; 188], // spills into two strong blocks
    ];
    for payload in payloads {
        let enc = codec.encode(payload);
        let dec = codec.decode(&enc).unwrap();
        assert_eq!(
            &dec,
            payload,
            "round-trip failed for {} bytes",
            payload.len()
        );
    }
}

/// Strong RS corrects up to 32 byte errors per block.
#[test]
fn strong_fec_corrects_up_to_32_byte_errors() {
    let codec = FecCodec::strong();
    let payload = b"strong RS error correction test payload abcdef";
    let mut encoded = codec.encode(payload);
    // Flip 32 bytes spread across the first block.
    for i in 0..32 {
        encoded[i * 5 % 255] ^= 0xA5;
    }
    let recovered = codec
        .decode(&encoded)
        .expect("should correct ≤32 byte errors");
    assert_eq!(recovered, payload);
}

/// Strong RS engine loopback: BPSK250 encode → strong RS → decode.
#[test]
fn strong_fec_bpsk250_loopback() {
    let mut engine = engine_with_both_plugins();
    let payload = b"strong FEC over BPSK250";
    engine
        .transmit_with_strong_fec(payload, "BPSK250", None)
        .unwrap();
    let received = engine.receive_with_strong_fec("BPSK250", None).unwrap();
    assert_eq!(received, payload);
}

/// Strong RS corrects error patterns that exceed the standard RS capacity.
///
/// Injects 20 consecutive byte errors — beyond standard t=16, within strong t=32.
#[test]
fn strong_fec_corrects_where_standard_cannot() {
    let payload = b"overhead comparison payload";
    let error_count = 20;

    // Standard RS (t=16) should fail with 20 errors.
    let mut std_encoded = FecCodec::new().encode(payload);
    for byte in std_encoded.iter_mut().take(error_count) {
        *byte ^= 0xFF;
    }
    assert!(
        FecCodec::new().decode(&std_encoded).is_err(),
        "standard RS (t=16) should fail with {error_count} errors"
    );

    // Strong RS (t=32) should correct the same error count.
    let mut strong_encoded = FecCodec::strong().encode(payload);
    for byte in strong_encoded.iter_mut().take(error_count) {
        *byte ^= 0xFF;
    }
    let recovered = FecCodec::strong()
        .decode(&strong_encoded)
        .expect("strong RS (t=32) should correct ≤32 byte errors");
    assert_eq!(recovered, payload);
}

// ── Memory-ARQ soft combining (BL-FEC-4) ─────────────────────────────────────

/// SoftCombiner returns an empty Vec before any pushes.
#[test]
fn soft_combiner_empty_returns_empty() {
    let c = SoftCombiner::new();
    assert!(c.combine().is_empty());
    assert_eq!(c.count(), 0);
}

/// SoftCombiner with a single push is an identity operation.
#[test]
fn soft_combiner_single_push_is_identity() {
    let mut c = SoftCombiner::new();
    let samples = vec![1.0f32, 2.0, 3.0];
    c.push(&samples);
    assert_eq!(c.count(), 1);
    let combined = c.combine();
    assert_eq!(combined.len(), 3);
    for (a, b) in combined.iter().zip(&samples) {
        assert!(
            (a - b).abs() < 1e-6,
            "single-push combine should be identity"
        );
    }
}

/// SoftCombiner with N equal pushes divides by N, reducing to original.
#[test]
fn soft_combiner_equal_pushes_averages_correctly() {
    let mut c = SoftCombiner::new();
    let samples = vec![4.0f32, 8.0, 12.0];
    for _ in 0..4 {
        c.push(&samples);
    }
    assert_eq!(c.count(), 4);
    let combined = c.combine();
    for (a, &b) in combined.iter().zip(&samples) {
        assert!(
            (a - b).abs() < 1e-6,
            "N equal pushes should reproduce original"
        );
    }
}

/// Soft combining engine loopback with n=1 (equivalent to normal receive).
#[test]
fn soft_combining_n1_loopback() {
    let mut engine = engine_with_both_plugins();
    let payload = b"soft combining n=1 loopback";
    engine.transmit_with_fec(payload, "BPSK250", None).unwrap();
    let received = engine
        .receive_with_soft_combining("BPSK250", None, 1)
        .unwrap();
    assert_eq!(received, payload);
}

/// Soft combining engine loopback with n=3 (three identical frames buffered).
#[test]
fn soft_combining_n3_loopback() {
    // The loopback backend drains its entire buffer on each capture call, so
    // three consecutive captures cannot each return one frame independently.
    // Instead: transmit once to materialise the encoded frame samples, then
    // push those same samples through a SoftCombiner three times externally,
    // re-fill the loopback with the combined (identical → unchanged) output,
    // and verify the engine's receive + FEC decode path recovers the payload.
    let backend = LoopbackBackend::new();
    let shared = backend.clone_shared();
    let mut engine = ModemEngine::new(Box::new(backend));
    engine.register_plugin(Box::new(BpskPlugin::new())).unwrap();

    let payload = b"soft combining n=3 loopback";
    engine.transmit_with_fec(payload, "BPSK250", None).unwrap();

    // Drain the one frame's worth of samples from the loopback.
    let frame_samples = shared.drain_samples();
    assert!(
        !frame_samples.is_empty(),
        "loopback should contain encoded frame samples"
    );

    // Combine three identical copies — averaging N identical signals returns
    // the original signal, so the demodulator sees the same frame unchanged.
    let mut combiner = SoftCombiner::new();
    for _ in 0..3 {
        combiner.push(&frame_samples);
    }
    assert_eq!(combiner.count(), 3);

    // Inject the combined samples back into the loopback for the engine to read.
    shared.fill_samples(&combiner.combine());

    // n_frames = 1 because one combined-frame buffer is in the loopback;
    // the multi-frame combining logic is verified by the SoftCombiner unit tests.
    let received = engine
        .receive_with_soft_combining("BPSK250", None, 1)
        .unwrap();
    assert_eq!(received, payload);
}
