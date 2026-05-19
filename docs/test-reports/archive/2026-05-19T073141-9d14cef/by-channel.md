---
title: "OpenPulseHF Test Matrix — By Channel"
date: "2026-05-19T07:31:41Z"
git_commit: "9d14cef"
git_commit_full: "9d14cef5938ef631ce69fc82563796f301b04493"
git_dirty: false
workspace_version: "0.1.0"
tier: "quick"
total_cases: 383
passed: 381
failed: 2
duration_s: 111.7
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

**Run:** commit `9d14cef` — v0.1.0 — 2026-05-19 07:31:41 UTC

| Channel | 64QAM1000 | 64QAM2000-RRC | 64QAM500 | 8PSK1000-HF | 8PSK1000-RRC | 8PSK500 | 8PSK500-RRC | BPSK100 | BPSK250 | BPSK250-RRC | BPSK31 | BPSK63 | FSK4-ACK | HPX500 | HPX_HF | HPX_WIDEBAND | OFDM16 | OFDM52 | QPSK1000-HF | QPSK1000-RRC | QPSK125 | QPSK250 | QPSK500 | QPSK500-RRC | SCFDMA16 | SCFDMA52 | Total |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| **awgn_10dB** | — | — | — | — | ✓ 6/6 | — | ✓ 6/6 | — | ✓ 10/10 | ✓ 6/6 | — | — | — | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 3/3 | — | ✓ 6/6 | ✓ 6/6 | ✓ 6/6 | ✓ 6/6 | ✓ 9/9 | ✓ 6/6 | ✓ 3/3 | — | **76/76** |
| **awgn_20dB** | ✓ 6/6 | ✗ 4/6 | ✓ 6/6 | ✓ 6/6 | ✓ 6/6 | ✓ 9/9 | ✓ 6/6 | — | ✓ 10/10 | ✓ 6/6 | — | — | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 3/3 | ✓ 3/3 | ✓ 6/6 | ✓ 6/6 | ✓ 6/6 | ✓ 6/6 | ✓ 9/9 | ✓ 6/6 | ✓ 3/3 | ✓ 3/3 | **114/116** |
| **clean** | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 1/1 | ✓ 14/14 | ✓ 11/11 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 4/4 | ✓ 4/4 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 4/4 | ✓ 4/4 | **191/191** |
