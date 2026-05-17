---
title: "OpenPulseHF Test Matrix — By Channel"
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

# Results by Channel

**Run:** commit `ce09bd9` ⚠ dirty — v0.1.0 — 2026-05-12 19:20:01 UTC

| Channel | 64QAM1000 | 64QAM2000-RRC | 64QAM500 | 8PSK1000-HF | 8PSK1000-RRC | 8PSK500 | 8PSK500-RRC | BPSK100 | BPSK250 | BPSK250-RRC | BPSK31 | BPSK63 | FSK4-ACK | HPX500 | HPX_HF | HPX_WIDEBAND | OFDM16 | OFDM52 | QPSK1000-HF | QPSK1000-RRC | QPSK125 | QPSK250 | QPSK500 | QPSK500-RRC | SCFDMA16 | SCFDMA52 | Total |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| **awgn_10dB** | — | — | — | — | ✓ 6/6 | — | ✓ 6/6 | — | ✓ 10/10 | ✓ 6/6 | — | — | — | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 2/2 | — | ✓ 6/6 | ✓ 6/6 | ✓ 6/6 | ✓ 6/6 | ✓ 9/9 | ✓ 6/6 | ✓ 2/2 | — | **74/74** |
| **awgn_20dB** | ✓ 6/6 | ✗ 4/6 | ✓ 6/6 | ✓ 6/6 | ✓ 6/6 | ✓ 9/9 | ✓ 6/6 | — | ✓ 10/10 | ✓ 6/6 | — | — | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 2/2 | ✓ 2/2 | ✓ 6/6 | ✓ 6/6 | ✓ 6/6 | ✓ 6/6 | ✓ 9/9 | ✓ 6/6 | ✓ 2/2 | ✓ 2/2 | **110/112** |
| **clean** | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 1/1 | ✓ 14/14 | ✓ 11/11 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 3/3 | ✓ 3/3 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 3/3 | ✓ 3/3 | **187/187** |
