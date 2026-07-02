//! Compression helpers for B2F proposals.
//!
//! Type D: Gzip (via `flate2`).
//! Type C: LZHUF LH5 (via `oxiarc-lzhuf`).
//!
//! LZHUF API helpers:
//! - `compress_lzhuf` / `decompress_lzhuf`: internal format with a 4-byte BE
//!   original-length prefix.  Self-contained but not wire-compatible with
//!   external Winlink software (RMS Express, RMS Gateway).
//! - `compress_lzhuf_winlink` / `decompress_lzhuf_winlink`: 4-byte LE
//!   original-length prefix followed by raw LH5 bytes — the classic Okumura
//!   `LZHUF.C` length-header convention.
//! - `decompress_lzhuf_compat`: decode helper that accepts either prefix format
//!   (chooses a plausible first attempt, then falls back) for mixed-version
//!   interoperability.
//!
//! **External Winlink Type C compatibility is UNVERIFIED.** These round-trip
//! cleanly between two OpenPulseHF stations, but interop with real RMS
//! Express / RMS Gateway has never been tested against a captured Winlink Type C
//! blob, and two things are uncertain: (1) the length-prefix convention, and
//! (2) the LZHUF variant — this uses LHA `LH5` (via `oxiarc-lzhuf`), whereas FBB
//! historically used the classic Okumura LZHUF, a *different* bitstream. Closing
//! the gap requires a real Winlink Type C test vector to confirm both. Not a
//! production risk today: the CMS gateway sends Type D (Gzip), not Type C.

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

/// Decompress Type C payloads accepting Winlink LE and legacy BE headers.
///
/// Uses prefix plausibility to pick a first attempt, then retries with the
/// other format if decode fails, preserving compatibility with older
/// OpenPulse peers and Winlink gateways.
pub fn decompress_lzhuf_compat(data: &[u8]) -> Result<Vec<u8>, B2fError> {
    if data.len() < 4 {
        return Err(B2fError::Compression("truncated LZHUF data".into()));
    }

    let le_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let be_len = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    let le_in_range = le_len <= LZHUF_MAX_UNCOMPRESSED;
    let be_in_range = be_len <= LZHUF_MAX_UNCOMPRESSED;

    let try_le_first = match (le_in_range, be_in_range) {
        (true, false) => true,
        (false, true) => false,
        (true, true) => le_len <= be_len,
        // Both out of range: keep LE first for deterministic error ordering.
        (false, false) => true,
    };

    if try_le_first {
        match decompress_lzhuf_winlink(data) {
            Ok(v) => Ok(v),
            Err(le_err) => match decompress_lzhuf(data) {
                Ok(v) => Ok(v),
                Err(be_err) => Err(B2fError::Compression(format!(
                    "type-c decode failed for LE ({le_err}) and BE ({be_err})"
                ))),
            },
        }
    } else {
        match decompress_lzhuf(data) {
            Ok(v) => Ok(v),
            Err(be_err) => match decompress_lzhuf_winlink(data) {
                Ok(v) => Ok(v),
                Err(le_err) => Err(B2fError::Compression(format!(
                    "type-c decode failed for BE ({be_err}) and LE ({le_err})"
                ))),
            },
        }
    }
}
