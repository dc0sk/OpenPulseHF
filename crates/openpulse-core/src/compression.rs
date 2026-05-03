use serde::{Deserialize, Serialize};

/// Hard ceiling on decompressed output size (matches SAR max segment: 255 × 251 bytes).
pub const MAX_DECOMPRESSED_SIZE: usize = 64_005;

/// Compression algorithm negotiated at session setup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompressionAlgorithm {
    /// No compression; payload transmitted as-is.
    #[default]
    None,
    /// LZ4 block format with a 4-byte little-endian decompressed size prefix.
    Lz4,
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
    }
}

/// Decompress `data` with `algo`. `None` returns the data unchanged.
///
/// Rejects LZ4 input whose size-prefix claims a decompressed size above
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
    }
}

/// Compress with Lz4 and return the result only if it is smaller than `data`.
///
/// Returns `(payload, algorithm)`. If compression does not reduce size the
/// original bytes are returned unchanged with `CompressionAlgorithm::None`.
pub fn compress_if_smaller(data: &[u8]) -> (Vec<u8>, CompressionAlgorithm) {
    let compressed = lz4_flex::compress_prepend_size(data);
    if compressed.len() < data.len() {
        (compressed, CompressionAlgorithm::Lz4)
    } else {
        (data.to_vec(), CompressionAlgorithm::None)
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
    fn compress_if_smaller_picks_lz4_for_repetitive_data() {
        let data = vec![0u8; 256];
        let (out, algo) = compress_if_smaller(&data);
        assert_eq!(algo, CompressionAlgorithm::Lz4);
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
}
