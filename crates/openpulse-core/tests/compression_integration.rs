use openpulse_core::compression::{
    compress, compress_if_smaller, decompress, CompressionAlgorithm,
};
use openpulse_core::handshake::{verify_conack, verify_conreq, ConAck, ConReq, InMemoryTrustStore};
use openpulse_core::trust::{PolicyProfile, SigningMode};

// ------------------------------------------------------------------
// Group 1: Codec correctness
// ------------------------------------------------------------------

#[test]
fn none_compress_decompress_is_identity() {
    let payload = b"OpenPulseHF test payload";
    let c = compress(payload, CompressionAlgorithm::None);
    assert_eq!(c, payload);
    assert_eq!(decompress(&c, CompressionAlgorithm::None).unwrap(), payload);
}

#[test]
fn lz4_compress_decompress_round_trip() {
    let payload = vec![0x42u8; 1024];
    let compressed = compress(&payload, CompressionAlgorithm::Lz4);
    assert!(
        compressed.len() < payload.len(),
        "repetitive payload should compress"
    );
    assert_eq!(
        decompress(&compressed, CompressionAlgorithm::Lz4).unwrap(),
        payload
    );
}

#[test]
fn lz4_decompress_garbage_returns_error() {
    let garbage = vec![0xFFu8; 64];
    assert!(
        decompress(&garbage, CompressionAlgorithm::Lz4).is_err(),
        "decompressing garbage must return an error"
    );
}

// ------------------------------------------------------------------
// Group 2: compress-then-compare
// ------------------------------------------------------------------

#[test]
fn compress_if_smaller_picks_lz4_for_compressible_payload() {
    let payload = b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let (out, algo) = compress_if_smaller(payload);
    assert_eq!(algo, CompressionAlgorithm::Lz4);
    assert!(out.len() < payload.len());
    assert_eq!(decompress(&out, algo).unwrap(), payload.as_ref());
}

#[test]
fn compress_if_smaller_keeps_original_when_incompressible() {
    // Single byte or already-dense data should not be re-compressed.
    let payload = b"x";
    let (out, algo) = compress_if_smaller(payload);
    assert_eq!(algo, CompressionAlgorithm::None);
    assert_eq!(out, payload.as_ref());
}

// ------------------------------------------------------------------
// Group 3: Handshake negotiation
// ------------------------------------------------------------------

fn make_seed(b: u8) -> [u8; 32] {
    [b; 32]
}

#[test]
fn conreq_carries_supported_compression_in_signature() {
    let req = ConReq::create(
        "W1AW",
        &make_seed(1),
        vec![SigningMode::Normal],
        "sess-comp-1",
        vec![CompressionAlgorithm::Lz4],
    )
    .unwrap();

    assert_eq!(req.supported_compression, vec![CompressionAlgorithm::Lz4]);

    // Verify the signature covers the compression list.
    let store = InMemoryTrustStore::new();
    verify_conreq(&req, &store, PolicyProfile::Balanced, SigningMode::Normal)
        .expect("request with Lz4 compression should verify");
}

#[test]
fn conack_carries_selected_compression_in_signature() {
    let ack = ConAck::create(
        "KD9XYZ",
        &make_seed(2),
        SigningMode::Normal,
        "sess-comp-2",
        CompressionAlgorithm::Lz4,
    )
    .unwrap();

    assert_eq!(ack.selected_compression, CompressionAlgorithm::Lz4);

    let store = InMemoryTrustStore::new();
    verify_conack(
        &ack,
        "sess-comp-2",
        &store,
        PolicyProfile::Balanced,
        SigningMode::Normal,
    )
    .expect("ack with Lz4 compression should verify");
}

#[test]
fn full_negotiation_round_trip_with_lz4() {
    // Alice initiates, advertising Lz4 support.
    let req = ConReq::create(
        "W1AW",
        &make_seed(3),
        vec![SigningMode::Normal],
        "sess-comp-3",
        vec![CompressionAlgorithm::Lz4],
    )
    .unwrap();

    let mut bob_store = InMemoryTrustStore::new();
    bob_store.add_trusted(
        "W1AW",
        ed25519_dalek::SigningKey::from_bytes(&make_seed(3))
            .verifying_key()
            .to_bytes(),
    );

    let req_decision = verify_conreq(
        &req,
        &bob_store,
        PolicyProfile::Balanced,
        SigningMode::Normal,
    )
    .unwrap();

    // Bob selects Lz4 (it was offered).
    let selected_compression = if req
        .supported_compression
        .contains(&CompressionAlgorithm::Lz4)
    {
        CompressionAlgorithm::Lz4
    } else {
        CompressionAlgorithm::None
    };

    let ack = ConAck::create(
        "KD9XYZ",
        &make_seed(4),
        req_decision.selected_mode,
        &req.session_id,
        selected_compression,
    )
    .unwrap();

    assert_eq!(ack.selected_compression, CompressionAlgorithm::Lz4);

    let mut alice_store = InMemoryTrustStore::new();
    alice_store.add_trusted(
        "KD9XYZ",
        ed25519_dalek::SigningKey::from_bytes(&make_seed(4))
            .verifying_key()
            .to_bytes(),
    );

    verify_conack(
        &ack,
        &req.session_id,
        &alice_store,
        PolicyProfile::Balanced,
        SigningMode::Normal,
    )
    .expect("Alice should accept Bob's Lz4-enabled CONACK");
}

#[test]
fn compression_field_tampering_invalidates_signature() {
    let mut req = ConReq::create(
        "W1AW",
        &make_seed(5),
        vec![SigningMode::Normal],
        "sess-comp-4",
        vec![],
    )
    .unwrap();

    // Attacker injects Lz4 after the fact — signature should fail.
    req.supported_compression = vec![CompressionAlgorithm::Lz4];

    let store = InMemoryTrustStore::new();
    assert!(
        verify_conreq(&req, &store, PolicyProfile::Balanced, SigningMode::Normal).is_err(),
        "tampering with compression field must invalidate signature"
    );
}
