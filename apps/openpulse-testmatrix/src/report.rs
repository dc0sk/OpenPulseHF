use std::fs;
use std::path::Path;

use chrono::{DateTime, Utc};
use openpulse_core::compression::CompressionAlgorithm;
use serde::{Deserialize, Serialize};

use crate::compare::{compare_runs, write_comparison};
use crate::matrix::{fec_label, ChannelSpec, TestResult, UseCase};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunMeta {
    /// UTC timestamp of this run.
    pub date: DateTime<Utc>,
    /// Short (7-char) git commit SHA.
    pub git_commit: String,
    /// Full 40-char git commit SHA.
    pub git_commit_full: String,
    /// True when uncommitted changes were present at run time.
    pub git_dirty: bool,
    /// Workspace crate version (all crates share this via `version.workspace`).
    pub workspace_version: String,
    /// Which tier was run.
    pub tier: String,
    /// Wall-clock seconds for the full run.
    pub duration_secs: f64,
    /// Crates exercised by this test matrix run.
    pub crates_tested: Vec<String>,
}

impl RunMeta {
    /// Human-readable one-line run identity for use in report bodies.
    pub fn identity_line(&self) -> String {
        let dirty = if self.git_dirty { " ⚠ dirty" } else { "" };
        format!(
            "commit `{}`{dirty} — v{} — {}",
            self.git_commit,
            self.workspace_version,
            self.date.format("%Y-%m-%d %H:%M:%S UTC"),
        )
    }
}

// ── Archival ─────────────────────────────────────────────────────────────────

/// Copy every regular file from `latest/` into `archive/<datetime>-<commit>/`.
/// Returns the archive path on success.
fn archive_latest(latest: &Path, archive_root: &Path, meta: &RunMeta) -> Option<()> {
    let dir_name = format!(
        "{}-{}",
        meta.date.format("%Y-%m-%dT%H%M%S"),
        meta.git_commit
    );
    let dest = archive_root.join(dir_name);
    fs::create_dir_all(&dest).ok()?;

    for entry in fs::read_dir(latest).ok()?.flatten() {
        let src = entry.path();
        if src.is_file() {
            if let Some(name) = src.file_name() {
                fs::copy(&src, dest.join(name)).ok();
            }
        }
    }
    Some(())
}

/// Load raw results + meta from a previous run in `latest/`, if present.
fn load_previous_run(latest: &Path) -> Option<(Vec<TestResult>, RunMeta)> {
    let raw = fs::read_to_string(latest.join("raw.json")).ok()?;
    let results: Vec<TestResult> = serde_json::from_str(&raw).ok()?;
    let meta_raw = fs::read_to_string(latest.join("meta.json")).ok()?;
    let meta: RunMeta = serde_json::from_str(&meta_raw).ok()?;
    Some((results, meta))
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn write_reports(results: &[TestResult], output_dir: &Path, meta: &RunMeta) {
    let latest = output_dir.join("latest");
    let archive_root = output_dir.join("archive");

    // Snapshot previous run before overwriting latest/.
    let previous = load_previous_run(&latest);
    if let Some((_, ref prev_meta)) = previous {
        fs::create_dir_all(&archive_root).expect("create archive dir");
        archive_latest(&latest, &archive_root, prev_meta);
    }

    fs::create_dir_all(&latest).expect("create latest dir");

    write_meta_json(&latest, meta);
    write_summary(&latest, results, meta);
    write_by_mode(&latest, results, meta);
    write_by_channel(&latest, results, meta);
    write_by_usecase(&latest, results, meta);
    write_csv(&latest, results, meta);
    write_raw_json(&latest, results);

    if let Some((prev_results, prev_meta)) = previous {
        let diffs = compare_runs(&prev_results, results);
        write_comparison(&latest, &diffs, &prev_meta, meta);
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn write_meta_json(dir: &Path, meta: &RunMeta) {
    let json = serde_json::to_string_pretty(meta).expect("serialize meta");
    fs::write(dir.join("meta.json"), json).expect("write meta.json");
}

/// Build YAML frontmatter shared by all Markdown reports.
fn frontmatter(title: &str, subtitle: &str, meta: &RunMeta, total: usize, passed: usize) -> String {
    let dirty = if meta.git_dirty { "true" } else { "false" };
    let crates = meta
        .crates_tested
        .iter()
        .map(|c| format!("  - \"{c}\""))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "---\ntitle: \"{title} — {subtitle}\"\ndate: \"{}\"\ngit_commit: \"{}\"\ngit_commit_full: \"{}\"\ngit_dirty: {dirty}\nworkspace_version: \"{}\"\ntier: \"{}\"\ntotal_cases: {total}\npassed: {passed}\nfailed: {}\nduration_s: {:.1}\ngenerator: \"openpulse-testmatrix\"\ncrates_tested:\n{crates}\n---\n\n",
        meta.date.format("%Y-%m-%dT%H:%M:%SZ"),
        meta.git_commit,
        meta.git_commit_full,
        meta.workspace_version,
        meta.tier,
        total.saturating_sub(passed),
        meta.duration_secs,
    )
}

// ── Report writers ────────────────────────────────────────────────────────────

fn write_summary(dir: &Path, results: &[TestResult], meta: &RunMeta) {
    let active: Vec<_> = results.iter().filter(|r| !r.skipped).collect();
    let skipped_count = results.len() - active.len();
    let total = active.len();
    let passed = active.iter().filter(|r| r.passed).count();
    let mut out = frontmatter("OpenPulseHF Test Matrix", "Summary", meta, total, passed);

    out.push_str("# Test Matrix Summary\n\n");
    out.push_str(&format!("**Run:** {}\n\n", meta.identity_line()));
    out.push_str(&format!(
        "**{passed}/{total} cases passed** in {:.1}s",
        meta.duration_secs
    ));
    if skipped_count > 0 {
        out.push_str(&format!(" ({skipped_count} skipped)"));
    }
    out.push_str("\n\n");

    out.push_str("## By Use Case\n\n");
    out.push_str("| Use Case | Passed | Total | Skipped | Pass Rate |\n");
    out.push_str("|---|---|---|---|---|\n");
    for use_case in &[
        UseCase::RawModem,
        UseCase::AdaptiveHpx500,
        UseCase::AdaptiveHpxHf,
        UseCase::AdaptiveHpxWideband,
        UseCase::AdaptiveHpxOfdmHf,
        UseCase::Ardop,
        UseCase::Kiss,
        UseCase::B2f,
    ] {
        let uc_results: Vec<_> = results
            .iter()
            .filter(|r| &r.case.use_case == use_case)
            .collect();
        if uc_results.is_empty() {
            continue;
        }
        let uc_skipped = uc_results.iter().filter(|r| r.skipped).count();
        let uc_active: Vec<_> = uc_results.iter().filter(|r| !r.skipped).collect();
        let uc_passed = uc_active.iter().filter(|r| r.passed).count();
        let uc_total = uc_active.len();
        let rate = (100 * uc_passed).checked_div(uc_total).unwrap_or(0);
        out.push_str(&format!(
            "| {} | {uc_passed} | {uc_total} | {uc_skipped} | {rate}% |\n",
            use_case.label()
        ));
    }

    let failures: Vec<_> = results.iter().filter(|r| !r.skipped && !r.passed).collect();
    if !failures.is_empty() {
        out.push_str("\n## Failures\n\n");
        out.push_str("| Case ID | Note |\n");
        out.push_str("|---|---|\n");
        for f in failures.iter().take(50) {
            let note = f.note.as_deref().unwrap_or("");
            out.push_str(&format!("| `{}` | {} |\n", f.case.id(), note));
        }
        if failures.len() > 50 {
            out.push_str(&format!(
                "\n*…and {} more failures. See `raw.json` for full list.*\n",
                failures.len() - 50
            ));
        }
    }

    fs::write(dir.join("summary.md"), out).expect("write summary.md");
}

fn write_by_mode(dir: &Path, results: &[TestResult], meta: &RunMeta) {
    let active: Vec<_> = results.iter().filter(|r| !r.skipped).collect();
    let total = active.len();
    let passed = active.iter().filter(|r| r.passed).count();
    let mut out = frontmatter("OpenPulseHF Test Matrix", "By Mode", meta, total, passed);

    out.push_str("# Results by Mode\n\n");
    out.push_str(&format!("**Run:** {}\n\n", meta.identity_line()));

    let modes: Vec<String> = results
        .iter()
        .map(|r| r.case.mode.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    let channels: Vec<String> = results
        .iter()
        .map(|r| r.case.channel.label())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    out.push_str("| Mode |");
    for ch in &channels {
        out.push_str(&format!(" {ch} |"));
    }
    out.push_str(" Total |\n|---|");
    for _ in &channels {
        out.push_str("---|");
    }
    out.push_str("---|\n");

    for mode in &modes {
        out.push_str(&format!("| **{mode}** |"));
        let mut mode_pass = 0;
        let mut mode_total = 0;
        for ch in &channels {
            let subset: Vec<_> = results
                .iter()
                .filter(|r| !r.skipped && &r.case.mode == mode && r.case.channel.label() == *ch)
                .collect();
            if subset.is_empty() {
                out.push_str(" — |");
            } else {
                let p = subset.iter().filter(|r| r.passed).count();
                let t = subset.len();
                mode_pass += p;
                mode_total += t;
                let cell = if p == t {
                    format!("✓ {p}/{t}")
                } else {
                    format!("✗ {p}/{t}")
                };
                out.push_str(&format!(" {cell} |"));
            }
        }
        out.push_str(&format!(" **{mode_pass}/{mode_total}** |\n"));
    }

    fs::write(dir.join("by-mode.md"), out).expect("write by-mode.md");
}

fn write_by_channel(dir: &Path, results: &[TestResult], meta: &RunMeta) {
    let active: Vec<_> = results.iter().filter(|r| !r.skipped).collect();
    let total = active.len();
    let passed = active.iter().filter(|r| r.passed).count();
    let mut out = frontmatter("OpenPulseHF Test Matrix", "By Channel", meta, total, passed);

    out.push_str("# Results by Channel\n\n");
    out.push_str(&format!("**Run:** {}\n\n", meta.identity_line()));

    let channels: Vec<String> = results
        .iter()
        .map(|r| r.case.channel.label())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    let modes: Vec<String> = results
        .iter()
        .map(|r| r.case.mode.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    out.push_str("| Channel |");
    for m in &modes {
        out.push_str(&format!(" {m} |"));
    }
    out.push_str(" Total |\n|---|");
    for _ in &modes {
        out.push_str("---|");
    }
    out.push_str("---|\n");

    for ch in &channels {
        out.push_str(&format!("| **{ch}** |"));
        let mut ch_pass = 0;
        let mut ch_total = 0;
        for mode in &modes {
            let subset: Vec<_> = results
                .iter()
                .filter(|r| !r.skipped && r.case.channel.label() == *ch && &r.case.mode == mode)
                .collect();
            if subset.is_empty() {
                out.push_str(" — |");
            } else {
                let p = subset.iter().filter(|r| r.passed).count();
                let t = subset.len();
                ch_pass += p;
                ch_total += t;
                let cell = if p == t {
                    format!("✓ {p}/{t}")
                } else {
                    format!("✗ {p}/{t}")
                };
                out.push_str(&format!(" {cell} |"));
            }
        }
        out.push_str(&format!(" **{ch_pass}/{ch_total}** |\n"));
    }

    fs::write(dir.join("by-channel.md"), out).expect("write by-channel.md");
}

fn write_by_usecase(dir: &Path, results: &[TestResult], meta: &RunMeta) {
    let active: Vec<_> = results.iter().filter(|r| !r.skipped).collect();
    let total = active.len();
    let passed = active.iter().filter(|r| r.passed).count();
    let mut out = frontmatter(
        "OpenPulseHF Test Matrix",
        "By Use Case",
        meta,
        total,
        passed,
    );

    out.push_str("# Results by Use Case\n\n");
    out.push_str(&format!("**Run:** {}\n\n", meta.identity_line()));
    out.push_str(
        "| Use Case | Mode | Channel | FEC | Compression | Payload | Result | BER | Eff. bps | Duration |\n",
    );
    out.push_str("|---|---|---|---|---|---|---|---|---|---|\n");

    for r in results {
        let status = if r.skipped {
            "— SKIP"
        } else if r.passed {
            "✓ PASS"
        } else {
            "✗ FAIL"
        };
        let ber = r
            .ber
            .map(|b| format!("{b:.4}"))
            .unwrap_or_else(|| "—".into());
        let comp = match r.case.compression {
            CompressionAlgorithm::None => "none",
            CompressionAlgorithm::Lz4 => "lz4",
            CompressionAlgorithm::Zstd(_) => "zstd",
        };
        let eff_bps = r
            .effective_bps
            .map(|b| format!("{b:.0}"))
            .unwrap_or_else(|| "—".into());
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {}B | {} | {} | {} | {}ms |\n",
            r.case.use_case.label(),
            r.case.mode,
            r.case.channel.label(),
            fec_label(r.case.fec_mode),
            comp,
            r.case.payload_len,
            status,
            ber,
            eff_bps,
            r.duration_ms,
        ));
    }

    fs::write(dir.join("by-usecase.md"), out).expect("write by-usecase.md");
}

fn write_csv(dir: &Path, results: &[TestResult], meta: &RunMeta) {
    let mut out = String::new();
    // run_date and run_commit are included as the first two columns so every row
    // is self-contained when multiple CSV files are concatenated for analysis.
    out.push_str(
        "run_date,run_commit,use_case,mode,fec,compression,channel,snr_db,payload_bytes,passed,skipped,ber,effective_bps,duration_ms,note\n",
    );

    let run_date = meta.date.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let dirty = if meta.git_dirty { "*" } else { "" };
    let run_commit = format!("{}{dirty}", meta.git_commit);

    for r in results {
        let comp = match r.case.compression {
            CompressionAlgorithm::None => "none",
            CompressionAlgorithm::Lz4 => "lz4",
            CompressionAlgorithm::Zstd(_) => "zstd",
        };
        let snr_db = match &r.case.channel {
            ChannelSpec::Awgn { snr_db, .. } => format!("{snr_db:.1}"),
            _ => "".into(),
        };
        let ber = r.ber.map(|b| format!("{b:.6}")).unwrap_or_default();
        let eff_bps = r
            .effective_bps
            .map(|b| format!("{b:.1}"))
            .unwrap_or_default();
        let note = r.note.as_deref().unwrap_or("").replace('"', "\"\"");
        out.push_str(&format!(
            "{run_date},{run_commit},{},{},{},{},{},{},{},{},{},{},{},{},\"{}\"\n",
            r.case.use_case.label(),
            r.case.mode,
            fec_label(r.case.fec_mode),
            comp,
            r.case.channel.label(),
            snr_db,
            r.case.payload_len,
            r.passed as u8,
            r.skipped as u8,
            ber,
            eff_bps,
            r.duration_ms,
            note,
        ));
    }

    fs::write(dir.join("results.csv"), out).expect("write results.csv");
}

fn write_raw_json(dir: &Path, results: &[TestResult]) {
    let json = serde_json::to_string_pretty(results).expect("serialize results");
    fs::write(dir.join("raw.json"), json).expect("write raw.json");
}
