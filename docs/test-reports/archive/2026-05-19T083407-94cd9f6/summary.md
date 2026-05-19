---
title: "OpenPulseHF Test Matrix — Summary"
date: "2026-05-19T08:34:07Z"
git_commit: "94cd9f6"
git_commit_full: "94cd9f69bb1383ff8f7adaad52e16a71ab563eeb"
git_dirty: true
workspace_version: "0.1.0"
tier: "quick"
total_cases: 443
passed: 426
failed: 17
duration_s: 112.9
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

**Run:** commit `94cd9f6` ⚠ dirty — v0.1.0 — 2026-05-19 08:34:07 UTC

**426/443 cases passed** in 112.9s

## By Use Case

| Use Case | Passed | Total | Skipped | Pass Rate |
|---|---|---|---|---|
| raw_modem | 412 | 429 | 0 | 96% |
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
| `raw_modem/QPSK2000/none/lz4/clean/128B` | RX error: frame encoding/decoding error: CRC mismatch (expected 0xf3d6, got 0x7bb7) |
| `raw_modem/QPSK2000/none/nocomp/awgn_10dB/128B` | RX error: frame encoding/decoding error: invalid magic |
| `raw_modem/QPSK2000/none/nocomp/awgn_10dB/223B` | RX error: frame encoding/decoding error: invalid magic |
| `raw_modem/QPSK2000/none/nocomp/awgn_20dB/128B` | RX error: frame encoding/decoding error: CRC mismatch (expected 0xfcd3, got 0x7bb7) |
| `raw_modem/QPSK2000/none/nocomp/awgn_20dB/223B` | RX error: frame encoding/decoding error: CRC mismatch (expected 0xc081, got 0x7956) |
| `raw_modem/QPSK2000/none/nocomp/clean/128B` | RX error: frame encoding/decoding error: CRC mismatch (expected 0xf3d6, got 0x7bb7) |
| `raw_modem/QPSK2000/none/nocomp/clean/223B` | RX error: frame encoding/decoding error: CRC mismatch (expected 0x647e, got 0x7956) |
| `raw_modem/QPSK2000/none/zstd/clean/128B` | RX error: frame encoding/decoding error: CRC mismatch (expected 0xf3d6, got 0x7bb7) |
| `raw_modem/QPSK2000/rs/nocomp/awgn_10dB/128B` | RX error: FEC error: RS correction failed at block 0: TooManyErrors |
| `raw_modem/QPSK2000/rs/nocomp/awgn_10dB/223B` | RX error: FEC error: RS correction failed at block 0: TooManyErrors |
| `raw_modem/QPSK2000/rs/nocomp/awgn_20dB/128B` | RX error: FEC error: RS correction failed at block 0: TooManyErrors |
| `raw_modem/QPSK2000/rs/nocomp/awgn_20dB/223B` | RX error: FEC error: RS correction failed at block 0: TooManyErrors |
| `raw_modem/QPSK2000/rs/nocomp/clean/223B` | RX error: FEC error: RS correction failed at block 0: TooManyErrors |
| `raw_modem/QPSK2000/rs_il/nocomp/awgn_10dB/128B` | RX error: FEC error: RS correction failed at block 0: TooManyErrors |
| `raw_modem/QPSK2000/rs_il/nocomp/awgn_10dB/223B` | RX error: FEC error: RS correction failed at block 0: TooManyErrors |
