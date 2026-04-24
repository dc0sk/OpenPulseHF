---
project: openpulsehf
doc: docs/raspberry-pi-4-5-tuning-and-benchmarks.md
status: living
last_updated: 2026-04-24
---

# Raspberry Pi 4/5 Tuning Guide and Benchmark Appendix

## Purpose

This guide provides practical tuning recommendations for OpenPulseHF on Raspberry Pi 4 and Raspberry Pi 5, plus a benchmark appendix template for repeatable reporting.

## Hardware assumptions

- Raspberry Pi 4 (4 GB or 8 GB) and Raspberry Pi 5 (4 GB or 8 GB)
- 64-bit Raspberry Pi OS
- stable power supply and adequate thermal solution
- optional USB audio interface for over-the-air usage

## Performance goals

- keep real-time encode/decode stable under sustained load
- avoid XRUN-like audio starvation conditions
- maintain deterministic benchmark runs for CI-comparable reports

## Baseline system preparation

1. Update OS and firmware.
2. Use performance CPU governor during benchmark runs.
3. Ensure thermal throttling is not active.
4. Disable non-essential background services on test images.

Suggested commands:

```sh
sudo apt-get update && sudo apt-get upgrade -y
sudo apt-get install -y cpufrequtils
echo 'GOVERNOR="performance"' | sudo tee /etc/default/cpufrequtils
sudo systemctl restart cpufrequtils
vcgencmd get_throttled
```

## Build and runtime tuning

### Build profile

- use release builds for all throughput or latency measurements
- keep profile settings consistent across compared runs

```sh
cargo build --release --workspace
```

### Audio/backend considerations

- prefer fixed sample-rate test plans (for example 8 kHz and 12 kHz classes)
- keep backend/device selection explicit in benchmark scripts
- avoid device auto-switching during benchmark runs

### Threading and scheduling

- pin benchmark workload to isolated cores when possible
- avoid mixing benchmark execution with heavy background compile jobs

Example affinity usage:

```sh
taskset -c 2,3 cargo test --workspace --no-default-features
```

## Raspberry Pi 4 recommendations

- start with conservative concurrency settings for long runs
- monitor thermal state every 30-60 seconds
- prefer lower-buffer jitter settings over aggressive throughput targets in warm ambient conditions

Validation checklist:

- no thermal throttle flags during run
- no benchmark harness validation failures
- no regression script threshold violations

## Raspberry Pi 5 recommendations

- leverage higher single-thread headroom for tighter latency targets
- still enforce thermal checks for long-duration stress scenarios
- use same scenario+seed policy as Pi 4 to keep comparisons valid

Validation checklist:

- sustained clock behavior within expected range
- deterministic scenario seeds in all scenario files
- stable pass/fail outcomes across at least 3 repeated runs

## Benchmark execution recipe

Run benchmark artifact and regression checks:

```sh
bash scripts/validate-benchmark-artifacts.sh
bash scripts/check-benchmark-regressions.sh benchmark/baselines benchmark/results/aggregate
```

Recommended per-platform run set:

1. HF500-NOM-01
2. HF500-FADE-02
3. HF500-BURST-03
4. HF2300-NOM-01
5. VHF-FM-NOM-01

## Benchmark appendix template

Use this section format in release PRs and performance reports.

### Environment metadata

- board: Raspberry Pi 4/5 model
- os_image: exact image build/version
- kernel: uname -r
- cooling: passive/active with fan profile
- power_supply: model and wattage
- audio_device: backend + hardware

### Aggregate results table template

| Platform | Scenario | Mode | Success Rate | Median Goodput (bps) | p95 Completion (ms) | Mean ARQ Efficiency |
|---|---|---|---:|---:|---:|---:|
| Pi 4 | HF500-NOM-01 | HPX500 | 0.00 | 0 | 0 | 0.00 |
| Pi 4 | HF500-FADE-02 | HPX500 | 0.00 | 0 | 0 | 0.00 |
| Pi 5 | HF500-NOM-01 | HPX500 | 0.00 | 0 | 0 | 0.00 |
| Pi 5 | HF500-FADE-02 | HPX500 | 0.00 | 0 | 0 | 0.00 |

Replace placeholder values with measured outputs from `benchmark/results/aggregate/*.json`.

### Regression verdict template

- baseline comparison script result: pass/fail
- goodput regression threshold violations: count
- p95 completion regression threshold violations: count
- operator notes: brief explanation for any deviation

## Quality gates for accepting Pi benchmark reports

- benchmark artifacts validated successfully
- baseline comparison executed with no unexpected threshold failures
- environment metadata included and complete
- at least one nominal and one adverse channel scenario reported per platform

## Open questions

- whether Pi-specific default runtime presets should be auto-selected by board detection
- whether thermal telemetry should be captured directly into benchmark result artifacts