---
project: openpulse
doc: docs/benchmark-harness.md
status: living
last_updated: 2026-04-23
---

# HPX Benchmark Harness Specification

## Purpose

This document defines the benchmark execution contract used to validate HPX performance and support parity or better claims against incumbent modems.

## Harness principles

- Reproducible: every run must declare channel model, seed, mode profile, and payload set.
- Comparable: baselines and HPX runs must use matched occupied bandwidth class.
- Auditable: raw artifacts are stored and traceable to run metadata.
- Automatable: reduced profile suite must run in CI; full suite runs in scheduled or manual jobs.

## Scenario catalog

Scenarios are identified by stable IDs.

### HF 500 Hz class

- HF500-NOM-01: moderate SNR, light fading.
- HF500-FADE-02: selective fading with periodic deep fades.
- HF500-BURST-03: burst-noise events at fixed intervals.

### HF 2300-2400 Hz class

- HF2300-NOM-01: moderate SNR, low multipath.
- HF2300-FADE-02: multipath + variable SNR over time.
- HF2300-STRESS-03: SNR step changes with timing jitter.

### VHF FM class

- VHF-FM-NOM-01: near-static link, occasional interference.
- VHF-FM-BURST-02: periodic impulse interference bursts.

## Required run inputs

Each scenario run must provide:

- scenario_id
- random_seed
- mode_under_test (for example HPX500, HPX2300, BPSK250 baseline)
- payload_profile (size distribution and total bytes)
- channel_profile_parameters
- run_duration_limit_s

## Result schema (JSON)

Each run emits one JSON result object conforming to this schema shape:

```json
{
  "schema_version": "1.0",
  "run_id": "2026-04-23T12:00:00Z-HF500-NOM-01-HPX500",
  "scenario_id": "HF500-NOM-01",
  "mode_under_test": "HPX500",
  "bandwidth_class_hz": 500,
  "random_seed": 42,
  "payload_bytes": 65536,
  "run_duration_s": 120,
  "success": true,
  "raw_throughput_bps": 1800.0,
  "goodput_bps": 1420.0,
  "completion_time_ms": 8420,
  "retransmissions": 14,
  "arq_efficiency": 0.86,
  "spectral_efficiency_bphz": 2.84,
  "time_to_first_payload_ms": 6200,
  "recovery_success_rate": 0.97,
  "profile_switches_per_min": 12.0,
  "p95_completion_time_ms": 10300,
  "trust_failures": 0,
  "signature_failures": 0
}
```

## Aggregated report schema

Per scenario family and mode, the harness emits an aggregated report containing:

- scenario_id
- mode_under_test
- run_count
- success_rate
- median_goodput_bps
- p95_completion_time_ms
- mean_arq_efficiency
- median_spectral_efficiency_bphz
- mean_time_to_first_payload_ms
- recovery_success_rate

## CI gate model

### Reduced CI suite

Run in pull requests and branch pushes when HPX code paths are touched.

Minimum reduced set:

- HF500-NOM-01
- HF2300-NOM-01
- VHF-FM-NOM-01

Gate rules:

- No scenario success_rate below 0.95.
- No regression in median_goodput_bps beyond 5% relative to current HPX baseline artifact.
- No increase in p95_completion_time_ms beyond 10% relative to baseline artifact.

### Full suite

Run on schedule and manual trigger.

Includes all scenarios and stress variants with multi-seed sweeps.

Gate rules:

- Must satisfy thresholds in docs/high-performance-mode.md.
- Regressions must block release-candidate tagging.

## Baseline management

- Baselines are versioned artifacts keyed by mode, scenario, and schema version.
- Baseline update requires explicit approval in pull request review.
- Baseline changes must include rationale and before/after summary.
- Regression checks are enforced with scripts/check-benchmark-regressions.sh.

## Artifact layout

Suggested layout under repository root:

- benchmark/scenarios/*.yaml: scenario parameter files.
- benchmark/results/raw/*.json: per-run result objects.
- benchmark/results/aggregate/*.json: aggregated reports.
- benchmark/reports/*.md: human-readable benchmark summaries.

## Initial bootstrap status

The repository includes initial scenario files under benchmark/scenarios and a validation script at scripts/validate-benchmark-artifacts.sh.

Run validation locally with:

```sh
bash scripts/validate-benchmark-artifacts.sh
```

If benchmark result JSON files are present, jq is required for schema-key checks.

## Signing and trust metrics in benchmarks

Benchmarks that exercise signed transfer must report:

- signature verification pass rate
- trust decision distribution (trusted, unknown, revoked, untrusted)
- policy override count for unknown signers
- cryptographic verification latency contribution

## Exit criteria

Benchmark harness M1 is complete when:

- reduced suite runs in CI and enforces gate rules,
- full suite runs manually and on schedule,
- result schema is stable and consumed by report tooling,
- benchmark reports are published for review before release decisions.

## Local regression check

Run baseline comparison locally with:

```sh
bash scripts/check-benchmark-regressions.sh benchmark/baselines benchmark/results/aggregate
```

Set strict baseline matching with:

```sh
REQUIRE_BASELINE=1 bash scripts/check-benchmark-regressions.sh benchmark/baselines benchmark/results/aggregate
```
