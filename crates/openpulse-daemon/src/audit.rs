//! Audit-mode recording (REQ-OBS-01): append the daemon's control-event stream to
//! `<archive_dir>/events.ndjson` so a run can be analysed after the fact without a live client.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::protocol::ControlEvent;

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
}
