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

    validate_trust_store_permissions(path)?;

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
        tracing::warn!(
            skipped,
            "trust_store_file: record(s) skipped (non-hex or wrong-length key_id)"
        );
    }
    Ok(store)
}

#[cfg(unix)]
fn validate_trust_store_permissions(
    path: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use std::os::unix::fs::PermissionsExt;

    let mode = std::fs::metadata(path)
        .map_err(|e| format!("stat {}: {e}", path.display()))?
        .permissions()
        .mode()
        & 0o777;
    if mode & 0o077 != 0 {
        return Err(format!(
            "unsafe trust store permissions on {}: {:o} (expected owner-only, e.g. 600)",
            path.display(),
            mode
        )
        .into());
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_trust_store_permissions(
    _path: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn rejects_insecure_permissions() {
        use std::os::unix::fs::PermissionsExt;
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "openpulse-core-trust-store-unsafe-perms-{}-{}",
            std::process::id(),
            nonce
        ));
        let path = root.join("trust-store.json");

        std::fs::create_dir_all(&root).expect("create temp root");
        std::fs::write(&path, r#"{"schema_version":"1.0.0","records":[]}"#)
            .expect("write trust store");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .expect("set insecure mode");

        let err = load_trust_store_from_file(&path).expect_err("should reject unsafe mode");
        assert!(
            err.to_string().contains("unsafe trust store permissions"),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(root);
    }
}
