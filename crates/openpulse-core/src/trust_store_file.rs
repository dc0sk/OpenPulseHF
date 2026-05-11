//! Persistence helpers for the on-disk trust store used by the TNC binaries.
//!
//! The file is a JSON object matching the schema written by `openpulse-cli`:
//! `{ "schema_version": "1.0.0", "records": [ { "station_id", "key_id", "trust", … } ] }`.

use std::path::Path;

use serde::Deserialize;

use crate::handshake::InMemoryTrustStore;
use crate::trust::PublicKeyTrustLevel;

#[derive(Debug, Deserialize)]
struct TrustFileRecord {
    station_id: String,
    /// Hex-encoded Ed25519 verifying-key bytes (64 hex chars = 32 bytes).
    key_id: String,
    trust: PublicKeyTrustLevel,
}

#[derive(Debug, Deserialize)]
struct TrustFile {
    #[allow(dead_code)]
    schema_version: String,
    records: Vec<TrustFileRecord>,
}

/// Load the trust store from `path` and return a populated [`InMemoryTrustStore`].
///
/// Returns an empty store when the file does not exist.
/// Returns `Err` if the file exists but cannot be read or parsed.
pub fn load_trust_store_from_file(
    path: &Path,
) -> Result<InMemoryTrustStore, Box<dyn std::error::Error + Send + Sync>> {
    if !path.exists() {
        return Ok(InMemoryTrustStore::new());
    }

    let content =
        std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let file: TrustFile =
        serde_json::from_str(&content).map_err(|e| format!("parse {}: {e}", path.display()))?;

    let mut store = InMemoryTrustStore::new();
    let mut skipped = 0usize;
    for rec in &file.records {
        if let Some(key_bytes) = parse_hex_key(&rec.key_id) {
            store.add_entry(&rec.station_id, key_bytes, rec.trust);
        } else {
            skipped += 1;
        }
    }
    if skipped > 0 {
        // Surface skipped records as a recoverable warning through the return value.
        // Callers log it; openpulse-core has no tracing dependency.
        eprintln!("trust_store_file: {skipped} record(s) skipped (non-hex or wrong-length key_id)");
    }
    Ok(store)
}

fn parse_hex_key(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, pair) in s.as_bytes().chunks(2).enumerate() {
        let pair_str = std::str::from_utf8(pair).ok()?;
        out[i] = u8::from_str_radix(pair_str, 16).ok()?;
    }
    Some(out)
}
