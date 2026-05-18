---
title: "Item 8 Lab Validation Report"
date: "2026-05-14T16:46:03Z"
source_dataset: "docs/dev/test-reports/item8-lab/latest/item8_sessions.json"
---

# Item 8 Lab Validation Report (2026-05-14)

This report evaluates Item 8 profiles using the lab fallback session dataset.

## Dataset Scope

- Profiles: field_relay, emergency, station_relay
- Sessions per profile: 10
- Frames per session: 4
- Total sessions: 30
- Total frames: 120

Acceptance volume checks:

- PASS min_sessions_per_profile_observed = 10 (>=10)
- PASS total_frames = 120 (>=100)

## Predicted vs Observed

| Profile | Target FER | Target p95 latency | Observed FER | Observed median p95 latency | Observed mean throughput | Outcome |
|---|---:|---:|---:|---:|---:|---|
| field_relay | <= 10% | <= 2000 ms | 0.000 | 16480 ms | 108.3 bps | FER pass, latency fail |
| emergency | <= 5% | <= 2500 ms | 0.000 | 16480 ms | 108.3 bps | FER pass, latency fail |
| station_relay | <= 3% | <= 1500 ms | 0.325 | 684 ms | 1760.5 bps | FER fail, latency pass |

## Interpretation

- field_relay and emergency reliability targets (FER) are met in this lab run, but both violate latency targets because robust BPSK+RS-IL settings with 223-byte payload produce long frame durations.
- station_relay meets latency target but does not meet FER target across the full 18-30 dB sweep; lower-SNR sessions in this range show expected decode failures for SCFDMA52-64QAM-P4.

## Recommendations

1. For field_relay and emergency latency closure, reduce payload per frame and/or evaluate QPSK500 with tuned FEC/interleaver depth.
2. For station_relay FER closure, narrow operational SNR floor for 64QAM profile or add profile fallback to QPSK1000-HF below ~22 dB.
3. Run a follow-up sweep with profile-specific payload and mode fallback thresholds to close both p95 latency and FER targets.

## Artifacts

- Dataset markdown: docs/dev/test-reports/item8-lab/latest/item8_sessions.md
- Dataset CSV: docs/dev/test-reports/item8-lab/latest/item8_sessions.csv
- Dataset JSON: docs/dev/test-reports/item8-lab/latest/item8_sessions.json
