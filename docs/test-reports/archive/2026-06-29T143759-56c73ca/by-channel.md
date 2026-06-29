---
title: "OpenPulseHF Test Matrix — By Channel"
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

# Results by Channel

**Run:** commit `56c73ca` ⚠ dirty — v0.3.0 — 2026-06-29 14:37:59 UTC

| Channel | 64QAM1000 | 64QAM2000-RRC | 64QAM500 | 8PSK1000 | 8PSK1000-HF | 8PSK1000-HF-RRC | 8PSK1000-RRC | 8PSK2000-RRC | 8PSK500 | 8PSK500-RRC | BPSK100 | BPSK250 | BPSK250-RRC | BPSK31 | BPSK63 | FSK4-ACK | HPX500 | HPX_HF | HPX_OFDM_HF | HPX_WIDEBAND | OFDM16 | OFDM52 | OFDM52-16QAM | OFDM52-32QAM | OFDM52-64QAM | OFDM52-8PSK | PILOT-16QAM1000 | PILOT-16QAM500 | PILOT-16QAM500-RRC | PILOT-32APSK500 | PILOT-8PSK500 | PILOT-QPSK500 | PILOT-QPSK500-RRC | QPSK1000 | QPSK1000-HF | QPSK1000-HF-RRC | QPSK1000-RRC | QPSK125 | QPSK2000 | QPSK2000-RRC | QPSK250 | QPSK500 | QPSK500-RRC | SCFDMA16 | SCFDMA26-16QAM | SCFDMA26-32QAM | SCFDMA26-8PSK | SCFDMA52 | SCFDMA52-16QAM | SCFDMA52-32QAM | SCFDMA52-64QAM | SCFDMA52-64QAM-P4 | SCFDMA52-8PSK | Total |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| **awgn_10dB** | — | — | — | — | — | ✓ 6/6 | ✓ 6/6 | — | — | ✓ 6/6 | — | ✓ 10/10 | ✓ 6/6 | — | — | — | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✗ 0/3 | — | — | — | — | — | — | — | — | — | — | ✓ 3/3 | ✓ 3/3 | — | ✓ 6/6 | ✓ 6/6 | ✓ 6/6 | ✓ 6/6 | — | ✓ 6/6 | ✓ 6/6 | ✓ 9/9 | ✓ 6/6 | ✓ 3/3 | — | — | — | — | — | — | — | — | — | **98/101** |
| **awgn_20dB** | ✓ 6/6 | — | ✓ 6/6 | — | ✓ 6/6 | ✓ 6/6 | ✓ 6/6 | — | ✓ 10/10 | ✓ 6/6 | — | ✓ 12/12 | ✓ 6/6 | — | — | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 3/3 | ✗ 1/3 | ✓ 3/3 | — | — | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | — | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | — | ✓ 6/6 | ✓ 6/6 | ✓ 6/6 | ✓ 6/6 | — | ✓ 6/6 | ✓ 6/6 | ✓ 11/11 | ✓ 6/6 | ✓ 3/3 | ✓ 3/3 | — | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | — | — | — | ✓ 3/3 | **168/170** |
| **clean** | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 1/1 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 1/1 | ✓ 12/12 | ✓ 11/11 | ✓ 1/1 | ✓ 16/16 | ✓ 11/11 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 4/4 | ✗ 2/4 | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | ✓ 1/1 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 1/1 | ✓ 11/11 | ✓ 11/11 | ✓ 13/13 | ✓ 11/11 | ✓ 4/4 | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | ✓ 4/4 | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | **289/291** |
