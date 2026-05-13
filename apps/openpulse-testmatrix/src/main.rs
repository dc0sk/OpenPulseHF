mod bench;
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
    "qam64-plugin",
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

    /// Run the multi-frame throughput benchmark after the test matrix.
    #[arg(long)]
    bench: bool,

    /// Run only the throughput benchmark (skip the test matrix pass).
    #[arg(long)]
    bench_only: bool,

    /// Run focused BL-TP-7 pilot-density sweep (SCFDMA52-64QAM vs SCFDMA52-64QAM-P4).
    #[arg(long)]
    pilot_density_sweep: bool,

    /// Run only the BL-TP-7 pilot-density sweep (skip the test matrix pass).
    #[arg(long)]
    pilot_density_sweep_only: bool,

    /// Restrict pilot-density sweep to crossover points (AWGN 22/24, Watterson 20/22/24).
    #[arg(long)]
    pilot_density_crossover: bool,

    /// Enforce BL-TP-7 crossover regression gate (requires --pilot-density-crossover).
    #[arg(long)]
    pilot_density_gate: bool,

    /// Number of frames per benchmark combination.
    #[arg(long, default_value = "50")]
    bench_frames: usize,

    /// Payload size in bytes for the benchmark (max 223 — RS(255,223) block limit).
    #[arg(long, default_value = "128")]
    bench_payload: usize,
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

    if cli.bench_only && !cli.bench {
        eprintln!("--bench-only requires --bench");
        std::process::exit(2);
    }
    if cli.pilot_density_sweep_only && !cli.pilot_density_sweep {
        eprintln!("--pilot-density-sweep-only requires --pilot-density-sweep");
        std::process::exit(2);
    }
    if cli.pilot_density_crossover && !cli.pilot_density_sweep {
        eprintln!("--pilot-density-crossover requires --pilot-density-sweep");
        std::process::exit(2);
    }
    if cli.pilot_density_gate && !cli.pilot_density_crossover {
        eprintln!("--pilot-density-gate requires --pilot-density-crossover");
        std::process::exit(2);
    }

    let tier = if cli.full { Tier::Full } else { Tier::Quick };

    let mut failed = 0usize;
    let elapsed = if cli.bench_only || cli.pilot_density_sweep_only {
        std::time::Duration::from_secs(0)
    } else {
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
        failed = total - passed;

        println!(
            "\n{passed}/{total} passed, {failed} failed in {:.1}s",
            elapsed.as_secs_f64()
        );

        write_reports(
            &results,
            &cli.output,
            &RunMeta {
                date: Utc::now(),
                git_commit: git_short(),
                git_commit_full: git_full(),
                git_dirty: git_dirty(),
                workspace_version: env!("CARGO_PKG_VERSION").to_string(),
                tier: format!("{:?}", tier).to_lowercase(),
                duration_secs: elapsed.as_secs_f64(),
                crates_tested: CRATES_TESTED.iter().map(|s| s.to_string()).collect(),
            },
        );
        println!("Reports written to {}/latest/", cli.output.display());
        elapsed
    };

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

    if cli.bench {
        let bench_cases = bench::build_bench_cases(cli.bench_payload, tier);
        let bench_total = bench_cases.len();
        println!(
            "\nRunning throughput benchmark: {} combinations × {} frames ({}-byte payload)",
            bench_total, cli.bench_frames, cli.bench_payload,
        );
        let bench_start = Instant::now();
        let bench_results: Vec<_> = bench_cases
            .iter()
            .enumerate()
            .map(|(i, case)| {
                let r = bench::run_bench(case, cli.bench_frames);
                println!(
                    "[{:3}/{bench_total}] {} | {} | {} | {} | {}/{} ok | {:.0} bps",
                    i + 1,
                    r.mode,
                    r.channel,
                    r.fec,
                    r.compression,
                    r.frames_ok,
                    r.n_frames,
                    r.measured_bps,
                );
                r
            })
            .collect();
        let bench_elapsed = bench_start.elapsed().as_secs_f64();
        let bench_dir = cli.output.join("latest");
        bench::write_bench_report(
            &bench_results,
            &bench_dir,
            &meta,
            cli.bench_frames,
            cli.bench_payload,
            bench_elapsed,
        );
        println!(
            "Throughput benchmark written to {}/latest/throughput.{{md,csv,json}}",
            cli.output.display(),
        );
    }

    if cli.pilot_density_sweep {
        let profile = if cli.pilot_density_crossover {
            bench::PilotDensitySweepProfile::Crossover
        } else {
            bench::PilotDensitySweepProfile::Full
        };
        let sweep_cases = bench::build_pilot_density_sweep_cases(cli.bench_payload, tier, profile);
        let sweep_total = sweep_cases.len();
        let profile_label = if cli.pilot_density_crossover {
            "crossover"
        } else {
            "full"
        };
        println!(
            "\nRunning BL-TP-7 pilot-density sweep ({profile_label}): {} combinations × {} frames ({}-byte payload)",
            sweep_total,
            cli.bench_frames,
            cli.bench_payload,
        );
        let sweep_start = Instant::now();
        let sweep_results: Vec<_> = sweep_cases
            .iter()
            .enumerate()
            .map(|(i, case)| {
                let r = bench::run_bench(case, cli.bench_frames);
                println!(
                    "[{:3}/{sweep_total}] {} | {} | {} | {}/{} ok | {:.0} bps",
                    i + 1,
                    r.mode,
                    r.channel,
                    r.fec,
                    r.frames_ok,
                    r.n_frames,
                    r.measured_bps,
                );
                r
            })
            .collect();
        let sweep_elapsed = sweep_start.elapsed().as_secs_f64();
        let sweep_dir = cli.output.join("latest");
        bench::write_pilot_density_report(
            &sweep_results,
            &sweep_dir,
            &meta,
            cli.bench_frames,
            cli.bench_payload,
            sweep_elapsed,
        );
        println!(
            "Pilot-density sweep written to {}/latest/pilot_density*.{{md,csv}}",
            cli.output.display(),
        );

        if cli.pilot_density_gate {
            let gate = bench::evaluate_pilot_density_crossover_gate(&sweep_results);
            for line in &gate.checks {
                println!("[pilot-density-gate] {line}");
            }
            if !gate.passed {
                eprintln!("BL-TP-7 pilot-density crossover gate failed");
                std::process::exit(1);
            }
            println!("BL-TP-7 pilot-density crossover gate passed");
        }
    }

    if failed > 0 {
        std::process::exit(1);
    }
}
