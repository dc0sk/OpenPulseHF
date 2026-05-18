---
title: "Item 8 Lab Validation Baseline"
date: "2026-05-14T00:00:00Z"
source_run: "docs/dev/test-reports/latest/raw.json"
status: "baseline-only"
---

# Item 8 Lab Validation Baseline (2026-05-14)

This report captures the current non-on-air baseline for Item 8.
It is based on the latest quick-tier matrix run and Item 6 HARQ-rate gate output.

## Data Sources

- Matrix raw results: docs/dev/test-reports/latest/raw.json
- Use-case table: docs/dev/test-reports/latest/by-usecase.md
- Item 6 aggregate: benchmark/results/aggregate/HF2300-AWGN30-ITEM6--SCFDMA52-64QAM-P4.json

## Aggregate Summary by Existing Testmatrix UseCase

| UseCase | Cases | Passed | Failed | Mean duration (ms) | Mean BER |
|---|---:|---:|---:|---:|---:|
| AdaptiveHpx500 | 3 | 3 | 0 | 5081 | 0.0000 |
| AdaptiveHpxHf | 3 | 3 | 0 | 5142 | 0.0000 |
| AdaptiveHpxWideband | 3 | 3 | 0 | 20 | 0.3333 |
| Ardop | 1 | 1 | 0 | 27 | n/a |
| B2f | 3 | 3 | 0 | 534 | n/a |
| Kiss | 1 | 1 | 0 | 27 | n/a |
| RawModem | 369 | 367 | 2 | 208 | 0.0000 |

## Throughput and Latency Baseline

From Item 6 HARQ-rate gate (SCFDMA52-64QAM-P4, AWGN 30 dB):

- Success rate: 100% (50/50)
- Throughput: 2608.2 bps
- p95 completion time: 684 ms

Interpretation:
- This is a valid high-SNR baseline for station_relay-style operation.
- It does not validate fading-heavy field_relay behavior.

## Coverage Gaps Against Item 8 Acceptance

Not yet covered in this baseline:

- No Watterson/Gilbert-Elliott field profile data in this quick-tier run.
- No on-air session logs.
- No >=10 sessions per Item 8 use-case profile (`field_relay`, `emergency`, `station_relay`).

## Next Validation Steps

1. Run targeted full-tier scenarios with Watterson F1/F2 and Gilbert-Elliott channels.
2. Collect at least 10 sessions per Item 8 profile, >=100 total frames.
3. Publish profile-mapped report with throughput vs predicted, FER, and latency per profile.
