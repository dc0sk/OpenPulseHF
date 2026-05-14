---
title: "OpenPulseHF Test Matrix — Summary"
date: "2026-05-12T19:20:01Z"
git_commit: "ce09bd9"
git_commit_full: "ce09bd9aebd4786e0c5faa9b6b05180664d19b0d"
git_dirty: true
workspace_version: "0.1.0"
tier: "quick"
total_cases: 373
passed: 371
failed: 2
duration_s: 21.3
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

**Run:** commit `ce09bd9` ⚠ dirty — v0.1.0 — 2026-05-12 19:20:01 UTC

**371/373 cases passed** in 21.3s

## By Use Case

| Use Case | Passed | Total | Skipped | Pass Rate |
|---|---|---|---|---|
| raw_modem | 357 | 359 | 0 | 99% |
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
