---
title: "OpenPulseHF Test Matrix — By Mode"
date: "2026-05-19T07:29:46Z"
git_commit: "9d14cef"
git_commit_full: "9d14cef5938ef631ce69fc82563796f301b04493"
git_dirty: false
workspace_version: "0.1.0"
tier: "quick"
total_cases: 383
passed: 381
failed: 2
duration_s: 111.5
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

# Results by Mode

**Run:** commit `9d14cef` — v0.1.0 — 2026-05-19 07:29:46 UTC

| Mode | awgn_10dB | awgn_20dB | clean | Total |
|---|---|---|---|---|
| **64QAM1000** | — | ✓ 6/6 | ✓ 11/11 | **17/17** |
| **64QAM2000-RRC** | — | ✗ 4/6 | ✓ 11/11 | **15/17** |
| **64QAM500** | — | ✓ 6/6 | ✓ 11/11 | **17/17** |
| **8PSK1000-HF** | — | ✓ 6/6 | ✓ 11/11 | **17/17** |
| **8PSK1000-RRC** | ✓ 6/6 | ✓ 6/6 | ✓ 11/11 | **23/23** |
| **8PSK500** | — | ✓ 9/9 | ✓ 11/11 | **20/20** |
| **8PSK500-RRC** | ✓ 6/6 | ✓ 6/6 | ✓ 11/11 | **23/23** |
| **BPSK100** | — | — | ✓ 1/1 | **1/1** |
| **BPSK250** | ✓ 10/10 | ✓ 10/10 | ✓ 14/14 | **34/34** |
| **BPSK250-RRC** | ✓ 6/6 | ✓ 6/6 | ✓ 11/11 | **23/23** |
| **BPSK31** | — | — | ✓ 1/1 | **1/1** |
| **BPSK63** | — | — | ✓ 1/1 | **1/1** |
| **FSK4-ACK** | — | ✓ 1/1 | ✓ 1/1 | **2/2** |
| **HPX500** | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | **3/3** |
| **HPX_HF** | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | **3/3** |
| **HPX_WIDEBAND** | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | **3/3** |
| **OFDM16** | ✓ 3/3 | ✓ 3/3 | ✓ 4/4 | **10/10** |
| **OFDM52** | — | ✓ 3/3 | ✓ 4/4 | **7/7** |
| **QPSK1000-HF** | ✓ 6/6 | ✓ 6/6 | ✓ 11/11 | **23/23** |
| **QPSK1000-RRC** | ✓ 6/6 | ✓ 6/6 | ✓ 11/11 | **23/23** |
| **QPSK125** | ✓ 6/6 | ✓ 6/6 | ✓ 11/11 | **23/23** |
| **QPSK250** | ✓ 6/6 | ✓ 6/6 | ✓ 11/11 | **23/23** |
| **QPSK500** | ✓ 9/9 | ✓ 9/9 | ✓ 11/11 | **29/29** |
| **QPSK500-RRC** | ✓ 6/6 | ✓ 6/6 | ✓ 11/11 | **23/23** |
| **SCFDMA16** | ✓ 3/3 | ✓ 3/3 | ✓ 4/4 | **10/10** |
| **SCFDMA52** | — | ✓ 3/3 | ✓ 4/4 | **7/7** |
