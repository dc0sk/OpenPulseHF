---
title: "OpenPulseHF Test Matrix — By Channel"
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

# Results by Channel

**Run:** commit `658f421` — v0.1.0 — 2026-05-11 13:44:57 UTC

| Channel | 8PSK1000-HF | 8PSK1000-RRC | 8PSK500 | 8PSK500-RRC | BPSK100 | BPSK250 | BPSK250-RRC | BPSK31 | BPSK63 | FSK4-ACK | HPX500 | HPX_HF | HPX_WIDEBAND | OFDM16 | OFDM52 | QPSK1000-HF | QPSK1000-RRC | QPSK125 | QPSK250 | QPSK500 | QPSK500-RRC | SCFDMA16 | SCFDMA52 | Total |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| **awgn_10dB** | — | ✓ 6/6 | — | ✓ 6/6 | — | ✓ 10/10 | ✓ 6/6 | — | — | — | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 2/2 | — | ✓ 6/6 | ✓ 6/6 | ✓ 6/6 | ✓ 6/6 | ✓ 9/9 | ✓ 6/6 | ✓ 2/2 | — | **74/74** |
| **awgn_20dB** | ✓ 6/6 | ✓ 6/6 | ✓ 9/9 | ✓ 6/6 | — | ✓ 10/10 | ✓ 6/6 | — | — | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 2/2 | ✓ 2/2 | ✓ 6/6 | ✓ 6/6 | ✓ 6/6 | ✓ 6/6 | ✓ 9/9 | ✓ 6/6 | ✓ 2/2 | ✓ 2/2 | **94/94** |
| **clean** | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 1/1 | ✓ 14/14 | ✓ 11/11 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 1/1 | ✓ 3/3 | ✓ 3/3 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 11/11 | ✓ 3/3 | ✓ 3/3 | **154/154** |
