//! Compression helpers for B2F proposals.
//!
//! Type D: Gzip (via `flate2`).
//! Type C: LZHUF LH5 (via `oxiarc-lzhuf`).
//!
//! **Note:** `compress_lzhuf` / `decompress_lzhuf` prepend a 4-byte big-endian
//! uncompressed-length header before the LH5 stream.  This makes the format
//! self-contained but incompatible with Type C messages produced by external
//! Winlink gateways (RMS Express, etc.).  Full Winlink compatibility is deferred.

use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use oxiarc_lzhuf::{decode_lzh, encode_lzh, LzhMethod};
use std::io::{Read, Write};

use crate::B2fError;

/// Compress `data` with Gzip (proposal type D).
pub fn compress_gzip(data: &[u8]) -> Result<Vec<u8>, B2fError> {
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    enc.write_all(data)
        .map_err(|e| B2fError::Compression(e.to_string()))?;
    enc.finish()
        .map_err(|e| B2fError::Compression(e.to_string()))
}

/// Decompress Gzip bytes.
pub fn decompress_gzip(data: &[u8]) -> Result<Vec<u8>, B2fError> {
    let mut dec = GzDecoder::new(data);
    let mut out = Vec::new();
    dec.read_to_end(&mut out)
        .map_err(|e| B2fError::Compression(e.to_string()))?;
    Ok(out)
}

/// Compress `data` with LZHUF LH5 (proposal type C).
///
/// Output layout: `[4-byte BE original length][LH5 compressed bytes]`.
pub fn compress_lzhuf(data: &[u8]) -> Result<Vec<u8>, B2fError> {
    let encoded =
        encode_lzh(data, LzhMethod::Lh5).map_err(|e| B2fError::Compression(e.to_string()))?;
    let orig_len = (data.len() as u32).to_be_bytes();
    let mut out = Vec::with_capacity(4 + encoded.len());
    out.extend_from_slice(&orig_len);
    out.extend_from_slice(&encoded);
    Ok(out)
}

/// Decompress LZHUF LH5 bytes (proposal type C).
///
/// Expects the 4-byte original-length prefix written by `compress_lzhuf`.
pub fn decompress_lzhuf(data: &[u8]) -> Result<Vec<u8>, B2fError> {
    if data.len() < 4 {
        return Err(B2fError::Compression("truncated LZHUF data".into()));
    }
    let orig_len = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as u64;
    decode_lzh(&data[4..], LzhMethod::Lh5, orig_len)
        .map_err(|e| B2fError::Compression(e.to_string()))
}
