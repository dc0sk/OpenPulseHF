mod cases;
mod channels;
mod compare;
mod matrix;
mod report;
mod runners;

use std::path::PathBuf;
use std::time::Instant;

use chrono::Utc;
use clap::Parser;

use crate::cases::build_cases;
use crate::matrix::Tier;
use crate::report::{write_reports, RunMeta};
use crate::runners::run_case;

/// Crates exercised by this test matrix.  Keep in sync with Cargo.toml [dependencies].
const CRATES_TESTED: &[&str] = &[
    "bpsk-plugin",
    "fsk4-plugin",
    "ofdm-plugin",
    "openpulse-ardop",
    "openpulse-audio",
    "openpulse-b2f",
    "openpulse-b2f-driver",
    "openpulse-channel",
    "openpulse-core",
    "openpulse-dsp",
    "openpulse-kiss",
    "openpulse-modem",
    "psk8-plugin",
    "qpsk-plugin",
    "scfdma-plugin",
];

#[derive(Parser)]
#[command(
    name = "openpulse-testmatrix",
    about = "OpenPulseHF comprehensive test matrix",
    long_about = "OpenPulseHF comprehensive test matrix.",
    author,
    version
)]
struct Cli {
    /// Run the full matrix including all propagation channels and payload sizes.
    #[arg(long)]
    full: bool,

    /// Output directory for test reports.
    #[arg(long, default_value = "docs/test-reports")]
    output: PathBuf,
}

fn git_short() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".into())
}

fn git_full() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".into())
}

/// Returns true when there are uncommitted changes (staged or unstaged).
fn git_dirty() -> bool {
    std::process::Command::new("git")
        .args(["diff", "--quiet", "HEAD"])
        .status()
        .map(|s| !s.success())
        .unwrap_or(false)
}

fn main() {
    let cli = Cli::parse();
    let tier = if cli.full { Tier::Full } else { Tier::Quick };

    let cases = build_cases(tier);
    let total = cases.len();
    println!("Running {} test cases (tier: {:?})", total, tier);

    let start = Instant::now();
    let mut results = Vec::with_capacity(total);

    for (i, case) in cases.iter().enumerate() {
        let result = run_case(case);
        let status = if result.passed { "PASS" } else { "FAIL" };
        println!(
            "[{:3}/{total}] {status} {} ({}ms)",
            i + 1,
            case.id(),
            result.duration_ms
        );
        results.push(result);
    }

    let elapsed = start.elapsed();
    let passed = results.iter().filter(|r| r.passed).count();
    let failed = total - passed;

    println!(
        "\n{passed}/{total} passed, {failed} failed in {:.1}s",
        elapsed.as_secs_f64()
    );

    let meta = RunMeta {
        date: Utc::now(),
        git_commit: git_short(),
        git_commit_full: git_full(),
        git_dirty: git_dirty(),
        workspace_version: env!("CARGO_PKG_VERSION").to_string(),
        tier: format!("{:?}", tier).to_lowercase(),
        duration_secs: elapsed.as_secs_f64(),
        crates_tested: CRATES_TESTED.iter().map(|s| s.to_string()).collect(),
    };

    write_reports(&results, &cli.output, &meta);
    println!("Reports written to {}/latest/", cli.output.display());

    if failed > 0 {
        std::process::exit(1);
    }
}
