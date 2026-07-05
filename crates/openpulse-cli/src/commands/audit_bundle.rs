//! `openpulse audit-bundle` (REQ-OBS-03): package the audit-mode artifacts (events.ndjson,
//! snapshot.json, rolled log files) into a single `.tar.gz` for handoff to a developer.

use std::fs::File;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::json;

/// What to include in a bundle. Metadata (version, timestamp) is injected so the core stays
/// pure/testable.
pub struct BundleSpec<'a> {
    /// Directory holding the daemon's audit artifacts (`events.ndjson`, `snapshot.json`, …).
    pub archive_dir: &'a Path,
    /// Extra files to include under `logs/` (e.g. daily-rolled log files).
    pub extra_files: &'a [PathBuf],
    pub version: &'a str,
    pub created_at_unix_ms: u128,
    pub label: Option<&'a str>,
}

/// Write `audit-bundle-<ts>[-<label>].tar.gz` under `dest_root` containing a `metadata.json`
/// manifest plus every file directly under `archive_dir` and each of `extra_files` (as `logs/<name>`).
/// Missing files are skipped. Returns the bundle path.
pub fn create_bundle(spec: &BundleSpec, dest_root: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dest_root)?;

    // Collect (source path, name-in-archive) pairs.
    let mut files: Vec<(PathBuf, String)> = Vec::new();
    if spec.archive_dir.is_dir() {
        for entry in std::fs::read_dir(spec.archive_dir)? {
            let p = entry?.path();
            if p.is_file() {
                if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                    files.push((p.clone(), name.to_string()));
                }
            }
        }
    }
    for f in spec.extra_files {
        if f.is_file() {
            if let Some(name) = f.file_name().and_then(|n| n.to_str()) {
                files.push((f.clone(), format!("logs/{name}")));
            }
        }
    }
    files.sort_by(|a, b| a.1.cmp(&b.1));

    let included: Vec<_> = files
        .iter()
        .map(|(src, name)| {
            json!({
                "name": name,
                "bytes": std::fs::metadata(src).map(|m| m.len()).unwrap_or(0),
            })
        })
        .collect();
    let metadata = json!({
        "schema": "openpulse-audit-bundle/1",
        "created_at_unix_ms": spec.created_at_unix_ms.to_string(),
        "version": spec.version,
        "included": included,
    });
    let meta_bytes = serde_json::to_vec_pretty(&metadata)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    let label = spec.label.map(|l| format!("-{l}")).unwrap_or_default();
    let bundle_path = dest_root.join(format!(
        "audit-bundle-{}{}.tar.gz",
        spec.created_at_unix_ms, label
    ));

    let enc =
        flate2::write::GzEncoder::new(File::create(&bundle_path)?, flate2::Compression::default());
    let mut builder = tar::Builder::new(enc);

    let mut header = tar::Header::new_gnu();
    header.set_size(meta_bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder.append_data(&mut header, "metadata.json", &meta_bytes[..])?;

    for (src, name) in &files {
        let mut f = File::open(src)?;
        builder.append_file(name, &mut f)?;
    }
    builder.into_inner()?.finish()?;
    Ok(bundle_path)
}

/// CLI entry: resolve the archive dir + log files from config (overridable), build the bundle,
/// and print its path.
pub fn run(archive_dir: Option<&str>, output: Option<&str>, label: Option<&str>) -> Result<i32> {
    let cfg = openpulse_config::load().context("failed to load config")?;

    let archive = match archive_dir {
        Some(d) => openpulse_config::logging::expand_tilde(d),
        None => openpulse_config::logging::expand_tilde(&cfg.observability.archive_dir),
    };
    if !archive.is_dir() {
        anyhow::bail!(
            "audit archive dir {} does not exist — enable [observability] audit_mode and run the daemon first",
            archive.display()
        );
    }

    // Include rolled log files that share the configured log-file prefix.
    let extra_files = collect_log_files(cfg.logging.file.as_deref());

    let dest = match output {
        Some(d) => openpulse_config::logging::expand_tilde(d),
        None => archive.join("bundles"),
    };
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);

    let spec = BundleSpec {
        archive_dir: &archive,
        extra_files: &extra_files,
        version: env!("CARGO_PKG_VERSION"),
        created_at_unix_ms: now_ms,
        label,
    };
    let path = create_bundle(&spec, &dest).context("failed to write audit bundle")?;
    println!("{}", path.display());
    Ok(0)
}

/// Resolve the daily-rolled log files (`<name>.<date>`) next to a configured log-file path.
fn collect_log_files(configured: Option<&str>) -> Vec<PathBuf> {
    let Some(raw) = configured else {
        return Vec::new();
    };
    let path = openpulse_config::logging::expand_tilde(raw);
    let (Some(dir), Some(prefix)) = (path.parent(), path.file_name().and_then(|n| n.to_str()))
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() {
                if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with(prefix) {
                        out.push(p);
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_contains_archive_files_and_metadata() {
        let tmp = std::env::temp_dir().join(format!("openpulse-bundle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let archive = tmp.join("archive");
        std::fs::create_dir_all(&archive).unwrap();
        std::fs::write(archive.join("events.ndjson"), "{\"a\":1}\n").unwrap();
        std::fs::write(archive.join("snapshot.json"), "{}").unwrap();
        let dest = tmp.join("out");

        let spec = BundleSpec {
            archive_dir: &archive,
            extra_files: &[],
            version: "9.9.9",
            created_at_unix_ms: 42,
            label: Some("test"),
        };
        let path = create_bundle(&spec, &dest).expect("create_bundle");
        let fname = path.file_name().unwrap().to_str().unwrap();
        assert!(fname.starts_with("audit-bundle-42-test"), "got {fname}");
        assert!(fname.ends_with(".tar.gz"));

        // Decode the tar.gz and assert the expected entries are present.
        let dec = flate2::read::GzDecoder::new(File::open(&path).unwrap());
        let mut ar = tar::Archive::new(dec);
        let mut names: Vec<String> = Vec::new();
        for e in ar.entries().unwrap() {
            names.push(e.unwrap().path().unwrap().to_string_lossy().into_owned());
        }
        assert!(names.contains(&"metadata.json".to_string()));
        assert!(names.contains(&"events.ndjson".to_string()));
        assert!(names.contains(&"snapshot.json".to_string()));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
