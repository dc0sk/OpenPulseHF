---
title: "OpenPulseHF Test Matrix — Summary"
date: "2026-06-29T14:37:59Z"
git_commit: "56c73ca"
git_commit_full: "56c73ca757937bd2bf942bab52bf3df797f42be2"
git_dirty: true
workspace_version: "0.3.0"
tier: "quick"
total_cases: 562
passed: 555
failed: 7
duration_s: 86.1
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

**Run:** commit `56c73ca` ⚠ dirty — v0.3.0 — 2026-06-29 14:37:59 UTC

**555/562 cases passed** in 86.1s

## By Use Case

| Use Case | Passed | Total | Skipped | Pass Rate |
|---|---|---|---|---|
| raw_modem | 538 | 545 | 0 | 98% |
| adaptive_hpx500 | 3 | 3 | 0 | 100% |
| adaptive_hpx_hf | 3 | 3 | 0 | 100% |
| adaptive_hpx_wideband | 3 | 3 | 0 | 100% |
| adaptive_hpx_ofdm_hf | 3 | 3 | 0 | 100% |
| ardop | 1 | 1 | 0 | 100% |
| kiss | 1 | 1 | 0 | 100% |
| b2f | 3 | 3 | 0 | 100% |

## Failures

| Case ID | Note |
|---|---|
| `raw_modem/OFDM16/none/nocomp/awgn_10dB/128B` | RX error: frame encoding/decoding error: frame truncated |
| `raw_modem/OFDM16/rs/nocomp/awgn_10dB/128B` | RX error: FEC error: RS correction failed at block 0: TooManyErrors |
| `raw_modem/OFDM16/rs_il/nocomp/awgn_10dB/128B` | RX error: FEC error: RS correction failed at block 0: TooManyErrors |
| `raw_modem/OFDM52/rs/nocomp/awgn_20dB/128B` | RX error: FEC error: RS correction failed at block 0: TooManyErrors |
| `raw_modem/OFDM52/rs/nocomp/clean/128B` | RX error: FEC error: RS correction failed at block 0: TooManyErrors |
| `raw_modem/OFDM52/rs_il/nocomp/awgn_20dB/128B` | RX error: FEC error: RS correction failed at block 0: TooManyErrors |
| `raw_modem/OFDM52/rs_il/nocomp/clean/128B` | RX error: FEC error: RS correction failed at block 0: TooManyErrors |
