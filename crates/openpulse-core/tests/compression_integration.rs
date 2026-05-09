use openpulse_core::compression::{
    compress, compress_if_smaller, decompress, CompressionAlgorithm, MAX_DECOMPRESSED_SIZE,
    ZSTD_DICT_ID,
};
use openpulse_core::fec::FecMode;
use openpulse_core::handshake::{
    verify_conack, verify_conreq, ConAck, ConReq, HandshakeError, InMemoryTrustStore,
};
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
        vec![],
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
        FecMode::None,
    )
    .unwrap();

    assert_eq!(ack.selected_compression, CompressionAlgorithm::Lz4);

    let store = InMemoryTrustStore::new();
    verify_conack(
        &ack,
        "sess-comp-2",
        &[CompressionAlgorithm::Lz4],
        &[],
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
        vec![],
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
        FecMode::None,
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
        &req.supported_compression,
        &[],
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

#[test]
fn decompress_rejects_oversized_size_prefix() {
    // Build a byte stream whose 4-byte LE prefix claims MAX_DECOMPRESSED_SIZE + 1.
    let claimed = (MAX_DECOMPRESSED_SIZE + 1) as u32;
    let mut buf = claimed.to_le_bytes().to_vec();
    buf.extend_from_slice(&[0u8; 16]); // dummy payload bytes

    assert!(
        decompress(&buf, CompressionAlgorithm::Lz4).is_err(),
        "size prefix exceeding limit must be rejected before allocation"
    );
}

#[test]
fn conack_rejected_when_compression_not_offered() {
    // Initiator advertises no compression; responder picks Lz4 anyway.
    let ack = ConAck::create(
        "KD9XYZ",
        &make_seed(6),
        SigningMode::Normal,
        "sess-comp-5",
        CompressionAlgorithm::Lz4,
        FecMode::None,
    )
    .unwrap();

    let store = InMemoryTrustStore::new();
    let result = verify_conack(
        &ack,
        "sess-comp-5",
        &[], // initiator offered nothing
        &[],
        &store,
        PolicyProfile::Balanced,
        SigningMode::Normal,
    );
    assert!(
        matches!(result, Err(HandshakeError::UnsupportedCompression)),
        "selecting an unoffered compression must be rejected"
    );
}

// ------------------------------------------------------------------
// Group 4: Zstd dictionary compression
// ------------------------------------------------------------------

#[test]
fn zstd_round_trip() {
    let payload = b"Date: Thu, 01 May 2026 14:23:00 +0000\r\nFrom: N0CALL@winlink.org\r\nTo: W1AW@winlink.org\r\nSubject: Check-in\r\n\r\nAll OK. Grid FN31.\r\n";
    let algo = CompressionAlgorithm::Zstd(ZSTD_DICT_ID);
    let compressed = compress(payload, algo);
    let recovered = decompress(&compressed, algo).unwrap();
    assert_eq!(recovered.as_slice(), payload.as_slice());
}

#[test]
fn zstd_compresses_structured_payload() {
    // A typical Winlink-style header should shrink with the HPX dictionary.
    let payload = b"Date: Fri, 02 May 2026 09:10:00 +0000\r\nFrom: KD9ABC@winlink.org\r\nTo: WB4GHI@winlink.org\r\nSubject: Weekly traffic net\r\nMime-Version: 1.0\r\nContent-Type: text/plain\r\n\r\nTraffic net check-in. Grid: EM60. No traffic.\r\n";
    let compressed = compress(payload, CompressionAlgorithm::Zstd(ZSTD_DICT_ID));
    assert!(
        compressed.len() < payload.len(),
        "structured payload should compress (compressed={}, original={})",
        compressed.len(),
        payload.len(),
    );
}

#[test]
fn zstd_decompression_oom_guard() {
    // 4-byte BE size prefix that exceeds MAX_DECOMPRESSED_SIZE must be rejected.
    use openpulse_core::compression::CompressionError;
    let oversized = (MAX_DECOMPRESSED_SIZE as u32 + 1).to_be_bytes();
    let garbage: Vec<u8> = oversized.iter().copied().chain([0u8; 8]).collect();
    let result = decompress(&garbage, CompressionAlgorithm::Zstd(ZSTD_DICT_ID));
    assert!(
        matches!(
            result,
            Err(CompressionError::DecompressedSizeTooLarge { .. })
        ),
        "oversized size prefix must be rejected before allocation"
    );
}

#[test]
fn zstd_dict_id_mismatch_rejected_in_negotiation() {
    // Bob selects Zstd with a wrong dict ID — Alice only offered ZSTD_DICT_ID.
    let wrong_id = ZSTD_DICT_ID.wrapping_add(1);
    let req = ConReq::create(
        "W1AW",
        &make_seed(11),
        vec![SigningMode::Normal],
        "sess-zstd-mismatch",
        vec![
            CompressionAlgorithm::Lz4,
            CompressionAlgorithm::Zstd(ZSTD_DICT_ID),
        ],
        vec![],
    )
    .unwrap();

    let ack = ConAck::create(
        "KD9XYZ",
        &make_seed(12),
        SigningMode::Normal,
        "sess-zstd-mismatch",
        CompressionAlgorithm::Zstd(wrong_id),
        FecMode::None,
    )
    .unwrap();

    let store = InMemoryTrustStore::new();
    let result = verify_conack(
        &ack,
        "sess-zstd-mismatch",
        &req.supported_compression,
        &[],
        &store,
        PolicyProfile::Balanced,
        SigningMode::Normal,
    );
    assert!(
        matches!(result, Err(HandshakeError::UnsupportedCompression)),
        "wrong dict ID must be rejected as unsupported compression"
    );
}

#[test]
fn zstd_full_negotiation_round_trip() {
    // Alice advertises [Lz4, Zstd(DICT_ID)]; Bob selects Zstd.
    let req = ConReq::create(
        "W1AW",
        &make_seed(13),
        vec![SigningMode::Normal],
        "sess-zstd-ok",
        vec![
            CompressionAlgorithm::Lz4,
            CompressionAlgorithm::Zstd(ZSTD_DICT_ID),
        ],
        vec![],
    )
    .unwrap();

    let mut bob_store = InMemoryTrustStore::new();
    bob_store.add_trusted(
        "W1AW",
        ed25519_dalek::SigningKey::from_bytes(&make_seed(13))
            .verifying_key()
            .to_bytes(),
    );
    verify_conreq(
        &req,
        &bob_store,
        PolicyProfile::Balanced,
        SigningMode::Normal,
    )
    .unwrap();

    let ack = ConAck::create(
        "KD9XYZ",
        &make_seed(14),
        SigningMode::Normal,
        "sess-zstd-ok",
        CompressionAlgorithm::Zstd(ZSTD_DICT_ID),
        FecMode::None,
    )
    .unwrap();
    assert_eq!(
        ack.selected_compression,
        CompressionAlgorithm::Zstd(ZSTD_DICT_ID)
    );

    let alice_store = InMemoryTrustStore::new();
    verify_conack(
        &ack,
        &req.session_id,
        &req.supported_compression,
        &[],
        &alice_store,
        PolicyProfile::Balanced,
        SigningMode::Normal,
    )
    .expect("Zstd negotiation should succeed");
}
