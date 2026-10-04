use serde::{Deserialize, Serialize};

/// Hard ceiling on decompressed output size (matches SAR max segment: 255 × 251 bytes).
pub const MAX_DECOMPRESSED_SIZE: usize = 64_005;

/// Pre-trained zstd dictionary for HPX/Winlink message payloads.
const HPX_DICT_BYTES: &[u8] = include_bytes!("../assets/zstd-hpx-dict.bin");

/// Dictionary ID embedded at bytes 4–7 (LE) of the zstd dictionary file.
pub const ZSTD_DICT_ID: u32 = u32::from_le_bytes([
    HPX_DICT_BYTES[4],
    HPX_DICT_BYTES[5],
    HPX_DICT_BYTES[6],
    HPX_DICT_BYTES[7],
]);

/// Compression algorithm negotiated at session setup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompressionAlgorithm {
    /// No compression; payload transmitted as-is.
    #[default]
    None,
    /// LZ4 block format with a 4-byte little-endian decompressed size prefix.
    Lz4,
    /// Zstd with the shared HPX dictionary; u32 is the dict ID to catch version skew.
    Zstd(u32),
}

/// Errors returned by decompression routines.
#[derive(Debug, thiserror::Error)]
pub enum CompressionError {
    #[error("decompression failed: {0}")]
    DecompressFailed(String),
    #[error("claimed decompressed size {claimed} exceeds limit {limit}")]
    DecompressedSizeTooLarge { claimed: usize, limit: usize },
}

/// Compress `data` with `algo`. `None` returns the data unchanged.
pub fn compress(data: &[u8], algo: CompressionAlgorithm) -> Vec<u8> {
    match algo {
        CompressionAlgorithm::None => data.to_vec(),
        CompressionAlgorithm::Lz4 => lz4_flex::compress_prepend_size(data),
        CompressionAlgorithm::Zstd(_) => zstd_compress(data),
    }
}

/// Decompress `data` with `algo`. `None` returns the data unchanged.
///
/// Rejects input whose size-prefix claims a decompressed size above
/// [`MAX_DECOMPRESSED_SIZE`] before allocating, preventing OOM on malicious input.
pub fn decompress(data: &[u8], algo: CompressionAlgorithm) -> Result<Vec<u8>, CompressionError> {
    match algo {
        CompressionAlgorithm::None => Ok(data.to_vec()),
        CompressionAlgorithm::Lz4 => {
            if data.len() < 4 {
                return Err(CompressionError::DecompressFailed(
                    "input too short for size prefix".to_string(),
                ));
            }
            let claimed = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
            if claimed > MAX_DECOMPRESSED_SIZE {
                return Err(CompressionError::DecompressedSizeTooLarge {
                    claimed,
                    limit: MAX_DECOMPRESSED_SIZE,
                });
            }
            lz4_flex::decompress_size_prepended(data)
                .map_err(|e| CompressionError::DecompressFailed(e.to_string()))
        }
        CompressionAlgorithm::Zstd(_) => {
            if data.len() < 4 {
                return Err(CompressionError::DecompressFailed(
                    "input too short for size prefix".to_string(),
                ));
            }
            let claimed = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
            if claimed > MAX_DECOMPRESSED_SIZE {
                return Err(CompressionError::DecompressedSizeTooLarge {
                    claimed,
                    limit: MAX_DECOMPRESSED_SIZE,
                });
            }
            zstd::bulk::Decompressor::with_dictionary(HPX_DICT_BYTES)
                .and_then(|mut d| d.decompress(&data[4..], claimed))
                .map_err(|e| CompressionError::DecompressFailed(e.to_string()))
        }
    }
}

/// Compress with the best algorithm and return the result only if it is smaller than `data`.
///
/// Tries Lz4 and Zstd; picks whichever produces the smaller output.
/// Returns `(payload, algorithm)`. If neither reduces size the original bytes are returned
/// unchanged with `CompressionAlgorithm::None`.
pub fn compress_if_smaller(data: &[u8]) -> (Vec<u8>, CompressionAlgorithm) {
    let lz4 = lz4_flex::compress_prepend_size(data);
    let zstd = zstd_compress(data);

    let (best_bytes, best_algo) = if lz4.len() <= zstd.len() {
        (lz4, CompressionAlgorithm::Lz4)
    } else {
        (zstd, CompressionAlgorithm::Zstd(ZSTD_DICT_ID))
    };

    if best_bytes.len() < data.len() {
        (best_bytes, best_algo)
    } else {
        (data.to_vec(), CompressionAlgorithm::None)
    }
}

/// Magic prefix of a self-describing compressed session frame (["OP"]en[P]ulse [Z]ip v1).
pub const PACK_MAGIC: [u8; 4] = *b"OPZ1";

/// Wrap `data` as a self-describing session frame: `PACK_MAGIC(4) | algo_tag(1) | payload`.
///
/// Picks the best of Lz4/Zstd via [`compress_if_smaller`] and records which one in the tag, so the
/// receiver needs no out-of-band negotiation. When nothing beats the raw size the tag is `None` and the
/// payload is the original bytes (the 5-byte header is the only overhead). The magic lets [`try_unpack`]
/// distinguish a packed frame from any other traffic (control frames, un-packed data) and pass those
/// through untouched — so enabling compression on one end never corrupts frames from the other.
pub fn pack(data: &[u8]) -> Vec<u8> {
    let (payload, algo) = compress_if_smaller(data);
    let tag: u8 = match algo {
        CompressionAlgorithm::None => 0,
        CompressionAlgorithm::Lz4 => 1,
        CompressionAlgorithm::Zstd(_) => 2,
    };
    let mut out = Vec::with_capacity(PACK_MAGIC.len() + 1 + payload.len());
    out.extend_from_slice(&PACK_MAGIC);
    out.push(tag);
    out.extend_from_slice(&payload);
    out
}

/// Why a frame carrying [`PACK_MAGIC`] could not be unpacked.
#[derive(Debug, thiserror::Error)]
pub enum UnpackError {
    #[error("packed frame has no algorithm tag")]
    MissingTag,
    #[error("packed frame has unknown algorithm tag {0}")]
    UnknownTag(u8),
    #[error(transparent)]
    Decompress(#[from] CompressionError),
}

/// Recover the original bytes from a [`pack`]ed frame, telling a frame that is not packed
/// (`Ok(None)`) from a packed frame that is corrupt (`Err`).
///
/// A receiver must not deliver the second kind: its bytes are still compressed, and passing them on
/// as the message is how a dictionary mismatch (zstd checks the dictionary ID carried in its own frame
/// header) used to surface as silent garbage instead of an error (REQ-CMP-05).
pub fn try_unpack(framed: &[u8]) -> Result<Option<Vec<u8>>, UnpackError> {
    if framed.len() < PACK_MAGIC.len() || framed[..PACK_MAGIC.len()] != PACK_MAGIC {
        return Ok(None);
    }
    let tag = *framed
        .get(PACK_MAGIC.len())
        .ok_or(UnpackError::MissingTag)?;
    let algo = match tag {
        0 => CompressionAlgorithm::None,
        1 => CompressionAlgorithm::Lz4,
        2 => CompressionAlgorithm::Zstd(ZSTD_DICT_ID),
        other => return Err(UnpackError::UnknownTag(other)),
    };
    Ok(Some(decompress(&framed[PACK_MAGIC.len() + 1..], algo)?))
}

/// Compress `data` with zstd + the embedded HPX dictionary.
///
/// Wire format: 4-byte big-endian original length, then the zstd frame.
fn zstd_compress(data: &[u8]) -> Vec<u8> {
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    match zstd::bulk::Compressor::with_dictionary(3, HPX_DICT_BYTES) {
        Ok(mut c) => match c.compress(data) {
            Ok(compressed) => {
                out.extend(compressed);
                out
            }
            Err(_) => {
                let mut fallback = u32::MAX.to_be_bytes().to_vec();
                fallback.extend_from_slice(data);
                fallback
            }
        },
        Err(_) => {
            let mut fallback = u32::MAX.to_be_bytes().to_vec();
            fallback.extend_from_slice(data);
            fallback
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_unpack_roundtrips_compressible_data() {
        let data = vec![0x5Au8; 4096]; // highly compressible
        let framed = pack(&data);
        assert!(framed.len() < data.len(), "packed frame should be smaller");
        assert_eq!(&framed[..4], &PACK_MAGIC);
        assert_ne!(
            framed[4], 0,
            "compressible data should not use the None tag"
        );
        assert_eq!(try_unpack(&framed).unwrap(), Some(data));
    }

    #[test]
    fn pack_unpack_roundtrips_incompressible_data() {
        // Random-ish, incompressible → None tag, payload is the original bytes (+5-byte header).
        let data: Vec<u8> = (0..97u16)
            .map(|i| (i.wrapping_mul(37) ^ 0xA3) as u8)
            .collect();
        let framed = pack(&data);
        assert_eq!(framed[4], 0, "incompressible data should use the None tag");
        assert_eq!(try_unpack(&framed).unwrap(), Some(data));
    }

    #[test]
    fn unpack_passes_through_non_packed_frames() {
        // Control-frame magics and plain text must not be mistaken for packed frames.
        assert!(matches!(
            try_unpack(b"OPHF\x01binary relay envelope"),
            Ok(None)
        ));
        assert!(matches!(try_unpack(b"HSCQ handshake conreq"), Ok(None)));
        assert!(matches!(try_unpack(b"QSY REQ token"), Ok(None)));
        assert!(matches!(try_unpack(b"plain user message body"), Ok(None)));
        assert!(matches!(try_unpack(b""), Ok(None)));
        assert!(matches!(try_unpack(b"OPZ"), Ok(None))); // too short to be a frame
    }

    #[test]
    fn try_unpack_tells_not_packed_from_corrupt() {
        assert!(matches!(try_unpack(b"plain user message body"), Ok(None)));
        assert!(matches!(try_unpack(b"OPZ"), Ok(None)));
        assert!(matches!(try_unpack(b"OPZ1"), Err(UnpackError::MissingTag)));
        assert!(matches!(
            try_unpack(b"OPZ1\x09garbage"),
            Err(UnpackError::UnknownTag(9))
        ));
        assert!(matches!(
            try_unpack(b"OPZ1\x01\x00\x00"),
            Err(UnpackError::Decompress(_))
        ));
        let data = vec![0x5Au8; 4096];
        assert_eq!(try_unpack(&pack(&data)).unwrap(), Some(data));
    }

    /// A frame compressed against a different dictionary is refused with zstd's own reason, not
    /// passed through: zstd carries the dictionary ID in its frame header and checks it on decode.
    #[test]
    fn a_frame_from_another_dictionary_is_an_error() {
        let msg = b"From: N0CALL\r\nTo: DL1ABC\r\nSubject: t\r\n\r\nbody text text text".repeat(4);
        // A stand-in for a retrained dictionary: same content, different dictionary ID.
        let mut other_dict = HPX_DICT_BYTES.to_vec();
        other_dict[4] ^= 0x01;
        let z = zstd::bulk::Compressor::with_dictionary(3, &other_dict)
            .and_then(|mut c| c.compress(&msg))
            .expect("compress with the other dictionary");
        let mut framed = PACK_MAGIC.to_vec();
        framed.push(2);
        framed.extend_from_slice(&(msg.len() as u32).to_be_bytes());
        framed.extend_from_slice(&z);
        match try_unpack(&framed) {
            Err(UnpackError::Decompress(CompressionError::DecompressFailed(reason))) => {
                assert!(reason.contains("Dictionary mismatch"), "reason: {reason}")
            }
            other => panic!("expected a dictionary-mismatch error, got {other:?}"),
        }
    }

    #[test]
    fn none_roundtrip() {
        let data = b"hello world";
        assert_eq!(
            decompress(
                &compress(data, CompressionAlgorithm::None),
                CompressionAlgorithm::None
            )
            .unwrap(),
            data
        );
    }

    #[test]
    fn lz4_roundtrip() {
        let data = b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let compressed = compress(data, CompressionAlgorithm::Lz4);
        assert!(
            compressed.len() < data.len(),
            "repetitive data should compress"
        );
        assert_eq!(
            decompress(&compressed, CompressionAlgorithm::Lz4).unwrap(),
            data
        );
    }

    #[test]
    fn zstd_roundtrip() {
        let data = b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let compressed = compress(data, CompressionAlgorithm::Zstd(ZSTD_DICT_ID));
        assert_eq!(
            decompress(&compressed, CompressionAlgorithm::Zstd(ZSTD_DICT_ID)).unwrap(),
            data
        );
    }

    #[test]
    fn compress_if_smaller_picks_compression_for_repetitive_data() {
        let data = vec![0u8; 256];
        let (out, algo) = compress_if_smaller(&data);
        assert_ne!(algo, CompressionAlgorithm::None, "should compress zeros");
        assert!(out.len() < data.len());
    }

    #[test]
    fn compress_if_smaller_keeps_original_for_random_data() {
        // Already-compressed or random data should not be re-compressed.
        let data: Vec<u8> = (0u8..=255).collect();
        let (out, algo) = compress_if_smaller(&data);
        assert_eq!(algo, CompressionAlgorithm::None);
        assert_eq!(out, data);
    }

    #[test]
    fn decompression_failure_returns_error() {
        let garbage = vec![0xFFu8; 32];
        assert!(decompress(&garbage, CompressionAlgorithm::Lz4).is_err());
    }

    #[test]
    fn zstd_dict_id_const_matches_embedded_dict() {
        let id_from_bytes = u32::from_le_bytes([
            HPX_DICT_BYTES[4],
            HPX_DICT_BYTES[5],
            HPX_DICT_BYTES[6],
            HPX_DICT_BYTES[7],
        ]);
        assert_eq!(ZSTD_DICT_ID, id_from_bytes);
    }
}
