---
project: openpulsehf
doc: docs/benchmark-harness.md
status: living
last_updated: 2026-05-01
---

# HPX Benchmark Harness Specification

## Purpose

This document defines the benchmark execution contract used to validate HPX performance and support parity or better claims against incumbent modems.

## Harness principles

- Reproducible: every run must declare channel model, seed, mode profile, and payload set.
- Comparable: baselines and HPX runs must use matched occupied bandwidth class.
- Auditable: raw artifacts are stored and traceable to run metadata.
- Automatable: reduced profile suite must run in CI; full suite runs in scheduled or manual jobs.

## Channel model specifications

All HF scenarios must use parameterised channel models drawn from standardised references. AWGN-only scenarios are not sufficient for HF performance claims.

### Watterson channel model (ITU-R F.1487 / CCIR 520-2)

The Watterson model is the standard academic HF channel model defined in ITU-R recommendation F.1487 (and the earlier CCIR 520-2). It characterises an HF path as the sum of two ionospheric paths (a two-ray model) with specified Doppler spread and time delay between paths.

Parameters for each path:

| Parameter | Description | Unit |
|-----------|-------------|------|
| `doppler_spread_hz` | One-sided Gaussian Doppler spread per ray | Hz |
| `delay_spread_ms` | Differential delay between the two rays | ms |
| `snr_db` | Signal-to-noise ratio at receiver input | dB |

Standardised path conditions:

| Condition | Doppler spread | Delay spread | Typical SNR range | Notes |
|-----------|---------------|-------------|-------------------|-------|
| AWGN | 0 Hz | 0 ms | 0–30 dB | Baseline; not representative of HF |
| Good (F1) | 0.1 Hz | 0.5 ms | 15–30 dB | Quiet short-path; best HF conditions |
| Good (F2) | 0.5 Hz | 1.0 ms | 10–25 dB | Good medium-path |
| Moderate (M1) | 1.0 Hz | 1.0 ms | 5–20 dB | Typical daytime mid-latitude path |
| Moderate (M2) | 1.0 Hz | 2.0 ms | 5–15 dB | Moderate long-path or disturbed |
| Poor (P1) | 1.0 Hz | 2.0 ms | 0–10 dB | Challenging path; deep fades |
| Poor (P2) | 2.0 Hz | 4.0 ms | −5–5 dB | Severe multipath; near the SNR floor |

Scenarios must state which Watterson condition they implement, with explicit parameter values, rather than vague descriptors such as "light fading."

### Gilbert-Elliott burst error model

The Gilbert-Elliott model is a two-state Markov model for burst error characterisation on fading channels. It is the standard model for evaluating FEC + interleaver performance.

States:
- **Good state (G):** low bit error probability `p_g` (typically ≈ 0.001).
- **Bad state (B):** high bit error probability `p_b` (typically ≈ 0.1–0.5).

Transition parameters:
- `p_gb`: probability of transitioning from Good to Bad per symbol.
- `p_bg`: probability of transitioning from Bad to Good per symbol.

Mean burst length in the Bad state: `1 / p_bg`.
Mean time between bursts: `1 / p_gb`.

Representative parameter sets for HF:

| Profile | `p_g` | `p_b` | `p_gb` | `p_bg` | Mean burst (symbols) | Mean gap (symbols) |
|---------|-------|-------|--------|--------|---------------------|-------------------|
| Light burst | 0.001 | 0.1 | 0.01 | 0.1 | 10 | 100 |
| Moderate burst | 0.001 | 0.2 | 0.05 | 0.05 | 20 | 20 |
| Heavy burst | 0.001 | 0.5 | 0.05 | 0.02 | 50 | 20 |
| Severe burst | 0.001 | 0.8 | 0.1 | 0.01 | 100 | 10 |

Burst scenarios must be parameterised using the Gilbert-Elliott model with explicit parameter values. The light and moderate profiles are the minimum required for CI; heavy and severe are for full-suite scheduled runs.

### FEC and interleaver validation requirement

Any scenario that exercises FEC must also include a burst-error variant (Gilbert-Elliott) at a burst duration that tests the interleaver boundary. An interleaver of depth D at baud rate B covers D/B seconds of burst. Scenarios should include at least one burst duration at D/B and one at 2×D/B to demonstrate both the protected and unprotected regimes.

## Scenario catalog

Scenarios are identified by stable IDs. Each scenario must reference a channel model by name with explicit parameter values.

### HF 500 Hz class

- HF500-AWGN-00: AWGN baseline, SNR sweep 0–20 dB. Channel: AWGN. Purpose: calibration and sanity check only; not used for HF performance claims.
- HF500-NOM-01: Watterson Moderate M1 (Doppler 1.0 Hz, delay 1.0 ms), SNR 12 dB. Purpose: nominal HF 500 Hz operation.
- HF500-FADE-02: Watterson Poor P1 (Doppler 1.0 Hz, delay 2.0 ms), SNR 5 dB with ±3 dB random variation. Purpose: selective fading with periodic deep fades.
- HF500-BURST-03: Gilbert-Elliott moderate burst (p_gb=0.05, p_bg=0.05, p_b=0.2) on top of Watterson M1. Purpose: burst-noise stress for FEC+interleaver validation.
- HF500-BURST-04: Gilbert-Elliott heavy burst (p_gb=0.05, p_bg=0.02, p_b=0.5) on top of Watterson M1. Purpose: full-suite; stress FEC beyond interleaver depth.

### HF 2300-2400 Hz class

- HF2300-AWGN-00: AWGN baseline, SNR sweep 0–25 dB. Calibration only.
- HF2300-NOM-01: Watterson Good F2 (Doppler 0.5 Hz, delay 1.0 ms), SNR 18 dB. Purpose: nominal wide-band HF operation.
- HF2300-FADE-02: Watterson Moderate M2 (Doppler 1.0 Hz, delay 2.0 ms), SNR 10 dB with ±4 dB random variation. Purpose: multipath + variable SNR.
- HF2300-STRESS-03: Watterson Poor P2 (Doppler 2.0 Hz, delay 4.0 ms), SNR 3 dB with step changes every 30 s. Purpose: near-floor stress with timing jitter.

### VHF FM class

- VHF-FM-NOM-01: AWGN, SNR 25 dB, near-static link. Purpose: FM baseline.
- VHF-FM-BURST-02: Gilbert-Elliott light burst (p_gb=0.01, p_bg=0.1, p_b=0.1) on top of AWGN, SNR 20 dB. Purpose: periodic impulse interference bursts typical of VHF FM.

## Required run inputs

Each scenario run must provide:

- scenario_id
- random_seed
- mode_under_test (for example HPX500, HPX2300, BPSK250 baseline)
- payload_profile (size distribution and total bytes)
- channel_model (one of: awgn, watterson, gilbert-elliott, watterson+gilbert-elliott)
- channel_profile_parameters (full parameter set matching the channel model above)
- run_duration_limit_s

## Scenario seed policy

Each scenario YAML in `benchmark/scenarios/` must include deterministic seed metadata:

- `seed_policy`: one of `fixed` or `sweep`
- `random_seed`: integer seed used by the harness when `seed_policy: fixed`

Current repository policy:

- CI reduced suite scenarios use `seed_policy: fixed` for strict reproducibility.
- Multi-seed experiments are allowed in scheduled/manual runs using `seed_policy: sweep` with external seed lists.

Validation:

- `scripts/validate-benchmark-artifacts.sh` enforces presence and basic validity of `seed_policy` and `random_seed` in every scenario file.

## Result schema (JSON)

Each run emits one JSON result object conforming to this schema shape:

- `benchmark/schema/raw-result.schema.json`

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

- `benchmark/schema/aggregate-result.schema.json`

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

If benchmark result JSON files are present, jq is required for schema validation against:

- `benchmark/schema/raw-result.schema.json`
- `benchmark/schema/aggregate-result.schema.json`

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
