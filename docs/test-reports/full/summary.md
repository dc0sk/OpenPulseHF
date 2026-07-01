---
title: "OpenPulseHF Test Matrix — Summary"
date: "2026-07-01T21:09:08Z"
git_commit: "8e2d35c"
git_commit_full: "8e2d35c63365de7f2bb3593d346f22c11d89b6e9"
git_dirty: true
workspace_version: "0.3.0"
tier: "full"
total_cases: 6022
passed: 3480
failed: 2542
duration_s: 234.7
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

# Test Matrix Summary

**Run:** commit `8e2d35c` ⚠ dirty — v0.3.0 — 2026-07-01 21:09:08 UTC

**3480/6022 cases passed** in 234.7s

## By Use Case

| Use Case | Passed | Total | Skipped | Pass Rate |
|---|---|---|---|---|
| raw_modem | 3430 | 5968 | 0 | 57% |
| adaptive_hpx500 | 11 | 11 | 0 | 100% |
| adaptive_hpx_hf | 11 | 11 | 0 | 100% |
| adaptive_hpx_wideband | 8 | 11 | 0 | 72% |
| adaptive_hpx_ofdm_hf | 8 | 8 | 0 | 100% |
| ardop | 1 | 1 | 0 | 100% |
| kiss | 1 | 1 | 0 | 100% |
| b2f | 10 | 11 | 0 | 90% |

## Failures

| Case ID | Note |
|---|---|
| `adaptive_hpx_wideband/HPX_WIDEBAND/none/nocomp/awgn_0dB/64B` | all frames failed to decode |
| `adaptive_hpx_wideband/HPX_WIDEBAND/none/nocomp/awgn_3dB/64B` | all frames failed to decode |
| `adaptive_hpx_wideband/HPX_WIDEBAND/none/nocomp/awgn_5dB/64B` | all frames failed to decode |
| `b2f/BPSK250/none/nocomp/awgn_0dB/64B` | I/O error: Resource temporarily unavailable (os error 11) |
| `raw_modem/8PSK1000-HF-RRC/concat/nocomp/ge_heavy/128B` | RX error: FEC error: RS correction failed at block 0: TooManyErrors |
| `raw_modem/8PSK1000-HF-RRC/concat/nocomp/ge_heavy/223B` | RX error: FEC error: RS correction failed at block 0: TooManyErrors |
| `raw_modem/8PSK1000-HF-RRC/concat/nocomp/ge_heavy/32B` | RX error: FEC error: RS correction failed at block 0: TooManyErrors |
| `raw_modem/8PSK1000-HF-RRC/concat/nocomp/ge_moderate/223B` | RX error: FEC error: RS correction failed at block 1: TooManyErrors |
| `raw_modem/8PSK1000-HF-RRC/concat/nocomp/ge_severe/128B` | RX error: FEC error: decoded stream has 2074 bits, need 181883456 |
| `raw_modem/8PSK1000-HF-RRC/concat/nocomp/ge_severe/223B` | RX error: FEC error: RS correction failed at block 0: TooManyErrors |
| `raw_modem/8PSK1000-HF-RRC/concat/nocomp/ge_severe/32B` | RX error: FEC error: decoded stream has 2074 bits, need 181883456 |
| `raw_modem/8PSK1000-HF-RRC/concat/nocomp/qsb_slow/128B` | RX error: FEC error: RS correction failed at block 0: TooManyErrors |
| `raw_modem/8PSK1000-HF-RRC/concat/nocomp/qsb_slow/223B` | RX error: FEC error: RS correction failed at block 0: TooManyErrors |
| `raw_modem/8PSK1000-HF-RRC/concat/nocomp/qsb_slow/32B` | RX error: FEC error: RS correction failed at block 0: TooManyErrors |
| `raw_modem/8PSK1000-HF-RRC/concat/nocomp/watterson_extreme/128B` | RX error: FEC error: decoded stream has 2074 bits, need 117258984 |
| `raw_modem/8PSK1000-HF-RRC/concat/nocomp/watterson_extreme/223B` | RX error: FEC error: decoded stream has 4114 bits, need 5912863000 |
| `raw_modem/8PSK1000-HF-RRC/concat/nocomp/watterson_extreme/32B` | RX error: FEC error: decoded stream has 2074 bits, need 117258984 |
| `raw_modem/8PSK1000-HF-RRC/concat/nocomp/watterson_good_f1/128B` | RX error: FEC error: RS correction failed at block 0: TooManyErrors |
| `raw_modem/8PSK1000-HF-RRC/concat/nocomp/watterson_good_f1/223B` | RX error: FEC error: RS correction failed at block 0: TooManyErrors |
| `raw_modem/8PSK1000-HF-RRC/concat/nocomp/watterson_good_f1/32B` | RX error: FEC error: RS correction failed at block 0: TooManyErrors |
| `raw_modem/8PSK1000-HF-RRC/concat/nocomp/watterson_good_f2/128B` | RX error: FEC error: RS correction failed at block 0: TooManyErrors |
| `raw_modem/8PSK1000-HF-RRC/concat/nocomp/watterson_good_f2/223B` | RX error: FEC error: RS correction failed at block 0: TooManyErrors |
| `raw_modem/8PSK1000-HF-RRC/concat/nocomp/watterson_good_f2/32B` | RX error: FEC error: RS correction failed at block 0: TooManyErrors |
| `raw_modem/8PSK1000-HF-RRC/concat/nocomp/watterson_moderate_f1/128B` | RX error: FEC error: RS correction failed at block 0: TooManyErrors |
| `raw_modem/8PSK1000-HF-RRC/concat/nocomp/watterson_moderate_f1/223B` | RX error: FEC error: decoded stream has 4114 bits, need 23136 |
| `raw_modem/8PSK1000-HF-RRC/concat/nocomp/watterson_moderate_f1/32B` | RX error: FEC error: RS correction failed at block 0: TooManyErrors |
| `raw_modem/8PSK1000-HF-RRC/concat/nocomp/watterson_poor_f1/128B` | RX error: FEC error: RS correction failed at block 0: TooManyErrors |
| `raw_modem/8PSK1000-HF-RRC/concat/nocomp/watterson_poor_f1/223B` | RX error: FEC error: decoded stream has 4114 bits, need 1916440592 |
| `raw_modem/8PSK1000-HF-RRC/concat/nocomp/watterson_poor_f1/32B` | RX error: FEC error: RS correction failed at block 0: TooManyErrors |
| `raw_modem/8PSK1000-HF-RRC/none/nocomp/awgn_0dB/128B` | RX error: frame encoding/decoding error: invalid magic |
| `raw_modem/8PSK1000-HF-RRC/none/nocomp/awgn_0dB/223B` | RX error: frame encoding/decoding error: invalid magic |
| `raw_modem/8PSK1000-HF-RRC/none/nocomp/awgn_3dB/128B` | RX error: frame encoding/decoding error: invalid magic |
| `raw_modem/8PSK1000-HF-RRC/none/nocomp/awgn_3dB/223B` | RX error: frame encoding/decoding error: invalid magic |
| `raw_modem/8PSK1000-HF-RRC/none/nocomp/awgn_5dB/128B` | RX error: frame encoding/decoding error: frame truncated |
| `raw_modem/8PSK1000-HF-RRC/none/nocomp/awgn_5dB/223B` | RX error: frame encoding/decoding error: CRC mismatch (expected 0x0692, got 0x7996) |
| `raw_modem/8PSK1000-HF-RRC/none/nocomp/awgn_8dB/128B` | RX error: frame encoding/decoding error: CRC mismatch (expected 0x8579, got 0x7bb7) |
| `raw_modem/8PSK1000-HF-RRC/none/nocomp/awgn_8dB/223B` | RX error: frame encoding/decoding error: CRC mismatch (expected 0xc606, got 0x7956) |
| `raw_modem/8PSK1000-HF-RRC/none/nocomp/ge_heavy/128B` | RX error: frame encoding/decoding error: frame truncated |
| `raw_modem/8PSK1000-HF-RRC/none/nocomp/ge_heavy/223B` | RX error: frame encoding/decoding error: CRC mismatch (expected 0x2696, got 0xb7b8) |
| `raw_modem/8PSK1000-HF-RRC/none/nocomp/ge_heavy/32B` | RX error: frame encoding/decoding error: frame truncated |
| `raw_modem/8PSK1000-HF-RRC/none/nocomp/ge_light/128B` | RX error: frame encoding/decoding error: CRC mismatch (expected 0x8b37, got 0x7bb7) |
| `raw_modem/8PSK1000-HF-RRC/none/nocomp/ge_light/223B` | RX error: frame encoding/decoding error: CRC mismatch (expected 0x0c41, got 0x7956) |
| `raw_modem/8PSK1000-HF-RRC/none/nocomp/ge_light/32B` | RX error: frame encoding/decoding error: CRC mismatch (expected 0x7402, got 0x0f6a) |
| `raw_modem/8PSK1000-HF-RRC/none/nocomp/ge_moderate/128B` | RX error: frame encoding/decoding error: frame truncated |
| `raw_modem/8PSK1000-HF-RRC/none/nocomp/ge_moderate/223B` | RX error: frame encoding/decoding error: frame truncated |
| `raw_modem/8PSK1000-HF-RRC/none/nocomp/ge_moderate/32B` | RX error: frame encoding/decoding error: CRC mismatch (expected 0x648c, got 0x0f6a) |
| `raw_modem/8PSK1000-HF-RRC/none/nocomp/ge_severe/128B` | RX error: frame encoding/decoding error: invalid magic |
| `raw_modem/8PSK1000-HF-RRC/none/nocomp/ge_severe/223B` | RX error: frame encoding/decoding error: invalid magic |
| `raw_modem/8PSK1000-HF-RRC/none/nocomp/ge_severe/32B` | RX error: frame encoding/decoding error: invalid magic |
| `raw_modem/8PSK1000-HF-RRC/none/nocomp/qrn_light/128B` | RX error: frame encoding/decoding error: CRC mismatch (expected 0xc164, got 0x7bb7) |

*…and 2492 more failures. See `raw.json` for full list.*
