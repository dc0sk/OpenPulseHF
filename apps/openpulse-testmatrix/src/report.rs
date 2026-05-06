use std::fs;
use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, Utc};
use openpulse_core::compression::CompressionAlgorithm;

use crate::matrix::{TestResult, UseCase};

pub struct RunMeta {
    pub date: DateTime<Utc>,
    pub git_commit: String,
    pub tier: String,
    pub duration: Duration,
}

pub fn write_reports(results: &[TestResult], output_dir: &Path, meta: &RunMeta) {
    let latest = output_dir.join("latest");
    fs::create_dir_all(&latest).expect("create latest dir");

    write_summary(&latest, results, meta);
    write_by_mode(&latest, results, meta);
    write_by_channel(&latest, results, meta);
    write_by_usecase(&latest, results, meta);
    write_raw_json(&latest, results);
}

fn frontmatter(title: &str, subtitle: &str, meta: &RunMeta, total: usize, passed: usize) -> String {
    format!(
        "---\ntitle: \"{title} — {subtitle}\"\ndate: \"{}\"\ngit_commit: \"{}\"\ntier: \"{}\"\ntotal_cases: {total}\npassed: {passed}\nfailed: {}\nduration_s: {}\ngenerator: \"openpulse-testmatrix\"\n---\n\n",
        meta.date.format("%Y-%m-%dT%H:%M:%SZ"),
        meta.git_commit,
        meta.tier,
        total - passed,
        meta.duration.as_secs(),
    )
}

fn write_summary(dir: &Path, results: &[TestResult], meta: &RunMeta) {
    let total = results.len();
    let passed = results.iter().filter(|r| r.passed).count();
    let mut out = frontmatter("OpenPulseHF Test Matrix", "Summary", meta, total, passed);

    out.push_str("# Test Matrix Summary\n\n");
    out.push_str(&format!(
        "**{passed}/{total} cases passed** in {}s\n\n",
        meta.duration.as_secs()
    ));

    // Per-use-case summary table
    out.push_str("## By Use Case\n\n");
    out.push_str("| Use Case | Passed | Total | Pass Rate |\n");
    out.push_str("|---|---|---|---|\n");
    for use_case in &[
        UseCase::RawModem,
        UseCase::AdaptiveHpx500,
        UseCase::AdaptiveHpx2300,
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
        let uc_passed = uc_results.iter().filter(|r| r.passed).count();
        let uc_total = uc_results.len();
        let rate = 100 * uc_passed / uc_total;
        out.push_str(&format!(
            "| {} | {uc_passed} | {uc_total} | {rate}% |\n",
            use_case.label()
        ));
    }

    // First failures
    let failures: Vec<_> = results.iter().filter(|r| !r.passed).collect();
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
    let total = results.len();
    let passed = results.iter().filter(|r| r.passed).count();
    let mut out = frontmatter("OpenPulseHF Test Matrix", "By Mode", meta, total, passed);

    out.push_str("# Results by Mode\n\n");

    // Collect unique modes
    let modes: Vec<String> = results
        .iter()
        .map(|r| r.case.mode.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    // Collect unique channel labels
    let channels: Vec<String> = results
        .iter()
        .map(|r| r.case.channel.label())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    // Header
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
                .filter(|r| &r.case.mode == mode && r.case.channel.label() == *ch)
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
    let total = results.len();
    let passed = results.iter().filter(|r| r.passed).count();
    let mut out = frontmatter("OpenPulseHF Test Matrix", "By Channel", meta, total, passed);

    out.push_str("# Results by Channel\n\n");

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
                .filter(|r| r.case.channel.label() == *ch && &r.case.mode == mode)
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
    let total = results.len();
    let passed = results.iter().filter(|r| r.passed).count();
    let mut out = frontmatter(
        "OpenPulseHF Test Matrix",
        "By Use Case",
        meta,
        total,
        passed,
    );

    out.push_str("# Results by Use Case\n\n");
    out.push_str(
        "| Use Case | Mode | Channel | FEC | Compression | Payload | Result | BER | Duration |\n",
    );
    out.push_str("|---|---|---|---|---|---|---|---|---|\n");

    for r in results {
        let status = if r.passed { "✓ PASS" } else { "✗ FAIL" };
        let ber = r
            .ber
            .map(|b| format!("{b:.4}"))
            .unwrap_or_else(|| "—".into());
        let comp = match r.case.compression {
            CompressionAlgorithm::None => "none",
            CompressionAlgorithm::Lz4 => "lz4",
        };
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {}B | {} | {} | {}ms |\n",
            r.case.use_case.label(),
            r.case.mode,
            r.case.channel.label(),
            if r.case.fec { "yes" } else { "no" },
            comp,
            r.case.payload_len,
            status,
            ber,
            r.duration_ms,
        ));
    }

    fs::write(dir.join("by-usecase.md"), out).expect("write by-usecase.md");
}

fn write_raw_json(dir: &Path, results: &[TestResult]) {
    let json = serde_json::to_string_pretty(results).expect("serialize results");
    fs::write(dir.join("raw.json"), json).expect("write raw.json");
}
