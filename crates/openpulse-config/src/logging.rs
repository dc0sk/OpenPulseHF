//! `tracing` initialisation with an optional persistent rolling file log (REQ-OBS-02).

use std::path::{Path, PathBuf};

use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};

use crate::LoggingConfig;

/// Expand a leading `~` / `~/` in a path to the user's home directory.
pub fn expand_tilde(raw: &str) -> PathBuf {
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    } else if raw == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    PathBuf::from(raw)
}

/// Build a non-blocking, daily-rolled file writer at `path` (creating its parent
/// directory). The rolled file is `<file_name>.<YYYY-MM-DD>` in the parent dir.
/// Returns the writer and its flush guard, which the caller must keep alive.
fn file_writer(path: &Path) -> std::io::Result<(NonBlocking, WorkerGuard)> {
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let name = path.file_name().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "log file path has no file name",
        )
    })?;
    std::fs::create_dir_all(dir)?;
    let appender = tracing_appender::rolling::daily(dir, name);
    Ok(tracing_appender::non_blocking(appender))
}

/// Initialise the global `tracing` subscriber for a long-running binary.
///
/// Always logs to stdout. When `cfg.file` is set, also appends to a daily-rolled file
/// (REQ-OBS-02). Level honours `RUST_LOG` over `cfg.level`. Returns the file appender's
/// [`WorkerGuard`] when file logging is active — the caller MUST keep it alive
/// (typically by binding it in `main`) or buffered lines are lost on exit.
#[must_use = "keep the returned WorkerGuard alive for the process lifetime, or file logs are dropped"]
pub fn init_tracing(cfg: &LoggingConfig) -> Option<WorkerGuard> {
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::{fmt, EnvFilter};

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&cfg.level));
    let stdout_layer = fmt::layer().with_target(false);

    let file = cfg.file.as_deref().and_then(|raw| {
        let path = expand_tilde(raw);
        match file_writer(&path) {
            Ok(w) => Some((path, w)),
            Err(e) => {
                eprintln!(
                    "warning: could not open log file {}: {e}; continuing with stdout only",
                    path.display()
                );
                None
            }
        }
    });

    match file {
        Some((path, (writer, guard))) => {
            let file_layer = fmt::layer()
                .with_ansi(false)
                .with_target(false)
                .with_writer(writer);
            let _ = tracing_subscriber::registry()
                .with(filter)
                .with(stdout_layer)
                .with(file_layer)
                .try_init();
            tracing::info!(path = %path.display(), "persistent file logging enabled (REQ-OBS-02)");
            Some(guard)
        }
        None => {
            let _ = tracing_subscriber::registry()
                .with(filter)
                .with(stdout_layer)
                .try_init();
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logging_config_file_defaults_to_none() {
        assert!(LoggingConfig::default().file.is_none());
    }

    #[test]
    fn expand_tilde_expands_home_and_passes_through() {
        if let Some(home) = dirs::home_dir() {
            assert_eq!(expand_tilde("~/x/y.log"), home.join("x/y.log"));
            assert_eq!(expand_tilde("~"), home);
        }
        assert_eq!(expand_tilde("/abs/z.log"), PathBuf::from("/abs/z.log"));
        assert_eq!(expand_tilde("rel.log"), PathBuf::from("rel.log"));
    }

    #[test]
    fn file_writer_creates_dir_and_writes_a_line() {
        let dir = std::env::temp_dir().join(format!("openpulse-logtest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("openpulse.log");

        let (writer, guard) = file_writer(&path).expect("file_writer should succeed");
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(writer)
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("audit-marker-abc123");
        });
        drop(guard); // flush the non-blocking worker

        // The daily appender writes `<name>.<date>`; scan the dir for our marker.
        let mut found = false;
        for entry in std::fs::read_dir(&dir).expect("dir should exist") {
            let p = entry.expect("dir entry").path();
            if let Ok(s) = std::fs::read_to_string(&p) {
                if s.contains("audit-marker-abc123") {
                    found = true;
                }
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert!(found, "expected the log marker in a file under the log dir");
    }
}
