---
title: "OpenPulseHF Item 8 Lab Dataset"
date: "2026-05-14T17:09:04Z"
git_commit: "feadf62"
---

# Item 8 Lab Dataset

**Run:** commit `feadf62` ⚠ dirty — v0.1.0 — 2026-05-14 17:09:04 UTC

Collected **10 sessions/profile** across **3 profiles** (30 total sessions, 120 total frames).

## Checks

- PASS min_sessions_per_profile_observed=10 (>=10 required)
- PASS total_frames=120 (>=100 required)

## Profile Summary

| Profile | Sessions | Frames | Frames OK | FER | Mean Throughput (bps) | Median p95 (ms) |
|---|---:|---:|---:|---:|---:|---:|
| field_relay | 10 | 40 | 40 | 0.000 | 108.3 | 16480 |
| emergency | 10 | 40 | 40 | 0.000 | 108.3 | 16480 |
| station_relay | 10 | 40 | 27 | 0.325 | 1760.5 | 684 |
