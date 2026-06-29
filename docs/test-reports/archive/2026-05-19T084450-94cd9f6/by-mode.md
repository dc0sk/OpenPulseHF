---
title: "OpenPulseHF Test Matrix — By Mode"
date: "2026-05-19T08:44:50Z"
git_commit: "94cd9f6"
git_commit_full: "94cd9f69bb1383ff8f7adaad52e16a71ab563eeb"
git_dirty: true
workspace_version: "0.1.0"
tier: "quick"
total_cases: 427
passed: 425
failed: 2
duration_s: 111.6
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

**Run:** commit `94cd9f6` ⚠ dirty — v0.1.0 — 2026-05-19 08:44:50 UTC

| Mode | awgn_10dB | awgn_20dB | clean | Total |
|---|---|---|---|---|
| **64QAM1000** | — | ✓ 6/6 | ✓ 11/11 | **17/17** |
| **64QAM2000-RRC** | — | ✗ 4/6 | ✓ 11/11 | **15/17** |
| **64QAM500** | — | ✓ 6/6 | ✓ 11/11 | **17/17** |
| **8PSK1000-HF** | — | ✓ 6/6 | ✓ 11/11 | **17/17** |
| **8PSK1000-RRC** | ✓ 6/6 | ✓ 6/6 | ✓ 11/11 | **23/23** |
| **8PSK500** | — | ✓ 10/10 | ✓ 12/12 | **22/22** |
| **8PSK500-RRC** | ✓ 6/6 | ✓ 6/6 | ✓ 11/11 | **23/23** |
| **BPSK100** | — | — | ✓ 1/1 | **1/1** |
| **BPSK250** | ✓ 10/10 | ✓ 11/11 | ✓ 15/15 | **36/36** |
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
| **QPSK2000** | — | — | ✓ 1/1 | **1/1** |
| **QPSK2000-RRC** | ✓ 6/6 | ✓ 6/6 | ✓ 11/11 | **23/23** |
| **QPSK250** | ✓ 6/6 | ✓ 6/6 | ✓ 11/11 | **23/23** |
| **QPSK500** | ✓ 9/9 | ✓ 10/10 | ✓ 12/12 | **31/31** |
| **QPSK500-RRC** | ✓ 6/6 | ✓ 6/6 | ✓ 11/11 | **23/23** |
| **SCFDMA16** | ✓ 3/3 | ✓ 3/3 | ✓ 4/4 | **10/10** |
| **SCFDMA52** | — | ✓ 3/3 | ✓ 4/4 | **7/7** |
| **SCFDMA52-16QAM** | — | ✓ 2/2 | ✓ 2/2 | **4/4** |
| **SCFDMA52-32QAM** | — | — | ✓ 2/2 | **2/2** |
| **SCFDMA52-64QAM** | — | — | ✓ 2/2 | **2/2** |
| **SCFDMA52-64QAM-P4** | — | — | ✓ 2/2 | **2/2** |
| **SCFDMA52-8PSK** | — | ✓ 2/2 | ✓ 2/2 | **4/4** |
