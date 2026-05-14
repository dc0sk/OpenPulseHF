---
title: "Item 8 Lab Validation Report"
date: "2026-05-14T16:46:03Z"
source_dataset: "docs/test-reports/item8-lab/latest/item8_sessions.json"
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

- PASS sessions_per_profile = 10 (>=10)
- PASS total_frames = 120 (>=100)

## Predicted vs Observed

| Profile | Target FER | Target p95 latency | Observed FER | Observed median p95 latency | Observed mean throughput | Outcome |
|---|---:|---:|---:|---:|---:|---|
| field_relay | <= 10% | <= 2000 ms | 0.000 | 16480 ms | 108.3 bps | FER pass, latency fail |
| emergency | <= 5% | <= 2500 ms | 0.000 | 16480 ms | 108.3 bps | FER pass, latency fail |
| station_relay | <= 3% | <= 1500 ms | 0.000 | 684 ms | 2608.2 bps | pass |

## Interpretation

- Reliability targets (FER) are met in this lab run for all three profiles.
- Latency targets are not met for field_relay and emergency profiles because both use robust BPSK+RS-IL style settings with long frame durations at 223-byte payload.
- station_relay profile meets both reliability and latency targets under high-SNR AWGN.

## Recommendations

1. For field_relay and emergency latency closure, reduce payload per frame and/or evaluate QPSK500 with tuned FEC and interleaver depth.
2. Keep station_relay baseline as high-throughput profile reference for Item 8.
3. Run a follow-up sweep with profile-specific payload sizes to close p95 latency without sacrificing FER.

## Artifacts

- Dataset markdown: docs/test-reports/item8-lab/latest/item8_sessions.md
- Dataset CSV: docs/test-reports/item8-lab/latest/item8_sessions.csv
- Dataset JSON: docs/test-reports/item8-lab/latest/item8_sessions.json
