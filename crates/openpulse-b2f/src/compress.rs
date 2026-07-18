//! Compression helpers for B2F proposals.
//!
//! Type D: Gzip (via `flate2`) — the only proposal type this crate produces or consumes.
//!
//! **Type C (LZHUF) is not supported.** An LH5 implementation lived here and was removed: it was
//! never wired to a production caller, and its external-Winlink compatibility was never verified —
//! FBB historically uses the classic Okumura LZHUF, a *different bitstream* from LHA `LH5`, so the
//! two would not have interoperated. Rather than keep unverified code that claimed compatibility it
//! did not have, an inbound Type C proposal is now answered `Reject` (see `session.rs`), which is an
//! honest "cannot decode this" instead of a silent corrupt decode. Restoring Type C requires a real
//! captured RMS Express / RMS Gateway Type C blob to validate both the bitstream and the
//! length-prefix convention against.

use flate2::{read::GzDecoder, write::GzEncoder, Compression};
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

/// Decompress Gzip bytes, bounding the output at [`MAX_UNCOMPRESSED`].
///
/// Gzip carries no trustworthy up-front length to check (unlike the LZHUF prefix), so the decoder is
/// wrapped in a `Take` that stops one byte past the cap; anything larger is a decompression bomb and
/// is rejected rather than allocated (audit B-1 — Type D is the format a real CMS actually sends).
pub fn decompress_gzip(data: &[u8]) -> Result<Vec<u8>, B2fError> {
    let dec = GzDecoder::new(data);
    let mut out = Vec::new();
    dec.take(u64::from(MAX_UNCOMPRESSED) + 1)
        .read_to_end(&mut out)
        .map_err(|e| B2fError::Compression(e.to_string()))?;
    if out.len() as u64 > u64::from(MAX_UNCOMPRESSED) {
        return Err(B2fError::Compression(format!(
            "decompressed gzip exceeds limit ({MAX_UNCOMPRESSED} bytes)"
        )));
    }
    Ok(out)
}

/// Maximum uncompressed payload accepted by the decompression helpers (16 MiB).
const MAX_UNCOMPRESSED: u32 = 16 * 1024 * 1024;
