//! Compression helpers for B2F proposals.
//!
//! Type D: Gzip (via `flate2`).
//! Type C: LZHUF LH5 (via `oxiarc-lzhuf`).
//!
//! Two LZHUF API tiers:
//! - `compress_lzhuf` / `decompress_lzhuf`: internal format with a 4-byte BE
//!   original-length prefix.  Self-contained but not wire-compatible with
//!   external Winlink software (RMS Express, RMS Gateway).
//! - `compress_lzhuf_winlink` / `decompress_lzhuf_winlink`: 4-byte LE
//!   original-length prefix followed by raw LH5 bytes.  Wire-compatible with
//!   Winlink Type C implementations (RMS Express, RMS Gateway), which use LE
//!   byte order for the length header.

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

/// Maximum uncompressed payload accepted by `decompress_lzhuf` (16 MiB).
const LZHUF_MAX_UNCOMPRESSED: u32 = 16 * 1024 * 1024;

/// Compress `data` with LZHUF LH5 (proposal type C).
///
/// Output layout: `[4-byte BE original length][LH5 compressed bytes]`.
pub fn compress_lzhuf(data: &[u8]) -> Result<Vec<u8>, B2fError> {
    let raw_len: u32 = data
        .len()
        .try_into()
        .map_err(|_| B2fError::Compression("payload exceeds u32::MAX bytes".into()))?;
    let encoded =
        encode_lzh(data, LzhMethod::Lh5).map_err(|e| B2fError::Compression(e.to_string()))?;
    let orig_len = raw_len.to_be_bytes();
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
    let orig_len = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    if orig_len > LZHUF_MAX_UNCOMPRESSED {
        return Err(B2fError::Compression(format!(
            "claimed uncompressed length {orig_len} exceeds limit"
        )));
    }
    decode_lzh(&data[4..], LzhMethod::Lh5, orig_len as u64)
        .map_err(|e| B2fError::Compression(e.to_string()))
}

/// Compress `data` with LZHUF LH5, Winlink-compatible (4-byte LE length prefix).
///
/// Output layout: `[4-byte LE original length][LH5 compressed bytes]`.
/// Use this for Type C proposals exchanged with external Winlink gateways
/// (RMS Express, RMS Gateway), which prepend the uncompressed size as LE.
pub fn compress_lzhuf_winlink(data: &[u8]) -> Result<Vec<u8>, B2fError> {
    let raw_len: u32 = data
        .len()
        .try_into()
        .map_err(|_| B2fError::Compression("payload exceeds u32::MAX bytes".into()))?;
    let encoded =
        encode_lzh(data, LzhMethod::Lh5).map_err(|e| B2fError::Compression(e.to_string()))?;
    let mut out = Vec::with_capacity(4 + encoded.len());
    out.extend_from_slice(&raw_len.to_le_bytes());
    out.extend_from_slice(&encoded);
    Ok(out)
}

/// Decompress Winlink-compatible LZHUF LH5 bytes (4-byte LE length prefix).
///
/// Expects the 4-byte LE uncompressed-size header used by Winlink gateways.
/// Output is capped at `LZHUF_MAX_UNCOMPRESSED` bytes to prevent OOM from
/// malformed streams.
pub fn decompress_lzhuf_winlink(data: &[u8]) -> Result<Vec<u8>, B2fError> {
    if data.len() < 4 {
        return Err(B2fError::Compression("truncated Winlink LZHUF data".into()));
    }
    let orig_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    if orig_len > LZHUF_MAX_UNCOMPRESSED {
        return Err(B2fError::Compression(format!(
            "claimed uncompressed length {orig_len} exceeds limit"
        )));
    }
    decode_lzh(&data[4..], LzhMethod::Lh5, orig_len as u64)
        .map_err(|e| B2fError::Compression(e.to_string()))
}
