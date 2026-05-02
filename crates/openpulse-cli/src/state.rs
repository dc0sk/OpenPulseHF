use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

use openpulse_core::trust::{CertificateSource, PolicyProfile, PublicKeyTrustLevel};

// ── Persisted types ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalTrustRecord {
    pub station_id: String,
    pub key_id: String,
    pub trust: PublicKeyTrustLevel,
    pub source: CertificateSource,
    pub status: String,
    pub reason: String,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalTrustStore {
    pub schema_version: String,
    pub records: Vec<LocalTrustRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedSessionState {
    pub session_id: String,
    pub peer: String,
    pub hpx_state: String,
    pub selected_mode: Option<String>,
    pub trust_level: Option<String>,
    pub policy_profile: String,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedSessionLogEntry {
    pub timestamp_ms: u64,
    pub from_state: String,
    pub to_state: String,
    pub event: String,
    pub reason_code: String,
    pub reason_string: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedSessionLog {
    pub schema_version: String,
    pub entries: Vec<PersistedSessionLogEntry>,
}

// ── Path helpers ──────────────────────────────────────────────────────────────

pub fn config_dir_path() -> PathBuf {
    if let Ok(path) = std::env::var("OPENPULSE_CONFIG_DIR") {
        return PathBuf::from(path);
    }
    if let Ok(path) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(path).join("openpulse");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".config").join("openpulse");
    }
    PathBuf::from(".")
}

pub fn policy_file_path() -> PathBuf {
    config_dir_path().join("trust-policy.json")
}

pub fn trust_store_file_path() -> PathBuf {
    config_dir_path().join("trust-store.json")
}

pub fn session_state_file_path() -> PathBuf {
    config_dir_path().join("session-state.json")
}

pub fn session_log_file_path() -> PathBuf {
    config_dir_path().join("session-log.json")
}

// ── Session state persistence ─────────────────────────────────────────────────

pub fn load_session_state() -> Result<Option<PersistedSessionState>> {
    load_session_state_at(&session_state_file_path())
}

pub fn persist_session_state(state: &PersistedSessionState) -> Result<()> {
    persist_session_state_at(&session_state_file_path(), state)
}

pub fn clear_session_state() -> Result<()> {
    clear_session_state_at(&session_state_file_path())
}

pub fn load_session_state_at(path: &Path) -> Result<Option<PersistedSessionState>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read session state file {}", path.display()))?;
    let state: PersistedSessionState = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse session state file {}", path.display()))?;
    Ok(Some(state))
}

pub fn persist_session_state_at(path: &Path, state: &PersistedSessionState) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory {}", parent.display()))?;
    }
    fs::write(path, serde_json::to_string_pretty(state)?)
        .with_context(|| format!("failed to write session state file {}", path.display()))?;
    Ok(())
}

pub fn clear_session_state_at(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_file(path)
            .with_context(|| format!("failed to remove session state file {}", path.display()))?;
    }
    Ok(())
}

// ── Session log persistence ───────────────────────────────────────────────────

pub fn load_session_log() -> Result<Option<Vec<PersistedSessionLogEntry>>> {
    load_session_log_at(&session_log_file_path())
}

pub fn persist_session_log(entries: &[PersistedSessionLogEntry]) -> Result<()> {
    persist_session_log_at(&session_log_file_path(), entries)
}

pub fn append_session_log_entry(entry: PersistedSessionLogEntry) -> Result<()> {
    append_session_log_entry_at(&session_log_file_path(), entry)
}

pub fn load_session_log_at(path: &Path) -> Result<Option<Vec<PersistedSessionLogEntry>>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read session log file {}", path.display()))?;

    // Backward compat: legacy format is a JSON array of entries.
    if let Ok(entries) = serde_json::from_str::<Vec<PersistedSessionLogEntry>>(&content) {
        return Ok(Some(entries));
    }

    let log: PersistedSessionLog = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse session log file {}", path.display()))?;
    Ok(Some(log.entries))
}

pub fn persist_session_log_at(path: &Path, entries: &[PersistedSessionLogEntry]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory {}", parent.display()))?;
    }
    let payload = PersistedSessionLog {
        schema_version: "1.0.0".to_string(),
        entries: entries.to_vec(),
    };
    fs::write(path, serde_json::to_string_pretty(&payload)?)
        .with_context(|| format!("failed to write session log file {}", path.display()))?;
    Ok(())
}

pub fn append_session_log_entry_at(path: &Path, entry: PersistedSessionLogEntry) -> Result<()> {
    let mut entries = load_session_log_at(path)?.unwrap_or_default();
    entries.push(entry);
    persist_session_log_at(path, &entries)
}

// ── Trust store persistence ───────────────────────────────────────────────────

pub fn load_trust_store() -> Result<LocalTrustStore> {
    load_trust_store_at(&trust_store_file_path())
}

pub fn persist_trust_store(store: &LocalTrustStore) -> Result<()> {
    persist_trust_store_at(&trust_store_file_path(), store)
}

pub fn load_trust_store_at(path: &Path) -> Result<LocalTrustStore> {
    if !path.exists() {
        return Ok(LocalTrustStore {
            schema_version: "1.0.0".to_string(),
            records: vec![],
        });
    }
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read trust store file {}", path.display()))?;
    let store: LocalTrustStore = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse trust store file {}", path.display()))?;
    Ok(store)
}

pub fn persist_trust_store_at(path: &Path, store: &LocalTrustStore) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory {}", parent.display()))?;
    }
    fs::write(path, serde_json::to_string_pretty(store)?)
        .with_context(|| format!("failed to write trust store file {}", path.display()))?;
    Ok(())
}

// ── Policy profile persistence ────────────────────────────────────────────────

pub fn load_policy_profile() -> Result<PolicyProfile> {
    let path = policy_file_path();
    if !path.exists() {
        return Ok(PolicyProfile::Balanced);
    }
    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read policy file {}", path.display()))?;
    let value: Value = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse policy file {}", path.display()))?;
    let Some(profile) = value.get("policy_profile").and_then(Value::as_str) else {
        anyhow::bail!(
            "invalid policy file {}: missing policy_profile",
            path.display()
        );
    };
    parse_policy_profile(profile)
}

pub fn load_policy_profile_or_default() -> PolicyProfile {
    match load_policy_profile() {
        Ok(profile) => profile,
        Err(err) => {
            tracing::warn!(
                "failed to load persisted trust policy profile ({}); defaulting to balanced",
                err
            );
            PolicyProfile::Balanced
        }
    }
}

pub fn persist_policy_profile(profile: PolicyProfile) -> Result<()> {
    let dir = config_dir_path();
    fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create config directory {}", dir.display()))?;
    let path = policy_file_path();
    let payload = json!({
        "policy_profile": policy_profile_to_str(profile),
        "policy_version": "1.0.0"
    });
    fs::write(&path, serde_json::to_string_pretty(&payload)?)
        .with_context(|| format!("failed to write policy file {}", path.display()))?;
    Ok(())
}

// ── Conversion helpers ────────────────────────────────────────────────────────

pub fn parse_policy_profile(value: &str) -> Result<PolicyProfile> {
    match value.to_lowercase().as_str() {
        "strict" => Ok(PolicyProfile::Strict),
        "balanced" => Ok(PolicyProfile::Balanced),
        "permissive" => Ok(PolicyProfile::Permissive),
        _ => anyhow::bail!("invalid policy profile"),
    }
}

pub fn policy_profile_to_str(profile: PolicyProfile) -> &'static str {
    match profile {
        PolicyProfile::Strict => "strict",
        PolicyProfile::Balanced => "balanced",
        PolicyProfile::Permissive => "permissive",
    }
}

pub fn key_trust_from_publication_state(publication_state: &str) -> PublicKeyTrustLevel {
    match publication_state {
        "published" => PublicKeyTrustLevel::Full,
        "pending" | "quarantined" => PublicKeyTrustLevel::Unknown,
        "rejected" | "revoked" => PublicKeyTrustLevel::Untrusted,
        _ => PublicKeyTrustLevel::Marginal,
    }
}

pub fn session_log_from_transitions(
    transitions: &[openpulse_core::hpx::HpxTransition],
) -> Vec<PersistedSessionLogEntry> {
    transitions
        .iter()
        .map(|t| PersistedSessionLogEntry {
            timestamp_ms: t.timestamp_ms,
            from_state: format!("{:?}", t.from_state).to_lowercase(),
            to_state: format!("{:?}", t.to_state).to_lowercase(),
            event: format!("{:?}", t.event).to_lowercase(),
            reason_code: format!("{:?}", t.reason_code).to_lowercase(),
            reason_string: t.reason_string.clone(),
        })
        .collect()
}

pub fn session_log_entry_to_value(entry: PersistedSessionLogEntry) -> serde_json::Value {
    json!({
        "timestamp_ms": entry.timestamp_ms,
        "from_state": entry.from_state,
        "to_state": entry.to_state,
        "event": entry.event,
        "reason_code": entry.reason_code,
        "reason_string": entry.reason_string,
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn publication_state_mapping_matches_policy_expectations() {
        assert_eq!(
            key_trust_from_publication_state("published"),
            PublicKeyTrustLevel::Full
        );
        assert_eq!(
            key_trust_from_publication_state("pending"),
            PublicKeyTrustLevel::Unknown
        );
        assert_eq!(
            key_trust_from_publication_state("quarantined"),
            PublicKeyTrustLevel::Unknown
        );
        assert_eq!(
            key_trust_from_publication_state("rejected"),
            PublicKeyTrustLevel::Untrusted
        );
        assert_eq!(
            key_trust_from_publication_state("revoked"),
            PublicKeyTrustLevel::Untrusted
        );
    }

    #[test]
    fn persisted_session_state_round_trips() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "openpulse-cli-session-state-{}-{}",
            std::process::id(),
            nonce
        ));
        let path = root.join("session-state.json");

        let state = PersistedSessionState {
            session_id: "sess-42".to_string(),
            peer: "N0CALL".to_string(),
            hpx_state: "activetransfer".to_string(),
            selected_mode: Some("normal".to_string()),
            trust_level: Some("verified".to_string()),
            policy_profile: "balanced".to_string(),
            updated_at_ms: 1234,
        };

        persist_session_state_at(&path, &state).expect("persist session state");
        let loaded = load_session_state_at(&path)
            .expect("load session state")
            .expect("state should exist");
        assert_eq!(loaded, state);

        clear_session_state_at(&path).expect("clear session state");
        let none = load_session_state_at(&path).expect("load after clear");
        assert!(none.is_none());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn trust_store_round_trips() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "openpulse-cli-trust-store-{}-{}",
            std::process::id(),
            nonce
        ));
        let path = root.join("trust-store.json");

        let store = LocalTrustStore {
            schema_version: "1.0.0".to_string(),
            records: vec![LocalTrustRecord {
                station_id: "N0CALL".to_string(),
                key_id: "key-42".to_string(),
                trust: PublicKeyTrustLevel::Full,
                source: CertificateSource::OutOfBand,
                status: "active".to_string(),
                reason: "operator_import".to_string(),
                updated_at_ms: 42,
            }],
        };

        persist_trust_store_at(&path, &store).expect("persist trust store");
        let loaded = load_trust_store_at(&path).expect("load trust store");
        assert_eq!(loaded, store);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn session_log_entries_append_in_order() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "openpulse-cli-session-log-{}-{}",
            std::process::id(),
            nonce
        ));
        let path = root.join("session-log.json");

        append_session_log_entry_at(
            &path,
            PersistedSessionLogEntry {
                timestamp_ms: 1,
                from_state: "idle".to_string(),
                to_state: "discovery".to_string(),
                event: "startsession".to_string(),
                reason_code: "success".to_string(),
                reason_string: "session start".to_string(),
            },
        )
        .expect("append first");
        append_session_log_entry_at(
            &path,
            PersistedSessionLogEntry {
                timestamp_ms: 2,
                from_state: "activetransfer".to_string(),
                to_state: "idle".to_string(),
                event: "localcancel".to_string(),
                reason_code: "session_ended".to_string(),
                reason_string: "session closed".to_string(),
            },
        )
        .expect("append second");

        let loaded = load_session_log_at(&path)
            .expect("load session log")
            .expect("entries should exist");
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].event, "startsession");
        assert_eq!(loaded[1].event, "localcancel");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn session_log_persists_versioned_container_schema() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "openpulse-cli-session-log-schema-{}-{}",
            std::process::id(),
            nonce
        ));
        let path = root.join("session-log.json");

        persist_session_log_at(
            &path,
            &[PersistedSessionLogEntry {
                timestamp_ms: 10,
                from_state: "idle".to_string(),
                to_state: "ready".to_string(),
                event: "startsession".to_string(),
                reason_code: "success".to_string(),
                reason_string: "ok".to_string(),
            }],
        )
        .expect("persist session log");

        let content = fs::read_to_string(&path).expect("read session log file");
        let payload: serde_json::Value = serde_json::from_str(&content).expect("json");
        assert_eq!(payload["schema_version"], "1.0.0");
        assert!(payload["entries"].is_array());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn session_log_loader_accepts_legacy_array_schema() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "openpulse-cli-session-log-legacy-{}-{}",
            std::process::id(),
            nonce
        ));
        let path = root.join("session-log.json");

        fs::create_dir_all(&root).expect("create temp root");
        fs::write(
            &path,
            r#"[
                {
                    "timestamp_ms": 1,
                    "from_state": "idle",
                    "to_state": "discovery",
                    "event": "startsession",
                    "reason_code": "success",
                    "reason_string": "session start"
                }
            ]"#,
        )
        .expect("write legacy log");

        let loaded = load_session_log_at(&path)
            .expect("load session log")
            .expect("entries should exist");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].event, "startsession");

        let _ = fs::remove_dir_all(root);
    }
}
