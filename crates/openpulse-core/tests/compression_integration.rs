use openpulse_core::compression::{
    compress, compress_if_smaller, decompress, CompressionAlgorithm, MAX_DECOMPRESSED_SIZE,
    ZSTD_DICT_ID,
};

// ------------------------------------------------------------------
// Group 1: Codec correctness
// ------------------------------------------------------------------

// ---------------------------------------------------------------------------
// #1166 / #1147: the handshake NEGOTIATION tests that lived here are GONE.
//
// `supported_compression` / `selected_compression` were deleted from the wire format because
// nothing consumed the selection — the daemon sent the list empty and hardcoded `None`, so the
// field was a capability claim the station could not back (the same reason Type C was removed in
// PR #948).
//
// Note precisely what that does to coverage: these tests are not "temporarily unported". The
// property they asserted — that a responder cannot select an algorithm, or a Zstd dict id, the
// initiator never offered — DISSOLVES with the field, because there is no selection to police.
// The codec tests below are unaffected and still cover compress/decompress including the Zstd
// dictionary path.
//
// IF COMPRESSION IS EVER WIRED FOR REAL, the membership check must come back WITH it: the deleted
// tests were `conreq_carries_supported_compression_in_signature`, `conack_carries_selected_compression_in_signature`, `full_negotiation_round_trip_with_lz4`, `compression_field_tampering_invalidates_signature`, `conack_rejected_when_compression_not_offered`, `zstd_dict_id_mismatch_rejected_in_negotiation`, `zstd_full_negotiation_round_trip`.
// ---------------------------------------------------------------------------

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

// VERIFIES: REQ-CMP-01

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
