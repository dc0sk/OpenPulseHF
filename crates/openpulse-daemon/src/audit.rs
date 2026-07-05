//! Audit-mode recording (REQ-OBS-01): a startup `snapshot.json` plus the daemon's
//! control-event stream appended to `<archive_dir>/events.ndjson`, so a run can be
//! analysed after the fact without a live client.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use openpulse_config::OpenpulseConfig;

use crate::protocol::ControlEvent;

/// Config keys whose string values are blanked in the snapshot (secret material).
fn is_secret_key(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    k == "key"
        || k.ends_with("_key")
        || k.contains("secret")
        || k.contains("password")
        || k.contains("passphrase")
        || k.contains("token")
        || k.contains("seed")
}

/// Recursively blank secret string values in a JSON tree (see [`is_secret_key`]).
/// Non-secret identifiers like `key_id` / `pubkey` are preserved.
pub fn redact_secrets(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                if is_secret_key(k) && v.is_string() {
                    *v = serde_json::Value::String("***REDACTED***".to_string());
                } else {
                    redact_secrets(v);
                }
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(redact_secrets),
        _ => {}
    }
}

/// Build the startup snapshot (REQ-OBS-01): version/build/runtime metadata plus the
/// running configuration with secret string values redacted. Metadata is injected so
/// the builder stays pure and testable.
pub fn build_snapshot(
    cfg: &OpenpulseConfig,
    version: &str,
    git_sha: &str,
    captured_at_unix_ms: u128,
) -> serde_json::Value {
    let mut config = serde_json::to_value(cfg).unwrap_or(serde_json::Value::Null);
    redact_secrets(&mut config);
    serde_json::json!({
        "schema": "openpulse-audit-snapshot/1",
        "captured_at_unix_ms": captured_at_unix_ms.to_string(),
        "version": version,
        "git_sha": git_sha,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "config": config,
    })
}

/// Write `<archive_dir>/snapshot.json` at startup (creating the directory). Best effort:
/// returns the error to the caller, which logs rather than fails.
pub fn write_startup_snapshot(archive_dir: &Path, cfg: &OpenpulseConfig) -> std::io::Result<()> {
    std::fs::create_dir_all(archive_dir)?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let git_sha = option_env!("OPENPULSE_GIT_SHA").unwrap_or("unknown");
    let snapshot = build_snapshot(cfg, env!("CARGO_PKG_VERSION"), git_sha, now_ms);
    let body = serde_json::to_string_pretty(&snapshot)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(archive_dir.join("snapshot.json"), body)
}

/// Appends serialized [`ControlEvent`]s as newline-delimited JSON to `events.ndjson`.
pub struct EventRecorder {
    writer: BufWriter<File>,
}

impl EventRecorder {
    /// Open (creating the directory + file) `<archive_dir>/events.ndjson` in append mode.
    pub fn open(archive_dir: &Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(archive_dir)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(archive_dir.join("events.ndjson"))?;
        Ok(Self {
            writer: BufWriter::new(file),
        })
    }

    /// Append one event as a JSON line, flushing so an abrupt exit keeps prior lines.
    pub fn record(&mut self, event: &ControlEvent) -> std::io::Result<()> {
        let line = serde_json::to_string(event)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        self.writer.write_all(line.as_bytes())?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()
    }
}

/// Spawn a task that records every broadcast [`ControlEvent`] to the audit log until the
/// channel closes. On open failure it logs a warning and disables recording (never fatal).
pub fn spawn_event_recorder(
    archive_dir: PathBuf,
    mut rx: tokio::sync::broadcast::Receiver<ControlEvent>,
) {
    use tokio::sync::broadcast::error::RecvError;
    tokio::spawn(async move {
        let mut recorder = match EventRecorder::open(&archive_dir) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(
                    dir = %archive_dir.display(),
                    error = %e,
                    "audit: could not open events.ndjson; event recording disabled"
                );
                return;
            }
        };
        tracing::info!(
            path = %archive_dir.join("events.ndjson").display(),
            "audit mode: recording control events (REQ-OBS-01)"
        );
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    if let Err(e) = recorder.record(&ev) {
                        tracing::warn!(error = %e, "audit: failed to write event; stopping recorder");
                        return;
                    }
                }
                Err(RecvError::Lagged(n)) => {
                    tracing::warn!(
                        skipped = n,
                        "audit: event recorder lagged; some events dropped"
                    );
                }
                Err(RecvError::Closed) => return,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("openpulse-audit-{tag}-{}", std::process::id()))
    }

    #[test]
    fn recorder_writes_one_json_line_per_event() {
        let dir = temp_dir("lines");
        let _ = std::fs::remove_dir_all(&dir);
        {
            let mut r = EventRecorder::open(&dir).expect("open");
            r.record(&ControlEvent::PttChanged { active: true })
                .expect("record 1");
            r.record(&ControlEvent::PttChanged { active: false })
                .expect("record 2");
        }
        let content = std::fs::read_to_string(dir.join("events.ndjson")).expect("read");
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2, "one line per event");
        for l in &lines {
            let _: serde_json::Value = serde_json::from_str(l).expect("each line is valid JSON");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recorder_appends_across_reopens() {
        let dir = temp_dir("append");
        let _ = std::fs::remove_dir_all(&dir);
        {
            let mut r = EventRecorder::open(&dir).expect("open 1");
            r.record(&ControlEvent::PttChanged { active: true })
                .expect("record");
        }
        {
            let mut r = EventRecorder::open(&dir).expect("open 2");
            r.record(&ControlEvent::PttChanged { active: false })
                .expect("record");
        }
        let content = std::fs::read_to_string(dir.join("events.ndjson")).expect("read");
        assert_eq!(
            content.lines().count(),
            2,
            "second open appends, not truncates"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn redact_blanks_secret_keys_but_keeps_identifiers() {
        let mut v = serde_json::json!({
            "signing_key": "abcd",
            "api_key": "xyz",
            "key_id": "pub-123",
            "pubkey": "pub-abc",
            "nested": { "password": "hunter2", "port": 9000 },
            "list": [ { "seed": "s" } ],
        });
        redact_secrets(&mut v);
        assert_eq!(v["signing_key"], "***REDACTED***");
        assert_eq!(v["api_key"], "***REDACTED***");
        assert_eq!(v["key_id"], "pub-123", "identifier preserved");
        assert_eq!(v["pubkey"], "pub-abc", "identifier preserved");
        assert_eq!(v["nested"]["password"], "***REDACTED***");
        assert_eq!(v["nested"]["port"], 9000);
        assert_eq!(v["list"][0]["seed"], "***REDACTED***");
    }

    #[test]
    fn snapshot_has_metadata_and_config() {
        let cfg = OpenpulseConfig::default();
        let snap = build_snapshot(&cfg, "9.9.9", "deadbeef", 1234);
        assert_eq!(snap["schema"], "openpulse-audit-snapshot/1");
        assert_eq!(snap["version"], "9.9.9");
        assert_eq!(snap["git_sha"], "deadbeef");
        assert!(snap["config"]["daemon"].is_object());
    }
}
