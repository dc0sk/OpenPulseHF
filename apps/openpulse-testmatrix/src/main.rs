mod cases;
mod channels;
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

#[derive(Parser)]
#[command(
    name = "openpulse-testmatrix",
    about = "OpenPulseHF comprehensive test matrix"
)]
struct Cli {
    /// Run the full matrix including all propagation channels and payload sizes.
    #[arg(long)]
    full: bool,

    /// Output directory for test reports.
    #[arg(long, default_value = "docs/test-reports")]
    output: PathBuf,
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
        "\n{passed}/{total} passed, {failed} failed in {}s",
        elapsed.as_secs()
    );

    let git_commit = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".into());

    let meta = RunMeta {
        date: Utc::now(),
        git_commit,
        tier: format!("{:?}", tier).to_lowercase(),
        duration: elapsed,
    };

    write_reports(&results, &cli.output, &meta);
    println!("Reports written to {}/latest/", cli.output.display());

    if failed > 0 {
        // Non-zero exit so CI can gate on failures if desired (not mandatory).
        std::process::exit(1);
    }
}
