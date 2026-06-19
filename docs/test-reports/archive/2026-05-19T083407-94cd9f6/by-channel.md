---
title: "OpenPulseHF Test Matrix — By Channel"
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

# Results by Channel

**Run:** commit `94cd9f6` ⚠ dirty — v0.1.0 — 2026-05-19 08:34:07 UTC

| Channel | 64QAM1000 | 64QAM2000-RRC | 64QAM500 | 8PSK1000-HF | 8PSK1000-RRC | 8PSK500 | 8PSK500-RRC | BPSK100 | BPSK250 | BPSK250-RRC | BPSK31 | BPSK63 | FSK4-ACK | HPX500 | HPX_HF | HPX_WIDEBAND | OFDM16 | OFDM52 | QPSK1000-HF | QPSK1000-RRC | QPSK125 | QPSK2000 | QPSK2000-RRC | QPSK250 | QPSK500 | QPSK500-RRC | SCFDMA16 | SCFDMA52 | SCFDMA52-16QAM | SCFDMA52-32QAM | SCFDMA52-64QAM | SCFDMA52-64QAM-P4 | SCFDMA52-8PSK | Total |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| **awgn_10dB** | — | — | — | — | ✓ 6/6 | — | ✓ 6/6 | — | ✓ 10/10 | ✓ 6/6 | — | — | — | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 3/3 | — | ✓ 6/6 | ✓ 6/6 | ✓ 6/6 | ✗ 0/6 | ✓ 6/6 | ✓ 6/6 | ✓ 9/9 | ✓ 6/6 | ✓ 3/3 | — | — | — | — | — | — | **82/88** |
| **awgn_20dB** | ✓ 6/6 | ✗ 4/6 | ✓ 6/6 | ✓ 6/6 | ✓ 6/6 | ✓ 9/9 | ✓ 6/6 | — | ✓ 10/10 | ✓ 6/6 | — | — | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 3/3 | ✓ 3/3 | ✓ 6/6 | ✓ 6/6 | ✓ 6/6 | ✗ 2/6 | ✓ 6/6 | ✓ 6/6 | ✓ 9/9 | ✓ 6/6 | ✓ 3/3 | ✓ 3/3 | ✓ 2/2 | — | — | — | ✓ 2/2 | **126/132** |
| **clean** | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 1/1 | ✓ 14/14 | ✓ 11/11 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 4/4 | ✓ 4/4 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✗ 6/11 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 4/4 | ✓ 4/4 | ✓ 2/2 | ✓ 2/2 | ✓ 2/2 | ✓ 2/2 | ✓ 2/2 | **218/223** |
