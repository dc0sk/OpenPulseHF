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
