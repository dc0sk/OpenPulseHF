//! Compression helpers for B2F proposals.
//!
//! Type D: Gzip (via `flate2`).
//! Type C: LZHUF — stub only; pure-Rust implementation pending.

use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use std::io::{Read, Write};

use crate::B2fError;

/// Compress `data` with Gzip (proposal type D).
pub fn compress_gzip(data: &[u8]) -> Vec<u8> {
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    enc.write_all(data).expect("gzip write");
    enc.finish().expect("gzip finish")
}

/// Decompress Gzip bytes.
pub fn decompress_gzip(data: &[u8]) -> Result<Vec<u8>, B2fError> {
    let mut dec = GzDecoder::new(data);
    let mut out = Vec::new();
    dec.read_to_end(&mut out)
        .map_err(|e| B2fError::Compression(e.to_string()))?;
    Ok(out)
}

/// Compress `data` with LZHUF (proposal type C).
///
/// Currently a pass-through stub; returns the original data unchanged.
/// A future implementation will integrate a pure-Rust LZHUF codec.
pub fn compress_lzhuf(data: &[u8]) -> Vec<u8> {
    data.to_vec()
}

/// Decompress LZHUF bytes.
///
/// Currently a pass-through stub.
pub fn decompress_lzhuf(data: &[u8]) -> Result<Vec<u8>, B2fError> {
    Ok(data.to_vec())
}
