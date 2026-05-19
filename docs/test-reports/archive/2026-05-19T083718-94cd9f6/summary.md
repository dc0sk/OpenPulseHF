---
title: "OpenPulseHF Test Matrix — Summary"
date: "2026-05-19T08:37:18Z"
git_commit: "94cd9f6"
git_commit_full: "94cd9f69bb1383ff8f7adaad52e16a71ab563eeb"
git_dirty: true
workspace_version: "0.1.0"
tier: "quick"
total_cases: 421
passed: 419
failed: 2
duration_s: 111.0
generator: "openpulse-testmatrix"
crates_tested:
  - "bpsk-plugin"
  - "fsk4-plugin"
  - "ofdm-plugin"
  - "openpulse-ardop"
  - "openpulse-audio"
  - "openpulse-b2f"
  - "openpulse-b2f-driver"
  - "openpulse-channel"
  - "openpulse-core"
  - "openpulse-dsp"
  - "openpulse-kiss"
  - "openpulse-modem"
  - "psk8-plugin"
  - "qam64-plugin"
  - "qpsk-plugin"
  - "scfdma-plugin"
---

# Test Matrix Summary

**Run:** commit `94cd9f6` ⚠ dirty — v0.1.0 — 2026-05-19 08:37:18 UTC

**419/421 cases passed** in 111.0s

## By Use Case

| Use Case | Passed | Total | Skipped | Pass Rate |
|---|---|---|---|---|
| raw_modem | 405 | 407 | 0 | 99% |
| adaptive_hpx500 | 3 | 3 | 0 | 100% |
| adaptive_hpx_hf | 3 | 3 | 0 | 100% |
| adaptive_hpx_wideband | 3 | 3 | 0 | 100% |
| ardop | 1 | 1 | 0 | 100% |
| kiss | 1 | 1 | 0 | 100% |
| b2f | 3 | 3 | 0 | 100% |

## Failures

| Case ID | Note |
|---|---|
| `raw_modem/64QAM2000-RRC/none/nocomp/awgn_20dB/128B` | RX error: frame encoding/decoding error: CRC mismatch (expected 0x3241, got 0x7bb7) |
| `raw_modem/64QAM2000-RRC/none/nocomp/awgn_20dB/223B` | RX error: frame encoding/decoding error: CRC mismatch (expected 0xe0a5, got 0x7956) |
