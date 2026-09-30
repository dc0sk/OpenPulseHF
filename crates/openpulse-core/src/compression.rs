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
    #[error("decompressed {got} bytes where the size prefix claimed {claimed}")]
    LengthMismatch { claimed: usize, got: usize },
    #[error("zstd dictionary id {got:#010x} is not this build's {expected:#010x}")]
    DictIdMismatch { got: u32, expected: u32 },
    #[error("packed frame is too short for its header")]
    Truncated,
    #[error("unknown compression tag {0}")]
    UnknownTag(u8),
    #[error("retired compression tag {0}")]
    RetiredTag(u8),
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
        CompressionAlgorithm::Zstd(id) => {
            if id != ZSTD_DICT_ID {
                return Err(CompressionError::DictIdMismatch {
                    got: id,
                    expected: ZSTD_DICT_ID,
                });
            }
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
            let out = zstd::bulk::Decompressor::with_dictionary(HPX_DICT_BYTES)
                .and_then(|mut d| d.decompress(&data[4..], claimed))
                .map_err(|e| CompressionError::DecompressFailed(e.to_string()))?;
            if out.len() != claimed {
                return Err(CompressionError::LengthMismatch {
                    claimed,
                    got: out.len(),
                });
            }
            Ok(out)
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

/// Container tag: payload is the original bytes.
pub const TAG_NONE: u8 = 0;
/// Container tag: payload is an LZ4 block with a 4-byte LE size prefix.
pub const TAG_LZ4: u8 = 1;
/// Retired container tag: zstd with no dictionary id. Refused, never reused.
pub const TAG_ZSTD_RETIRED: u8 = 2;
/// Container tag: `dict_id (LE u32) | BE u32 size | zstd frame`.
pub const TAG_ZSTD_DICT: u8 = 3;

const HEADER_LEN: usize = PACK_MAGIC.len() + 1;

/// Wrap `data` as a self-describing session frame: `PACK_MAGIC(4) | tag(1) | payload`.
///
/// Picks the smaller of Lz4/Zstd (counting the zstd arm's 4-byte dictionary id) and records it in
/// the tag, so the receiver needs no negotiation. When nothing beats the raw size the tag is
/// [`TAG_NONE`] and the payload is the original bytes (the 5-byte header is the only overhead).
pub fn pack(data: &[u8]) -> Vec<u8> {
    let lz4 = lz4_flex::compress_prepend_size(data);
    let zstd = zstd_compress(data);
    let zstd_cost = zstd.len() + 4;
    let mut out = Vec::with_capacity(HEADER_LEN + data.len());
    out.extend_from_slice(&PACK_MAGIC);
    if lz4.len() < data.len() && lz4.len() <= zstd_cost {
        out.push(TAG_LZ4);
        out.extend_from_slice(&lz4);
    } else if zstd_cost < data.len() {
        out.push(TAG_ZSTD_DICT);
        out.extend_from_slice(&ZSTD_DICT_ID.to_le_bytes());
        out.extend_from_slice(&zstd);
    } else {
        out.push(TAG_NONE);
        out.extend_from_slice(data);
    }
    out
}

/// The bytes to transmit for `data`: [`pack`]ed when `compress` is set, raw otherwise — except that
/// raw data which itself begins with [`PACK_MAGIC`] is packed anyway, since a receiver would
/// otherwise read it as a corrupt packed frame and refuse it.
pub fn outbound(data: &[u8], compress: bool) -> Vec<u8> {
    if compress || data.starts_with(&PACK_MAGIC) {
        pack(data)
    } else {
        data.to_vec()
    }
}

/// Recover the original bytes from a [`pack`]ed frame.
///
/// `Ok(None)`: no magic — not a packed frame; the caller keeps its bytes. `Ok(Some)`: the original
/// bytes. `Err`: the magic is present but the frame does not decode — a frame-integrity error
/// (REQ-CMP-05), never to be delivered as raw bytes. Never allocates above [`MAX_DECOMPRESSED_SIZE`].
pub fn unpack(framed: &[u8]) -> Result<Option<Vec<u8>>, CompressionError> {
    if !framed.starts_with(&PACK_MAGIC) {
        return Ok(None);
    }
    let (&tag, body) = framed[PACK_MAGIC.len()..]
        .split_first()
        .ok_or(CompressionError::Truncated)?;
    let out = match tag {
        TAG_NONE => body.to_vec(),
        TAG_LZ4 => decompress(body, CompressionAlgorithm::Lz4)?,
        TAG_ZSTD_DICT => {
            let (id, zbody) = body
                .split_at_checked(4)
                .ok_or(CompressionError::Truncated)?;
            let id = u32::from_le_bytes([id[0], id[1], id[2], id[3]]);
            decompress(zbody, CompressionAlgorithm::Zstd(id))?
        }
        TAG_ZSTD_RETIRED => return Err(CompressionError::RetiredTag(tag)),
        _ => return Err(CompressionError::UnknownTag(tag)),
    };
    Ok(Some(out))
}

/// Compress `data` with zstd + the embedded HPX dictionary.
///
/// Wire format: 4-byte big-endian original length, then the zstd frame. The frame omits zstd's own
/// dictionary-id field (the container carries the id and checks it) and spends those 4 bytes on a
/// content checksum, which catches a dictionary whose content changed under the same id.
fn zstd_compress(data: &[u8]) -> Vec<u8> {
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    let compressor =
        zstd::bulk::Compressor::with_dictionary(3, HPX_DICT_BYTES).and_then(|mut c| {
            c.set_parameter(zstd::zstd_safe::CParameter::DictIdFlag(false))?;
            c.set_parameter(zstd::zstd_safe::CParameter::ChecksumFlag(true))?;
            Ok(c)
        });
    match compressor {
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
        assert_eq!(unpack(&framed).unwrap(), Some(data));
    }

    #[test]
    fn pack_unpack_roundtrips_incompressible_data() {
        // Random-ish, incompressible → None tag, payload is the original bytes (+5-byte header).
        let data: Vec<u8> = (0..97u16)
            .map(|i| (i.wrapping_mul(37) ^ 0xA3) as u8)
            .collect();
        let framed = pack(&data);
        assert_eq!(framed[4], 0, "incompressible data should use the None tag");
        assert_eq!(unpack(&framed).unwrap(), Some(data));
    }

    #[test]
    fn unpack_passes_through_non_packed_frames() {
        // Control-frame magics and plain text must not be mistaken for packed frames.
        for raw in [
            &b"OPHF\x01binary relay envelope"[..],
            b"HSCQ handshake conreq",
            b"QSY REQ token",
            b"plain user message body",
            b"",
            b"OPZ", // shorter than the magic
        ] {
            assert!(matches!(unpack(raw), Ok(None)), "{raw:?} must pass through");
        }
    }

    #[test]
    fn unpack_rejects_unknown_tag_and_corrupt_payload() {
        assert!(matches!(
            unpack(b"OPZ1\x09garbage"),
            Err(CompressionError::UnknownTag(9))
        ));
        assert!(unpack(b"OPZ1\x01\x00\x00").is_err()); // Lz4 tag, truncated/garbage payload
        assert!(matches!(unpack(b"OPZ1"), Err(CompressionError::Truncated))); // magic, no tag
    }

    /// Data that zstd+dictionary beats LZ4 on, so `pack` takes the zstd arm.
    fn zstd_favoured() -> Vec<u8> {
        b"From: N0CALL To: W1AW Subject: net report QTH JO31 rig IC-9700 ant dipole wx fine 73"
            .to_vec()
    }

    fn packed_zstd() -> Vec<u8> {
        let framed = pack(&zstd_favoured());
        assert_eq!(framed[4], TAG_ZSTD_DICT, "fixture must take the zstd arm");
        framed
    }

    #[test]
    fn a_zstd_frame_names_its_dictionary_and_the_zstd_header_does_not_repeat_it() {
        let framed = packed_zstd();
        assert_eq!(framed[5..9], ZSTD_DICT_ID.to_le_bytes());
        // Our header is the only place the id lives (the 4 bytes are spent on a checksum instead):
        // FHD bits 0-1 = Dictionary_ID_flag (0 = absent), bit 2 = Content_Checksum_flag.
        let zframe = &framed[9 + 4..];
        assert_eq!(zframe[..4], 0xFD2F_B528u32.to_le_bytes(), "zstd magic");
        assert_eq!(
            zframe[4] & 0b11,
            0,
            "zstd frame still carries its own dict id"
        );
        assert_eq!(
            zframe[4] & 0b100,
            0b100,
            "zstd frame carries no content checksum"
        );
        assert_eq!(unpack(&framed).unwrap(), Some(zstd_favoured()));
    }

    // VERIFIES: REQ-CMP-05
    #[test]
    fn a_dictionary_id_mismatch_is_rejected() {
        let mut framed = packed_zstd();
        let other = ZSTD_DICT_ID.wrapping_add(1);
        framed[5..9].copy_from_slice(&other.to_le_bytes());
        assert!(matches!(
            unpack(&framed),
            Err(CompressionError::DictIdMismatch { got, expected })
                if got == other && expected == ZSTD_DICT_ID
        ));
    }

    #[test]
    fn a_dictionary_with_our_id_but_other_content_is_rejected_by_the_checksum() {
        // The id check cannot see a dictionary whose content changed under the same id; without the
        // content checksum this decodes to garbage `Ok` (measured in review).
        let framed = packed_zstd();
        let mut skewed = HPX_DICT_BYTES.to_vec();
        let half = skewed.len() / 2;
        skewed[half..].iter_mut().for_each(|b| *b ^= 0x5A);
        let claimed = u32::from_be_bytes([framed[9], framed[10], framed[11], framed[12]]) as usize;
        let decoded = zstd::bulk::Decompressor::with_dictionary(&skewed)
            .and_then(|mut d| d.decompress(&framed[13..], claimed));
        assert!(decoded.is_err(), "content skew decoded: {decoded:?}");
    }

    #[test]
    fn the_retired_zstd_tag_is_refused() {
        // Tag 2 carried no dictionary id; a frame in that layout cannot be attributed to a dictionary.
        let mut legacy = PACK_MAGIC.to_vec();
        legacy.push(2);
        legacy.extend_from_slice(&compress(
            &zstd_favoured(),
            CompressionAlgorithm::Zstd(ZSTD_DICT_ID),
        ));
        assert!(matches!(
            unpack(&legacy),
            Err(CompressionError::RetiredTag(2))
        ));
    }

    #[test]
    fn a_zstd_frame_whose_size_prefix_lies_is_rejected() {
        let mut framed = packed_zstd();
        let claimed = u32::from_be_bytes([framed[9], framed[10], framed[11], framed[12]]);
        framed[9..13].copy_from_slice(&(claimed + 1).to_be_bytes());
        assert!(unpack(&framed).is_err());
    }

    #[test]
    fn decompress_refuses_a_foreign_dictionary_id() {
        let c = compress(&zstd_favoured(), CompressionAlgorithm::Zstd(ZSTD_DICT_ID));
        assert!(matches!(
            decompress(&c, CompressionAlgorithm::Zstd(ZSTD_DICT_ID ^ 1)),
            Err(CompressionError::DictIdMismatch { .. })
        ));
    }

    // VERIFIES: REQ-CMP-03
    #[test]
    fn an_outbound_body_is_raw_unless_compressing_or_it_would_read_as_packed() {
        assert_eq!(outbound(b"hello", false), b"hello");
        assert_eq!(
            unpack(&outbound(b"hello", true)).unwrap(),
            Some(b"hello".to_vec())
        );
        let looks_packed = b"OPZ1 hello";
        assert_eq!(
            unpack(&outbound(looks_packed, false)).unwrap(),
            Some(looks_packed.to_vec())
        );
    }

    #[test]
    fn pack_never_makes_a_frame_larger_than_raw_plus_the_header() {
        // REQ-CMP-04 at the container level: the zstd arm's 4-byte id counts against it.
        for data in [zstd_favoured(), vec![0x5A; 4096], (0u8..=255).collect()] {
            assert!(pack(&data).len() <= data.len() + PACK_MAGIC.len() + 1);
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
