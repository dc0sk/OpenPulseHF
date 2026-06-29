---
title: "OpenPulseHF Test Matrix — Comparison"
date: "2026-06-29T14:37:59Z"
old_commit: "94cd9f6 (dirty)"
new_commit: "56c73ca (dirty)"
regressions: 7
fixed: 0
new_cases: 141
removed: 6
unchanged: 414
generator: "openpulse-testmatrix"
---

# Test Matrix Comparison

**Previous:** `94cd9f6 (dirty)` — 2026-05-19 08:44:50 UTC\
**Current:** `56c73ca (dirty)` — 2026-06-29 14:37:59 UTC

**Verdict: ✗ **7 regression(s) detected**** — 141 new cases, 6 removed

## Regressions (7)

| Case ID |
|---|
| `raw_modem/OFDM16/none/nocomp/awgn_10dB/128B` |
| `raw_modem/OFDM16/rs/nocomp/awgn_10dB/128B` |
| `raw_modem/OFDM16/rs_il/nocomp/awgn_10dB/128B` |
| `raw_modem/OFDM52/rs/nocomp/awgn_20dB/128B` |
| `raw_modem/OFDM52/rs/nocomp/clean/128B` |
| `raw_modem/OFDM52/rs_il/nocomp/awgn_20dB/128B` |
| `raw_modem/OFDM52/rs_il/nocomp/clean/128B` |

## Fixed (0)

None.

## New Cases (141)

| Case ID | Result |
|---|---|
| `adaptive_hpx_ofdm_hf/HPX_OFDM_HF/none/nocomp/awgn_10dB/64B` | ✓ PASS |
| `adaptive_hpx_ofdm_hf/HPX_OFDM_HF/none/nocomp/awgn_20dB/64B` | ✓ PASS |
| `adaptive_hpx_ofdm_hf/HPX_OFDM_HF/none/nocomp/clean/64B` | ✓ PASS |
| `raw_modem/8PSK1000-HF-RRC/none/lz4/clean/128B` | ✓ PASS |
| `raw_modem/8PSK1000-HF-RRC/none/nocomp/awgn_10dB/128B` | ✓ PASS |
| `raw_modem/8PSK1000-HF-RRC/none/nocomp/awgn_10dB/223B` | ✓ PASS |
| `raw_modem/8PSK1000-HF-RRC/none/nocomp/awgn_20dB/128B` | ✓ PASS |
| `raw_modem/8PSK1000-HF-RRC/none/nocomp/awgn_20dB/223B` | ✓ PASS |
| `raw_modem/8PSK1000-HF-RRC/none/nocomp/clean/128B` | ✓ PASS |
| `raw_modem/8PSK1000-HF-RRC/none/nocomp/clean/223B` | ✓ PASS |
| `raw_modem/8PSK1000-HF-RRC/none/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/8PSK1000-HF-RRC/none/zstd/clean/128B` | ✓ PASS |
| `raw_modem/8PSK1000-HF-RRC/rs/lz4/clean/128B` | ✓ PASS |
| `raw_modem/8PSK1000-HF-RRC/rs/nocomp/awgn_10dB/128B` | ✓ PASS |
| `raw_modem/8PSK1000-HF-RRC/rs/nocomp/awgn_10dB/223B` | ✓ PASS |
| `raw_modem/8PSK1000-HF-RRC/rs/nocomp/awgn_20dB/128B` | ✓ PASS |
| `raw_modem/8PSK1000-HF-RRC/rs/nocomp/awgn_20dB/223B` | ✓ PASS |
| `raw_modem/8PSK1000-HF-RRC/rs/nocomp/clean/128B` | ✓ PASS |
| `raw_modem/8PSK1000-HF-RRC/rs/nocomp/clean/223B` | ✓ PASS |
| `raw_modem/8PSK1000-HF-RRC/rs/zstd/clean/128B` | ✓ PASS |
| `raw_modem/8PSK1000-HF-RRC/rs_il/nocomp/awgn_10dB/128B` | ✓ PASS |
| `raw_modem/8PSK1000-HF-RRC/rs_il/nocomp/awgn_10dB/223B` | ✓ PASS |
| `raw_modem/8PSK1000-HF-RRC/rs_il/nocomp/awgn_20dB/128B` | ✓ PASS |
| `raw_modem/8PSK1000-HF-RRC/rs_il/nocomp/awgn_20dB/223B` | ✓ PASS |
| `raw_modem/8PSK1000-HF-RRC/rs_il/nocomp/clean/128B` | ✓ PASS |
| `raw_modem/8PSK1000-HF-RRC/rs_il/nocomp/clean/223B` | ✓ PASS |
| `raw_modem/8PSK1000/none/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/8PSK2000-RRC/none/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/BPSK250/turbo/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/BPSK250/turbo/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/OFDM52-16QAM/none/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/OFDM52-16QAM/none/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/OFDM52-16QAM/rs/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/OFDM52-16QAM/rs/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/OFDM52-16QAM/soft_concat/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/OFDM52-16QAM/soft_concat/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/OFDM52-32QAM/none/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/OFDM52-32QAM/rs/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/OFDM52-32QAM/soft_concat/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/OFDM52-64QAM/none/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/OFDM52-64QAM/rs/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/OFDM52-64QAM/soft_concat/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/OFDM52-8PSK/none/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/OFDM52-8PSK/none/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/OFDM52-8PSK/rs/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/OFDM52-8PSK/rs/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/OFDM52-8PSK/soft_concat/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/OFDM52-8PSK/soft_concat/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/PILOT-16QAM1000/ldpc/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/PILOT-16QAM1000/ldpc/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/PILOT-16QAM1000/none/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/PILOT-16QAM1000/none/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/PILOT-16QAM1000/rs/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/PILOT-16QAM1000/rs/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/PILOT-16QAM500-RRC/ldpc/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/PILOT-16QAM500-RRC/ldpc/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/PILOT-16QAM500-RRC/none/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/PILOT-16QAM500-RRC/none/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/PILOT-16QAM500-RRC/rs/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/PILOT-16QAM500-RRC/rs/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/PILOT-16QAM500/ldpc/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/PILOT-16QAM500/ldpc/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/PILOT-16QAM500/none/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/PILOT-16QAM500/none/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/PILOT-16QAM500/rs/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/PILOT-16QAM500/rs/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/PILOT-32APSK500/ldpc/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/PILOT-32APSK500/none/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/PILOT-32APSK500/rs/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/PILOT-8PSK500/ldpc/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/PILOT-8PSK500/ldpc/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/PILOT-8PSK500/none/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/PILOT-8PSK500/none/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/PILOT-8PSK500/rs/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/PILOT-8PSK500/rs/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/PILOT-QPSK500-RRC/ldpc/nocomp/awgn_10dB/32B` | ✓ PASS |
| `raw_modem/PILOT-QPSK500-RRC/ldpc/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/PILOT-QPSK500-RRC/ldpc/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/PILOT-QPSK500-RRC/none/nocomp/awgn_10dB/32B` | ✓ PASS |
| `raw_modem/PILOT-QPSK500-RRC/none/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/PILOT-QPSK500-RRC/none/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/PILOT-QPSK500-RRC/rs/nocomp/awgn_10dB/32B` | ✓ PASS |
| `raw_modem/PILOT-QPSK500-RRC/rs/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/PILOT-QPSK500-RRC/rs/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/PILOT-QPSK500/ldpc/nocomp/awgn_10dB/32B` | ✓ PASS |
| `raw_modem/PILOT-QPSK500/ldpc/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/PILOT-QPSK500/ldpc/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/PILOT-QPSK500/none/nocomp/awgn_10dB/32B` | ✓ PASS |
| `raw_modem/PILOT-QPSK500/none/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/PILOT-QPSK500/none/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/PILOT-QPSK500/rs/nocomp/awgn_10dB/32B` | ✓ PASS |
| `raw_modem/PILOT-QPSK500/rs/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/PILOT-QPSK500/rs/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/QPSK1000-HF-RRC/none/lz4/clean/128B` | ✓ PASS |
| `raw_modem/QPSK1000-HF-RRC/none/nocomp/awgn_10dB/128B` | ✓ PASS |
| `raw_modem/QPSK1000-HF-RRC/none/nocomp/awgn_10dB/223B` | ✓ PASS |
| `raw_modem/QPSK1000-HF-RRC/none/nocomp/awgn_20dB/128B` | ✓ PASS |
| `raw_modem/QPSK1000-HF-RRC/none/nocomp/awgn_20dB/223B` | ✓ PASS |
| `raw_modem/QPSK1000-HF-RRC/none/nocomp/clean/128B` | ✓ PASS |
| `raw_modem/QPSK1000-HF-RRC/none/nocomp/clean/223B` | ✓ PASS |
| `raw_modem/QPSK1000-HF-RRC/none/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/QPSK1000-HF-RRC/none/zstd/clean/128B` | ✓ PASS |
| `raw_modem/QPSK1000-HF-RRC/rs/lz4/clean/128B` | ✓ PASS |
| `raw_modem/QPSK1000-HF-RRC/rs/nocomp/awgn_10dB/128B` | ✓ PASS |
| `raw_modem/QPSK1000-HF-RRC/rs/nocomp/awgn_10dB/223B` | ✓ PASS |
| `raw_modem/QPSK1000-HF-RRC/rs/nocomp/awgn_20dB/128B` | ✓ PASS |
| `raw_modem/QPSK1000-HF-RRC/rs/nocomp/awgn_20dB/223B` | ✓ PASS |
| `raw_modem/QPSK1000-HF-RRC/rs/nocomp/clean/128B` | ✓ PASS |
| `raw_modem/QPSK1000-HF-RRC/rs/nocomp/clean/223B` | ✓ PASS |
| `raw_modem/QPSK1000-HF-RRC/rs/zstd/clean/128B` | ✓ PASS |
| `raw_modem/QPSK1000-HF-RRC/rs_il/nocomp/awgn_10dB/128B` | ✓ PASS |
| `raw_modem/QPSK1000-HF-RRC/rs_il/nocomp/awgn_10dB/223B` | ✓ PASS |
| `raw_modem/QPSK1000-HF-RRC/rs_il/nocomp/awgn_20dB/128B` | ✓ PASS |
| `raw_modem/QPSK1000-HF-RRC/rs_il/nocomp/awgn_20dB/223B` | ✓ PASS |
| `raw_modem/QPSK1000-HF-RRC/rs_il/nocomp/clean/128B` | ✓ PASS |
| `raw_modem/QPSK1000-HF-RRC/rs_il/nocomp/clean/223B` | ✓ PASS |
| `raw_modem/QPSK1000/none/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/QPSK500/turbo/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/QPSK500/turbo/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/SCFDMA26-16QAM/none/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/SCFDMA26-16QAM/none/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/SCFDMA26-16QAM/rs/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/SCFDMA26-16QAM/rs/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/SCFDMA26-16QAM/soft_concat/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/SCFDMA26-16QAM/soft_concat/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/SCFDMA26-32QAM/none/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/SCFDMA26-32QAM/rs/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/SCFDMA26-32QAM/soft_concat/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/SCFDMA26-8PSK/none/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/SCFDMA26-8PSK/none/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/SCFDMA26-8PSK/rs/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/SCFDMA26-8PSK/rs/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/SCFDMA26-8PSK/soft_concat/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/SCFDMA26-8PSK/soft_concat/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/SCFDMA52-16QAM/soft_concat/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/SCFDMA52-16QAM/soft_concat/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/SCFDMA52-32QAM/soft_concat/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/SCFDMA52-64QAM-P4/soft_concat/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/SCFDMA52-64QAM/soft_concat/nocomp/clean/32B` | ✓ PASS |
| `raw_modem/SCFDMA52-8PSK/soft_concat/nocomp/awgn_20dB/32B` | ✓ PASS |
| `raw_modem/SCFDMA52-8PSK/soft_concat/nocomp/clean/32B` | ✓ PASS |

## Removed Cases (6)

| Case ID | Previous Result |
|---|---|
| `raw_modem/64QAM2000-RRC/none/nocomp/awgn_20dB/128B` | ✗ FAIL |
| `raw_modem/64QAM2000-RRC/none/nocomp/awgn_20dB/223B` | ✗ FAIL |
| `raw_modem/64QAM2000-RRC/rs/nocomp/awgn_20dB/128B` | ✓ PASS |
| `raw_modem/64QAM2000-RRC/rs/nocomp/awgn_20dB/223B` | ✓ PASS |
| `raw_modem/64QAM2000-RRC/rs_il/nocomp/awgn_20dB/128B` | ✓ PASS |
| `raw_modem/64QAM2000-RRC/rs_il/nocomp/awgn_20dB/223B` | ✓ PASS |

