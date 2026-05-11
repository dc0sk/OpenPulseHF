//! Comparison between two consecutive test matrix runs.
//!
//! Loads the previous run's results from `latest/raw.json` and `latest/meta.json`
//! before they are overwritten, diffs them against the new run, and writes
//! `latest/comparison.md` and `latest/comparison.json`.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::matrix::TestResult;
use crate::report::RunMeta;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffStatus {
    Unchanged,
    /// fail → pass
    Fixed,
    /// pass → fail
    Regressed,
    /// only in the new run
    NewCase,
    /// only in the old run
    Removed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseDiff {
    pub id: String,
    pub status: DiffStatus,
    pub old_passed: Option<bool>,
    pub new_passed: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ComparisonSummary {
    pub regressions: usize,
    pub fixed: usize,
    pub new_cases: usize,
    pub removed: usize,
    pub unchanged: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ComparisonReport {
    pub old_meta: RunMeta,
    pub new_meta: RunMeta,
    pub summary: ComparisonSummary,
    pub diffs: Vec<CaseDiff>,
}

/// Diff two runs keyed on `TestCase::id()`.  Skipped cases are excluded.
pub fn compare_runs(old: &[TestResult], new: &[TestResult]) -> Vec<CaseDiff> {
    let old_map: HashMap<String, bool> = old
        .iter()
        .filter(|r| !r.skipped)
        .map(|r| (r.case.id(), r.passed))
        .collect();
    let new_map: HashMap<String, bool> = new
        .iter()
        .filter(|r| !r.skipped)
        .map(|r| (r.case.id(), r.passed))
        .collect();

    let mut diffs: Vec<CaseDiff> = new_map
        .iter()
        .map(|(id, &new_passed)| {
            let status = match old_map.get(id) {
                Some(&old) if old == new_passed => DiffStatus::Unchanged,
                Some(&true) => DiffStatus::Regressed,
                Some(&false) => DiffStatus::Fixed,
                None => DiffStatus::NewCase,
            };
            CaseDiff {
                id: id.clone(),
                status,
                old_passed: old_map.get(id).copied(),
                new_passed: Some(new_passed),
            }
        })
        .chain(old_map.iter().filter_map(|(id, &old_passed)| {
            if new_map.contains_key(id) {
                None
            } else {
                Some(CaseDiff {
                    id: id.clone(),
                    status: DiffStatus::Removed,
                    old_passed: Some(old_passed),
                    new_passed: None,
                })
            }
        }))
        .collect();

    diffs.sort_by(|a, b| a.id.cmp(&b.id));
    diffs
}

pub fn write_comparison(dir: &Path, diffs: &[CaseDiff], old_meta: &RunMeta, new_meta: &RunMeta) {
    let regressions = diffs
        .iter()
        .filter(|d| d.status == DiffStatus::Regressed)
        .count();
    let fixed = diffs
        .iter()
        .filter(|d| d.status == DiffStatus::Fixed)
        .count();
    let new_cases = diffs
        .iter()
        .filter(|d| d.status == DiffStatus::NewCase)
        .count();
    let removed = diffs
        .iter()
        .filter(|d| d.status == DiffStatus::Removed)
        .count();
    let unchanged = diffs
        .iter()
        .filter(|d| d.status == DiffStatus::Unchanged)
        .count();

    let json_report = ComparisonReport {
        old_meta: old_meta.clone(),
        new_meta: new_meta.clone(),
        summary: ComparisonSummary {
            regressions,
            fixed,
            new_cases,
            removed,
            unchanged,
        },
        diffs: diffs.to_vec(),
    };
    let json = serde_json::to_string_pretty(&json_report).expect("serialize comparison");
    fs::write(dir.join("comparison.json"), json).expect("write comparison.json");

    let verdict = if regressions > 0 {
        format!("✗ **{regressions} regression(s) detected**")
    } else {
        "✓ No regressions".to_string()
    };
    let mut detail_parts = Vec::new();
    if fixed > 0 {
        detail_parts.push(format!("{fixed} fixed"));
    }
    if new_cases > 0 {
        detail_parts.push(format!("{new_cases} new cases"));
    }
    if removed > 0 {
        detail_parts.push(format!("{removed} removed"));
    }

    let dirty_old = if old_meta.git_dirty { " (dirty)" } else { "" };
    let dirty_new = if new_meta.git_dirty { " (dirty)" } else { "" };

    let mut out = format!(
        "---\ntitle: \"OpenPulseHF Test Matrix — Comparison\"\ndate: \"{}\"\nold_commit: \"{}{dirty_old}\"\nnew_commit: \"{}{dirty_new}\"\nregressions: {regressions}\nfixed: {fixed}\nnew_cases: {new_cases}\nremoved: {removed}\nunchanged: {unchanged}\ngenerator: \"openpulse-testmatrix\"\n---\n\n",
        new_meta.date.format("%Y-%m-%dT%H:%M:%SZ"),
        old_meta.git_commit,
        new_meta.git_commit,
    );

    out.push_str("# Test Matrix Comparison\n\n");
    out.push_str(&format!(
        "**Previous:** `{}{dirty_old}` — {}\\\n**Current:** `{}{dirty_new}` — {}\n\n",
        old_meta.git_commit,
        old_meta.date.format("%Y-%m-%d %H:%M:%S UTC"),
        new_meta.git_commit,
        new_meta.date.format("%Y-%m-%d %H:%M:%S UTC"),
    ));
    out.push_str(&format!("**Verdict: {verdict}**"));
    if !detail_parts.is_empty() {
        out.push_str(&format!(" — {}", detail_parts.join(", ")));
    }
    out.push_str("\n\n");

    // Regressions
    let regressed: Vec<_> = diffs
        .iter()
        .filter(|d| d.status == DiffStatus::Regressed)
        .collect();
    out.push_str(&format!("## Regressions ({})\n\n", regressed.len()));
    if regressed.is_empty() {
        out.push_str("None.\n\n");
    } else {
        out.push_str("| Case ID |\n|---|\n");
        for d in &regressed {
            out.push_str(&format!("| `{}` |\n", d.id));
        }
        out.push('\n');
    }

    // Fixed
    let fixed_cases: Vec<_> = diffs
        .iter()
        .filter(|d| d.status == DiffStatus::Fixed)
        .collect();
    out.push_str(&format!("## Fixed ({})\n\n", fixed_cases.len()));
    if fixed_cases.is_empty() {
        out.push_str("None.\n\n");
    } else {
        out.push_str("| Case ID |\n|---|\n");
        for d in &fixed_cases {
            out.push_str(&format!("| `{}` |\n", d.id));
        }
        out.push('\n');
    }

    // New cases
    let new: Vec<_> = diffs
        .iter()
        .filter(|d| d.status == DiffStatus::NewCase)
        .collect();
    if !new.is_empty() {
        out.push_str(&format!("## New Cases ({})\n\n", new.len()));
        out.push_str("| Case ID | Result |\n|---|---|\n");
        for d in &new {
            let result = match d.new_passed {
                Some(true) => "✓ PASS",
                Some(false) => "✗ FAIL",
                None => "—",
            };
            out.push_str(&format!("| `{}` | {result} |\n", d.id));
        }
        out.push('\n');
    }

    // Removed cases
    let removed_cases: Vec<_> = diffs
        .iter()
        .filter(|d| d.status == DiffStatus::Removed)
        .collect();
    if !removed_cases.is_empty() {
        out.push_str(&format!("## Removed Cases ({})\n\n", removed_cases.len()));
        out.push_str("| Case ID | Previous Result |\n|---|---|\n");
        for d in &removed_cases {
            let was = match d.old_passed {
                Some(true) => "✓ PASS",
                Some(false) => "✗ FAIL",
                None => "—",
            };
            out.push_str(&format!("| `{}` | {was} |\n", d.id));
        }
        out.push('\n');
    }

    fs::write(dir.join("comparison.md"), out).expect("write comparison.md");
}
