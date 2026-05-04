//! WL2K connection banner encode/decode.
//!
//! Format: `[WL2K-3.0-B2FWINMOR-4.0-<SESSION_KEY>]`

use crate::B2fError;

/// Decoded connection banner.
#[derive(Debug, Clone, PartialEq)]
pub struct Banner {
    pub version: String,
    pub session_key: String,
}

/// Encode a banner line for the given callsign.
///
/// Derives a deterministic (but opaque) session key from the callsign.
pub fn encode(callsign: &str) -> String {
    let key = session_key(callsign);
    format!("[WL2K-3.0-B2FWINMOR-4.0-{key}]")
}

/// Decode a banner line.
pub fn decode(line: &str) -> Result<Banner, B2fError> {
    let trimmed = line.trim();
    if !trimmed.starts_with('[') || !trimmed.ends_with(']') {
        return Err(B2fError::InvalidBanner(trimmed.to_string()));
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    let parts: Vec<&str> = inner.splitn(5, '-').collect();
    if parts.len() < 5 || parts[0] != "WL2K" {
        return Err(B2fError::InvalidBanner(trimmed.to_string()));
    }
    Ok(Banner {
        version: format!("{}-{}", parts[1], parts[2]),
        session_key: parts[4].to_string(),
    })
}

fn session_key(callsign: &str) -> String {
    // Simple FNV-1a hash of the callsign bytes → 8 uppercase hex chars.
    let mut hash: u32 = 2_166_136_261;
    for b in callsign.as_bytes() {
        hash ^= *b as u32;
        hash = hash.wrapping_mul(16_777_619);
    }
    format!("{hash:08X}")
}
