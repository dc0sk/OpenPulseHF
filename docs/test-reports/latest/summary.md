---
title: "OpenPulseHF Test Matrix — Summary"
date: "2026-05-06T17:09:23Z"
git_commit: "04a6864"
tier: "quick"
total_cases: 76
passed: 75
failed: 1
duration_s: 27
generator: "openpulse-testmatrix"
---

# Test Matrix Summary

**75/76 cases passed** in 27s

## By Use Case

| Use Case | Passed | Total | Pass Rate |
|---|---|---|---|
| raw_modem | 64 | 65 | 98% |
| adaptive_hpx500 | 3 | 3 | 100% |
| adaptive_hpx2300 | 3 | 3 | 100% |
| ardop | 1 | 1 | 100% |
| kiss | 1 | 1 | 100% |
| b2f | 3 | 3 | 100% |

## Failures

| Case ID | Note |
|---|---|
| `raw_modem/QPSK1000/raw/nocomp/awgn_10dB/128B` | RX error: frame encoding/decoding error: CRC mismatch (expected 0x4f3a, got 0x7bb7) |
