---
title: "OpenPulseHF Test Matrix — By Mode"
date: "2026-05-11T13:44:57Z"
git_commit: "658f421"
git_commit_full: "658f42176275dc66c7c1fd5fbd3316058d4225ca"
git_dirty: false
workspace_version: "0.1.0"
tier: "quick"
total_cases: 322
passed: 322
failed: 0
duration_s: 101.2
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
  - "qpsk-plugin"
  - "scfdma-plugin"
---

# Results by Mode

**Run:** commit `658f421` — v0.1.0 — 2026-05-11 13:44:57 UTC

| Mode | awgn_10dB | awgn_20dB | clean | Total |
|---|---|---|---|---|
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
| **OFDM16** | ✓ 2/2 | ✓ 2/2 | ✓ 3/3 | **7/7** |
| **OFDM52** | — | ✓ 2/2 | ✓ 3/3 | **5/5** |
| **QPSK1000-HF** | ✓ 6/6 | ✓ 6/6 | ✓ 11/11 | **23/23** |
| **QPSK1000-RRC** | ✓ 6/6 | ✓ 6/6 | ✓ 11/11 | **23/23** |
| **QPSK125** | ✓ 6/6 | ✓ 6/6 | ✓ 11/11 | **23/23** |
| **QPSK250** | ✓ 6/6 | ✓ 6/6 | ✓ 11/11 | **23/23** |
| **QPSK500** | ✓ 9/9 | ✓ 9/9 | ✓ 11/11 | **29/29** |
| **QPSK500-RRC** | ✓ 6/6 | ✓ 6/6 | ✓ 11/11 | **23/23** |
| **SCFDMA16** | ✓ 2/2 | ✓ 2/2 | ✓ 3/3 | **7/7** |
| **SCFDMA52** | — | ✓ 2/2 | ✓ 3/3 | **5/5** |
