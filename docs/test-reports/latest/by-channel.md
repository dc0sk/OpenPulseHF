---
title: "OpenPulseHF Test Matrix — By Channel"
date: "2026-06-29T15:12:02Z"
git_commit: "76de87e"
git_commit_full: "76de87e18f18027cf13f5879bba387b173877bf5"
git_dirty: true
workspace_version: "0.3.0"
tier: "quick"
total_cases: 555
passed: 555
failed: 0
duration_s: 85.5
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

**Run:** commit `76de87e` ⚠ dirty — v0.3.0 — 2026-06-29 15:12:02 UTC

| Channel | 64QAM1000 | 64QAM2000-RRC | 64QAM500 | 8PSK1000 | 8PSK1000-HF | 8PSK1000-HF-RRC | 8PSK1000-RRC | 8PSK2000-RRC | 8PSK500 | 8PSK500-RRC | BPSK100 | BPSK250 | BPSK250-RRC | BPSK31 | BPSK63 | FSK4-ACK | HPX500 | HPX_HF | HPX_OFDM_HF | HPX_WIDEBAND | OFDM16 | OFDM52 | OFDM52-16QAM | OFDM52-32QAM | OFDM52-64QAM | OFDM52-8PSK | PILOT-16QAM1000 | PILOT-16QAM500 | PILOT-16QAM500-RRC | PILOT-32APSK500 | PILOT-8PSK500 | PILOT-QPSK500 | PILOT-QPSK500-RRC | QPSK1000 | QPSK1000-HF | QPSK1000-HF-RRC | QPSK1000-RRC | QPSK125 | QPSK2000 | QPSK2000-RRC | QPSK250 | QPSK500 | QPSK500-RRC | SCFDMA16 | SCFDMA26-16QAM | SCFDMA26-32QAM | SCFDMA26-8PSK | SCFDMA52 | SCFDMA52-16QAM | SCFDMA52-32QAM | SCFDMA52-64QAM | SCFDMA52-64QAM-P4 | SCFDMA52-8PSK | Total |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| **awgn_10dB** | — | — | — | — | — | ✓ 6/6 | ✓ 6/6 | — | — | ✓ 6/6 | — | ✓ 10/10 | ✓ 6/6 | — | — | — | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | — | — | — | — | — | — | — | — | — | — | — | ✓ 3/3 | ✓ 3/3 | — | ✓ 6/6 | ✓ 6/6 | ✓ 6/6 | ✓ 6/6 | — | ✓ 6/6 | ✓ 6/6 | ✓ 9/9 | ✓ 6/6 | ✓ 3/3 | — | — | — | — | — | — | — | — | — | **98/98** |
| **awgn_20dB** | ✓ 6/6 | — | ✓ 6/6 | — | ✓ 6/6 | ✓ 6/6 | ✓ 6/6 | — | ✓ 10/10 | ✓ 6/6 | — | ✓ 12/12 | ✓ 6/6 | — | — | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 3/3 | ✓ 1/1 | ✓ 3/3 | — | — | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | — | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | — | ✓ 6/6 | ✓ 6/6 | ✓ 6/6 | ✓ 6/6 | — | ✓ 6/6 | ✓ 6/6 | ✓ 11/11 | ✓ 6/6 | ✓ 3/3 | ✓ 3/3 | — | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | — | — | — | ✓ 3/3 | **168/168** |
| **clean** | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 1/1 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 1/1 | ✓ 12/12 | ✓ 11/11 | ✓ 1/1 | ✓ 16/16 | ✓ 11/11 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 4/4 | ✓ 2/2 | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | ✓ 1/1 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 1/1 | ✓ 11/11 | ✓ 11/11 | ✓ 13/13 | ✓ 11/11 | ✓ 4/4 | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | ✓ 4/4 | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | ✓ 3/3 | **289/289** |
