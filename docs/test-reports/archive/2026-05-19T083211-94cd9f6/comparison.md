---
title: "OpenPulseHF Test Matrix — Comparison"
date: "2026-05-19T08:32:11Z"
old_commit: "9d14cef"
new_commit: "94cd9f6 (dirty)"
regressions: 0
fixed: 0
new_cases: 60
removed: 0
unchanged: 383
generator: "openpulse-testmatrix"
---

# Test Matrix Comparison

**Previous:** `9d14cef` — 2026-05-19 07:31:41 UTC\
**Current:** `94cd9f6 (dirty)` — 2026-05-19 08:32:11 UTC

**Verdict: ✓ No regressions** — 60 new cases

## Regressions (0)

None.

## Fixed (0)

None.

## New Cases (60)

| Case ID | Result |
|---|---|
| `raw_modem/QPSK2000-RRC/none/lz4/clean/128B` | ✓ PASS |
| `raw_modem/QPSK2000-RRC/none/nocomp/awgn_10dB/128B` | ✓ PASS |
| `raw_modem/QPSK2000-RRC/none/nocomp/awgn_10dB/223B` | ✓ PASS |
| `raw_modem/QPSK2000-RRC/none/nocomp/awgn_20dB/128B` | ✓ PASS |
| `raw_modem/QPSK2000-RRC/none/nocomp/awgn_20dB/223B` | ✓ PASS |
| `raw_modem/QPSK2000-RRC/none/nocomp/clean/128B` | ✓ PASS |
| `raw_modem/QPSK2000-RRC/none/nocomp/clean/223B` | ✓ PASS |
| `raw_modem/QPSK2000-RRC/none/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/QPSK2000-RRC/none/zstd/clean/128B` | ✓ PASS |
| `raw_modem/QPSK2000-RRC/rs/lz4/clean/128B` | ✓ PASS |
| `raw_modem/QPSK2000-RRC/rs/nocomp/awgn_10dB/128B` | ✓ PASS |
| `raw_modem/QPSK2000-RRC/rs/nocomp/awgn_10dB/223B` | ✓ PASS |
| `raw_modem/QPSK2000-RRC/rs/nocomp/awgn_20dB/128B` | ✓ PASS |
| `raw_modem/QPSK2000-RRC/rs/nocomp/awgn_20dB/223B` | ✓ PASS |
| `raw_modem/QPSK2000-RRC/rs/nocomp/clean/128B` | ✓ PASS |
| `raw_modem/QPSK2000-RRC/rs/nocomp/clean/223B` | ✓ PASS |
| `raw_modem/QPSK2000-RRC/rs/zstd/clean/128B` | ✓ PASS |
| `raw_modem/QPSK2000-RRC/rs_il/nocomp/awgn_10dB/128B` | ✓ PASS |
| `raw_modem/QPSK2000-RRC/rs_il/nocomp/awgn_10dB/223B` | ✓ PASS |
| `raw_modem/QPSK2000-RRC/rs_il/nocomp/awgn_20dB/128B` | ✓ PASS |
| `raw_modem/QPSK2000-RRC/rs_il/nocomp/awgn_20dB/223B` | ✓ PASS |
| `raw_modem/QPSK2000-RRC/rs_il/nocomp/clean/128B` | ✓ PASS |
| `raw_modem/QPSK2000-RRC/rs_il/nocomp/clean/223B` | ✓ PASS |
| `raw_modem/QPSK2000/none/lz4/clean/128B` | ✗ FAIL |
| `raw_modem/QPSK2000/none/nocomp/awgn_10dB/128B` | ✗ FAIL |
| `raw_modem/QPSK2000/none/nocomp/awgn_10dB/223B` | ✗ FAIL |
| `raw_modem/QPSK2000/none/nocomp/awgn_20dB/128B` | ✗ FAIL |
| `raw_modem/QPSK2000/none/nocomp/awgn_20dB/223B` | ✗ FAIL |
| `raw_modem/QPSK2000/none/nocomp/clean/128B` | ✗ FAIL |
| `raw_modem/QPSK2000/none/nocomp/clean/223B` | ✗ FAIL |
| `raw_modem/QPSK2000/none/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/QPSK2000/none/zstd/clean/128B` | ✗ FAIL |
| `raw_modem/QPSK2000/rs/lz4/clean/128B` | ✓ PASS |
| `raw_modem/QPSK2000/rs/nocomp/awgn_10dB/128B` | ✗ FAIL |
| `raw_modem/QPSK2000/rs/nocomp/awgn_10dB/223B` | ✗ FAIL |
| `raw_modem/QPSK2000/rs/nocomp/awgn_20dB/128B` | ✗ FAIL |
| `raw_modem/QPSK2000/rs/nocomp/awgn_20dB/223B` | ✗ FAIL |
| `raw_modem/QPSK2000/rs/nocomp/clean/128B` | ✓ PASS |
| `raw_modem/QPSK2000/rs/nocomp/clean/223B` | ✗ FAIL |
| `raw_modem/QPSK2000/rs/zstd/clean/128B` | ✓ PASS |
| `raw_modem/QPSK2000/rs_il/nocomp/awgn_10dB/128B` | ✗ FAIL |
| `raw_modem/QPSK2000/rs_il/nocomp/awgn_10dB/223B` | ✗ FAIL |
| `raw_modem/QPSK2000/rs_il/nocomp/awgn_20dB/128B` | ✓ PASS |
| `raw_modem/QPSK2000/rs_il/nocomp/awgn_20dB/223B` | ✓ PASS |
| `raw_modem/QPSK2000/rs_il/nocomp/clean/128B` | ✓ PASS |
| `raw_modem/QPSK2000/rs_il/nocomp/clean/223B` | ✓ PASS |
| `raw_modem/SCFDMA52-16QAM/none/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/SCFDMA52-16QAM/none/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/SCFDMA52-16QAM/rs/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/SCFDMA52-16QAM/rs/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/SCFDMA52-32QAM/none/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/SCFDMA52-32QAM/rs/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/SCFDMA52-64QAM-P4/none/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/SCFDMA52-64QAM-P4/rs/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/SCFDMA52-64QAM/none/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/SCFDMA52-64QAM/rs/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/SCFDMA52-8PSK/none/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/SCFDMA52-8PSK/none/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/SCFDMA52-8PSK/rs/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/SCFDMA52-8PSK/rs/nocomp/clean/32B` | ✓ PASS |

