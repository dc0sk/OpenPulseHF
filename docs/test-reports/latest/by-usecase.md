---
title: "OpenPulseHF Test Matrix — By Use Case"
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

# Results by Use Case

**Run:** commit `658f421` — v0.1.0 — 2026-05-11 13:44:57 UTC

| Use Case | Mode | Channel | FEC | Compression | Payload | Result | BER | Eff. bps | Duration |
|---|---|---|---|---|---|---|---|---|---|
| adaptive_hpx500 | HPX500 | awgn_10dB | none | none | 64B | ✓ PASS | 0.0000 | 628 | 4889ms |
| adaptive_hpx500 | HPX500 | awgn_20dB | none | none | 64B | ✓ PASS | 0.0000 | 617 | 4977ms |
| adaptive_hpx500 | HPX500 | clean | none | none | 64B | ✓ PASS | 0.0000 | 641 | 4791ms |
| adaptive_hpx_hf | HPX_HF | awgn_10dB | none | none | 64B | ✓ PASS | 0.0000 | 614 | 5000ms |
| adaptive_hpx_hf | HPX_HF | awgn_20dB | none | none | 64B | ✓ PASS | 0.0000 | 604 | 5085ms |
| adaptive_hpx_hf | HPX_HF | clean | none | none | 64B | ✓ PASS | 0.0000 | 624 | 4925ms |
| adaptive_hpx_wideband | HPX_WIDEBAND | awgn_10dB | none | none | 64B | ✓ PASS | 0.6667 | 46545 | 22ms |
| adaptive_hpx_wideband | HPX_WIDEBAND | awgn_20dB | none | none | 64B | ✓ PASS | 0.3333 | 93091 | 22ms |
| adaptive_hpx_wideband | HPX_WIDEBAND | clean | none | none | 64B | ✓ PASS | 0.0000 | 236308 | 13ms |
| ardop | BPSK250 | clean | none | none | 64B | ✓ PASS | — | 18286 | 28ms |
| b2f | BPSK250 | awgn_10dB | none | none | 64B | ✓ PASS | — | 948 | 540ms |
| b2f | BPSK250 | awgn_20dB | none | none | 64B | ✓ PASS | — | 1000 | 512ms |
| b2f | BPSK250 | clean | none | none | 64B | ✓ PASS | — | 1089 | 470ms |
| kiss | BPSK250 | clean | none | none | 64B | ✓ PASS | — | 18286 | 28ms |
| raw_modem | 8PSK1000-HF | clean | none | lz4 | 128B | ✓ PASS | 0.0000 | 204800 | 5ms |
| raw_modem | 8PSK1000-HF | awgn_20dB | none | none | 128B | ✓ PASS | 0.0000 | 146286 | 7ms |
| raw_modem | 8PSK1000-HF | awgn_20dB | none | none | 223B | ✓ PASS | 0.0000 | 137231 | 13ms |
| raw_modem | 8PSK1000-HF | clean | none | none | 128B | ✓ PASS | 0.0000 | 204800 | 5ms |
| raw_modem | 8PSK1000-HF | clean | none | none | 223B | ✓ PASS | 0.0000 | 223000 | 8ms |
| raw_modem | 8PSK1000-HF | clean | none | none | 32B | ✓ PASS | 0.0000 | 256000 | 1ms |
| raw_modem | 8PSK1000-HF | clean | none | zstd | 128B | ✓ PASS | 0.0000 | 256000 | 4ms |
| raw_modem | 8PSK1000-HF | clean | rs | lz4 | 128B | ✓ PASS | 0.0000 | 204800 | 5ms |
| raw_modem | 8PSK1000-HF | awgn_20dB | rs | none | 128B | ✓ PASS | 0.0000 | 170667 | 6ms |
| raw_modem | 8PSK1000-HF | awgn_20dB | rs | none | 223B | ✓ PASS | 0.0000 | 148667 | 12ms |
| raw_modem | 8PSK1000-HF | clean | rs | none | 128B | ✓ PASS | 0.0000 | 341333 | 3ms |
| raw_modem | 8PSK1000-HF | clean | rs | none | 223B | ✓ PASS | 0.0000 | 297333 | 6ms |
| raw_modem | 8PSK1000-HF | clean | rs | zstd | 128B | ✓ PASS | 0.0000 | 341333 | 3ms |
| raw_modem | 8PSK1000-HF | awgn_20dB | rs_il | none | 128B | ✓ PASS | 0.0000 | 146286 | 7ms |
| raw_modem | 8PSK1000-HF | awgn_20dB | rs_il | none | 223B | ✓ PASS | 0.0000 | 148667 | 12ms |
| raw_modem | 8PSK1000-HF | clean | rs_il | none | 128B | ✓ PASS | 0.0000 | 341333 | 3ms |
| raw_modem | 8PSK1000-HF | clean | rs_il | none | 223B | ✓ PASS | 0.0000 | 297333 | 6ms |
| raw_modem | 8PSK1000-RRC | clean | none | lz4 | 128B | ✓ PASS | 0.0000 | 46545 | 22ms |
| raw_modem | 8PSK1000-RRC | awgn_10dB | none | none | 128B | ✓ PASS | 0.0000 | 44522 | 23ms |
| raw_modem | 8PSK1000-RRC | awgn_10dB | none | none | 223B | ✓ PASS | 0.0000 | 46947 | 38ms |
| raw_modem | 8PSK1000-RRC | awgn_20dB | none | none | 128B | ✓ PASS | 0.0000 | 42667 | 24ms |
| raw_modem | 8PSK1000-RRC | awgn_20dB | none | none | 223B | ✓ PASS | 0.0000 | 46947 | 38ms |
| raw_modem | 8PSK1000-RRC | clean | none | none | 128B | ✓ PASS | 0.0000 | 46545 | 22ms |
| raw_modem | 8PSK1000-RRC | clean | none | none | 223B | ✓ PASS | 0.0000 | 49556 | 36ms |
| raw_modem | 8PSK1000-RRC | clean | none | none | 32B | ✓ PASS | 0.0000 | 32000 | 8ms |
| raw_modem | 8PSK1000-RRC | clean | none | zstd | 128B | ✓ PASS | 0.0000 | 46545 | 22ms |
| raw_modem | 8PSK1000-RRC | clean | rs | lz4 | 128B | ✓ PASS | 0.0000 | 25600 | 40ms |
| raw_modem | 8PSK1000-RRC | awgn_10dB | rs | none | 128B | ✓ PASS | 0.0000 | 23814 | 43ms |
| raw_modem | 8PSK1000-RRC | awgn_10dB | rs | none | 223B | ✓ PASS | 0.0000 | 20744 | 86ms |
| raw_modem | 8PSK1000-RRC | awgn_20dB | rs | none | 128B | ✓ PASS | 0.0000 | 23814 | 43ms |
| raw_modem | 8PSK1000-RRC | awgn_20dB | rs | none | 223B | ✓ PASS | 0.0000 | 20988 | 85ms |
| raw_modem | 8PSK1000-RRC | clean | rs | none | 128B | ✓ PASS | 0.0000 | 25600 | 40ms |
| raw_modem | 8PSK1000-RRC | clean | rs | none | 223B | ✓ PASS | 0.0000 | 22582 | 79ms |
| raw_modem | 8PSK1000-RRC | clean | rs | zstd | 128B | ✓ PASS | 0.0000 | 25600 | 40ms |
| raw_modem | 8PSK1000-RRC | awgn_10dB | rs_il | none | 128B | ✓ PASS | 0.0000 | 23273 | 44ms |
| raw_modem | 8PSK1000-RRC | awgn_10dB | rs_il | none | 223B | ✓ PASS | 0.0000 | 20988 | 85ms |
| raw_modem | 8PSK1000-RRC | awgn_20dB | rs_il | none | 128B | ✓ PASS | 0.0000 | 23814 | 43ms |
| raw_modem | 8PSK1000-RRC | awgn_20dB | rs_il | none | 223B | ✓ PASS | 0.0000 | 20988 | 85ms |
| raw_modem | 8PSK1000-RRC | clean | rs_il | none | 128B | ✓ PASS | 0.0000 | 25600 | 40ms |
| raw_modem | 8PSK1000-RRC | clean | rs_il | none | 223B | ✓ PASS | 0.0000 | 22582 | 79ms |
| raw_modem | 8PSK500-RRC | clean | none | lz4 | 128B | ✓ PASS | 0.0000 | 12190 | 84ms |
| raw_modem | 8PSK500-RRC | awgn_10dB | none | none | 128B | ✓ PASS | 0.0000 | 11770 | 87ms |
| raw_modem | 8PSK500-RRC | awgn_10dB | none | none | 223B | ✓ PASS | 0.0000 | 12219 | 146ms |
| raw_modem | 8PSK500-RRC | awgn_20dB | none | none | 128B | ✓ PASS | 0.0000 | 11770 | 87ms |
| raw_modem | 8PSK500-RRC | awgn_20dB | none | none | 223B | ✓ PASS | 0.0000 | 12563 | 142ms |
| raw_modem | 8PSK500-RRC | clean | none | none | 128B | ✓ PASS | 0.0000 | 12337 | 83ms |
| raw_modem | 8PSK500-RRC | clean | none | none | 223B | ✓ PASS | 0.0000 | 13515 | 132ms |
| raw_modem | 8PSK500-RRC | clean | none | none | 32B | ✓ PASS | 0.0000 | 9143 | 28ms |
| raw_modem | 8PSK500-RRC | clean | none | zstd | 128B | ✓ PASS | 0.0000 | 12642 | 81ms |
| raw_modem | 8PSK500-RRC | clean | rs | lz4 | 128B | ✓ PASS | 0.0000 | 7062 | 145ms |
| raw_modem | 8PSK500-RRC | awgn_10dB | rs | none | 128B | ✓ PASS | 0.0000 | 6781 | 151ms |
| raw_modem | 8PSK500-RRC | awgn_10dB | rs | none | 223B | ✓ PASS | 0.0000 | 6007 | 297ms |
| raw_modem | 8PSK500-RRC | awgn_20dB | rs | none | 128B | ✓ PASS | 0.0000 | 6827 | 150ms |
| raw_modem | 8PSK500-RRC | awgn_20dB | rs | none | 223B | ✓ PASS | 0.0000 | 6027 | 296ms |
| raw_modem | 8PSK500-RRC | clean | rs | none | 128B | ✓ PASS | 0.0000 | 6966 | 147ms |
| raw_modem | 8PSK500-RRC | clean | rs | none | 223B | ✓ PASS | 0.0000 | 6027 | 296ms |
| raw_modem | 8PSK500-RRC | clean | rs | zstd | 128B | ✓ PASS | 0.0000 | 6781 | 151ms |
| raw_modem | 8PSK500-RRC | awgn_10dB | rs_il | none | 128B | ✓ PASS | 0.0000 | 6564 | 156ms |
| raw_modem | 8PSK500-RRC | awgn_10dB | rs_il | none | 223B | ✓ PASS | 0.0000 | 5987 | 298ms |
| raw_modem | 8PSK500-RRC | awgn_20dB | rs_il | none | 128B | ✓ PASS | 0.0000 | 6737 | 152ms |
| raw_modem | 8PSK500-RRC | awgn_20dB | rs_il | none | 223B | ✓ PASS | 0.0000 | 6007 | 297ms |
| raw_modem | 8PSK500-RRC | clean | rs_il | none | 128B | ✓ PASS | 0.0000 | 6872 | 149ms |
| raw_modem | 8PSK500-RRC | clean | rs_il | none | 223B | ✓ PASS | 0.0000 | 6173 | 289ms |
| raw_modem | 8PSK500 | awgn_20dB | concat | none | 128B | ✓ PASS | 0.0000 | 30118 | 34ms |
| raw_modem | 8PSK500 | clean | none | lz4 | 128B | ✓ PASS | 0.0000 | 204800 | 5ms |
| raw_modem | 8PSK500 | awgn_20dB | none | none | 128B | ✓ PASS | 0.0000 | 113778 | 9ms |
| raw_modem | 8PSK500 | awgn_20dB | none | none | 223B | ✓ PASS | 0.0000 | 127429 | 14ms |
| raw_modem | 8PSK500 | clean | none | none | 128B | ✓ PASS | 0.0000 | 204800 | 5ms |
| raw_modem | 8PSK500 | clean | none | none | 223B | ✓ PASS | 0.0000 | 198222 | 9ms |
| raw_modem | 8PSK500 | clean | none | none | 32B | ✓ PASS | 0.0000 | 256000 | 1ms |
| raw_modem | 8PSK500 | clean | none | zstd | 128B | ✓ PASS | 0.0000 | 204800 | 5ms |
| raw_modem | 8PSK500 | clean | rs | lz4 | 128B | ✓ PASS | 0.0000 | 102400 | 10ms |
| raw_modem | 8PSK500 | awgn_20dB | rs | none | 128B | ✓ PASS | 0.0000 | 64000 | 16ms |
| raw_modem | 8PSK500 | awgn_20dB | rs | none | 223B | ✓ PASS | 0.0000 | 55750 | 32ms |
| raw_modem | 8PSK500 | clean | rs | none | 128B | ✓ PASS | 0.0000 | 102400 | 10ms |
| raw_modem | 8PSK500 | clean | rs | none | 223B | ✓ PASS | 0.0000 | 89200 | 20ms |
| raw_modem | 8PSK500 | clean | rs | zstd | 128B | ✓ PASS | 0.0000 | 102400 | 10ms |
| raw_modem | 8PSK500 | awgn_20dB | rs_il | none | 128B | ✓ PASS | 0.0000 | 64000 | 16ms |
| raw_modem | 8PSK500 | awgn_20dB | rs_il | none | 223B | ✓ PASS | 0.0000 | 55750 | 32ms |
| raw_modem | 8PSK500 | clean | rs_il | none | 128B | ✓ PASS | 0.0000 | 102400 | 10ms |
| raw_modem | 8PSK500 | clean | rs_il | none | 223B | ✓ PASS | 0.0000 | 89200 | 20ms |
| raw_modem | 8PSK500 | awgn_20dB | rs_strong | none | 128B | ✓ PASS | 0.0000 | 64000 | 16ms |
| raw_modem | 8PSK500 | awgn_20dB | soft_concat | none | 128B | ✓ PASS | 0.0000 | 21333 | 48ms |
| raw_modem | BPSK100 | clean | none | none | 32B | ✓ PASS | 0.0000 | 1085 | 236ms |
| raw_modem | BPSK250-RRC | clean | none | lz4 | 128B | ✓ PASS | 0.0000 | 1330 | 770ms |
| raw_modem | BPSK250-RRC | awgn_10dB | none | none | 128B | ✓ PASS | 0.0000 | 1286 | 796ms |
| raw_modem | BPSK250-RRC | awgn_10dB | none | none | 223B | ✓ PASS | 0.0000 | 1356 | 1316ms |
| raw_modem | BPSK250-RRC | awgn_20dB | none | none | 128B | ✓ PASS | 0.0000 | 1291 | 793ms |
| raw_modem | BPSK250-RRC | awgn_20dB | none | none | 223B | ✓ PASS | 0.0000 | 1345 | 1326ms |
| raw_modem | BPSK250-RRC | clean | none | none | 128B | ✓ PASS | 0.0000 | 1330 | 770ms |
| raw_modem | BPSK250-RRC | clean | none | none | 223B | ✓ PASS | 0.0000 | 1379 | 1294ms |
| raw_modem | BPSK250-RRC | clean | none | none | 32B | ✓ PASS | 0.0000 | 992 | 258ms |
| raw_modem | BPSK250-RRC | clean | none | zstd | 128B | ✓ PASS | 0.0000 | 1346 | 761ms |
| raw_modem | BPSK250-RRC | clean | rs | lz4 | 128B | ✓ PASS | 0.0000 | 739 | 1385ms |
| raw_modem | BPSK250-RRC | awgn_10dB | rs | none | 128B | ✓ PASS | 0.0000 | 712 | 1438ms |
| raw_modem | BPSK250-RRC | awgn_10dB | rs | none | 223B | ✓ PASS | 0.0000 | 632 | 2825ms |
| raw_modem | BPSK250-RRC | awgn_20dB | rs | none | 128B | ✓ PASS | 0.0000 | 735 | 1393ms |
| raw_modem | BPSK250-RRC | awgn_20dB | rs | none | 223B | ✓ PASS | 0.0000 | 621 | 2871ms |
| raw_modem | BPSK250-RRC | clean | rs | none | 128B | ✓ PASS | 0.0000 | 730 | 1402ms |
| raw_modem | BPSK250-RRC | clean | rs | none | 223B | ✓ PASS | 0.0000 | 643 | 2773ms |
| raw_modem | BPSK250-RRC | clean | rs | zstd | 128B | ✓ PASS | 0.0000 | 719 | 1424ms |
| raw_modem | BPSK250-RRC | awgn_10dB | rs_il | none | 128B | ✓ PASS | 0.0000 | 727 | 1408ms |
| raw_modem | BPSK250-RRC | awgn_10dB | rs_il | none | 223B | ✓ PASS | 0.0000 | 638 | 2797ms |
| raw_modem | BPSK250-RRC | awgn_20dB | rs_il | none | 128B | ✓ PASS | 0.0000 | 721 | 1420ms |
| raw_modem | BPSK250-RRC | awgn_20dB | rs_il | none | 223B | ✓ PASS | 0.0000 | 639 | 2791ms |
| raw_modem | BPSK250-RRC | clean | rs_il | none | 128B | ✓ PASS | 0.0000 | 751 | 1364ms |
| raw_modem | BPSK250-RRC | clean | rs_il | none | 223B | ✓ PASS | 0.0000 | 638 | 2798ms |
| raw_modem | BPSK250 | awgn_10dB | concat | none | 128B | ✓ PASS | 0.0000 | 1925 | 532ms |
| raw_modem | BPSK250 | awgn_20dB | concat | none | 128B | ✓ PASS | 0.0000 | 1973 | 519ms |
| raw_modem | BPSK250 | clean | none | lz4 | 128B | ✓ PASS | 0.0000 | 8678 | 118ms |
| raw_modem | BPSK250 | awgn_10dB | none | none | 128B | ✓ PASS | 0.0000 | 7111 | 144ms |
| raw_modem | BPSK250 | awgn_10dB | none | none | 223B | ✓ PASS | 0.0000 | 7757 | 230ms |
| raw_modem | BPSK250 | awgn_20dB | none | none | 128B | ✓ PASS | 0.0000 | 7474 | 137ms |
| raw_modem | BPSK250 | awgn_20dB | none | none | 223B | ✓ PASS | 0.0000 | 7825 | 228ms |
| raw_modem | BPSK250 | clean | none | none | 128B | ✓ PASS | 0.0000 | 8678 | 118ms |
| raw_modem | BPSK250 | clean | none | none | 223B | ✓ PASS | 0.0000 | 9102 | 196ms |
| raw_modem | BPSK250 | clean | none | none | 32B | ✓ PASS | 0.0000 | 6737 | 38ms |
| raw_modem | BPSK250 | clean | none | zstd | 128B | ✓ PASS | 0.0000 | 8678 | 118ms |
| raw_modem | BPSK250 | clean | rs | lz4 | 128B | ✓ PASS | 0.0000 | 4741 | 216ms |
| raw_modem | BPSK250 | awgn_10dB | rs | none | 128B | ✓ PASS | 0.0000 | 4080 | 251ms |
| raw_modem | BPSK250 | awgn_10dB | rs | none | 223B | ✓ PASS | 0.0000 | 3590 | 497ms |
| raw_modem | BPSK250 | awgn_20dB | rs | none | 128B | ✓ PASS | 0.0000 | 3850 | 266ms |
| raw_modem | BPSK250 | awgn_20dB | rs | none | 223B | ✓ PASS | 0.0000 | 3471 | 514ms |
| raw_modem | BPSK250 | clean | rs | none | 128B | ✓ PASS | 0.0000 | 4763 | 215ms |
| raw_modem | BPSK250 | clean | rs | none | 223B | ✓ PASS | 0.0000 | 4168 | 428ms |
| raw_modem | BPSK250 | clean | rs | zstd | 128B | ✓ PASS | 0.0000 | 4763 | 215ms |
| raw_modem | BPSK250 | awgn_10dB | rs_il | none | 128B | ✓ PASS | 0.0000 | 4096 | 250ms |
| raw_modem | BPSK250 | awgn_10dB | rs_il | none | 223B | ✓ PASS | 0.0000 | 3582 | 498ms |
| raw_modem | BPSK250 | awgn_20dB | rs_il | none | 128B | ✓ PASS | 0.0000 | 4000 | 256ms |
| raw_modem | BPSK250 | awgn_20dB | rs_il | none | 223B | ✓ PASS | 0.0000 | 3505 | 509ms |
| raw_modem | BPSK250 | clean | rs_il | none | 128B | ✓ PASS | 0.0000 | 4633 | 221ms |
| raw_modem | BPSK250 | clean | rs_il | none | 223B | ✓ PASS | 0.0000 | 4111 | 434ms |
| raw_modem | BPSK250 | awgn_10dB | rs_strong | none | 128B | ✓ PASS | 0.0000 | 4080 | 251ms |
| raw_modem | BPSK250 | awgn_20dB | rs_strong | none | 128B | ✓ PASS | 0.0000 | 4080 | 251ms |
| raw_modem | BPSK250 | awgn_10dB | soft_concat | none | 128B | ✓ PASS | 0.0000 | 1947 | 526ms |
| raw_modem | BPSK250 | awgn_20dB | soft_concat | none | 128B | ✓ PASS | 0.0000 | 1962 | 522ms |
| raw_modem | BPSK31 | clean | none | none | 32B | ✓ PASS | 0.0000 | 112 | 2294ms |
| raw_modem | BPSK63 | clean | none | none | 32B | ✓ PASS | 0.0000 | 432 | 592ms |
| raw_modem | FSK4-ACK | awgn_20dB | none | none | 5B | ✓ PASS | 0.0000 | 13333 | 3ms |
| raw_modem | FSK4-ACK | clean | none | none | 5B | ✓ PASS | 0.0000 | — | 0ms |
| raw_modem | OFDM16 | awgn_10dB | none | none | 128B | ✓ PASS | 0.0000 | 113778 | 9ms |
| raw_modem | OFDM16 | awgn_20dB | none | none | 128B | ✓ PASS | 0.0000 | 113778 | 9ms |
| raw_modem | OFDM16 | clean | none | none | 128B | ✓ PASS | 0.0000 | 341333 | 3ms |
| raw_modem | OFDM16 | clean | none | none | 32B | ✓ PASS | 0.0000 | 256000 | 1ms |
| raw_modem | OFDM16 | awgn_10dB | rs | none | 128B | ✓ PASS | 0.0000 | 56889 | 18ms |
| raw_modem | OFDM16 | awgn_20dB | rs | none | 128B | ✓ PASS | 0.0000 | 56889 | 18ms |
| raw_modem | OFDM16 | clean | rs | none | 128B | ✓ PASS | 0.0000 | 146286 | 7ms |
| raw_modem | OFDM52 | awgn_20dB | none | none | 128B | ✓ PASS | 0.0000 | 341333 | 3ms |
| raw_modem | OFDM52 | clean | none | none | 128B | ✓ PASS | 0.0000 | 1024000 | 1ms |
| raw_modem | OFDM52 | clean | none | none | 32B | ✓ PASS | 0.0000 | — | 0ms |
| raw_modem | OFDM52 | awgn_20dB | rs | none | 128B | ✓ PASS | 0.0000 | 170667 | 6ms |
| raw_modem | OFDM52 | clean | rs | none | 128B | ✓ PASS | 0.0000 | 341333 | 3ms |
| raw_modem | QPSK1000-HF | clean | none | lz4 | 128B | ✓ PASS | 0.0000 | 512000 | 2ms |
| raw_modem | QPSK1000-HF | awgn_10dB | none | none | 128B | ✓ PASS | 0.0000 | 204800 | 5ms |
| raw_modem | QPSK1000-HF | awgn_10dB | none | none | 223B | ✓ PASS | 0.0000 | 223000 | 8ms |
| raw_modem | QPSK1000-HF | awgn_20dB | none | none | 128B | ✓ PASS | 0.0000 | 204800 | 5ms |
| raw_modem | QPSK1000-HF | awgn_20dB | none | none | 223B | ✓ PASS | 0.0000 | 223000 | 8ms |
| raw_modem | QPSK1000-HF | clean | none | none | 128B | ✓ PASS | 0.0000 | 512000 | 2ms |
| raw_modem | QPSK1000-HF | clean | none | none | 223B | ✓ PASS | 0.0000 | 446000 | 4ms |
| raw_modem | QPSK1000-HF | clean | none | none | 32B | ✓ PASS | 0.0000 | — | 0ms |
| raw_modem | QPSK1000-HF | clean | none | zstd | 128B | ✓ PASS | 0.0000 | 512000 | 2ms |
| raw_modem | QPSK1000-HF | clean | rs | lz4 | 128B | ✓ PASS | 0.0000 | 204800 | 5ms |
| raw_modem | QPSK1000-HF | awgn_10dB | rs | none | 128B | ✓ PASS | 0.0000 | 113778 | 9ms |
| raw_modem | QPSK1000-HF | awgn_10dB | rs | none | 223B | ✓ PASS | 0.0000 | 93895 | 19ms |
| raw_modem | QPSK1000-HF | awgn_20dB | rs | none | 128B | ✓ PASS | 0.0000 | 93091 | 11ms |
| raw_modem | QPSK1000-HF | awgn_20dB | rs | none | 223B | ✓ PASS | 0.0000 | 93895 | 19ms |
| raw_modem | QPSK1000-HF | clean | rs | none | 128B | ✓ PASS | 0.0000 | 204800 | 5ms |
| raw_modem | QPSK1000-HF | clean | rs | none | 223B | ✓ PASS | 0.0000 | 178400 | 10ms |
| raw_modem | QPSK1000-HF | clean | rs | zstd | 128B | ✓ PASS | 0.0000 | 204800 | 5ms |
| raw_modem | QPSK1000-HF | awgn_10dB | rs_il | none | 128B | ✓ PASS | 0.0000 | 102400 | 10ms |
| raw_modem | QPSK1000-HF | awgn_10dB | rs_il | none | 223B | ✓ PASS | 0.0000 | 93895 | 19ms |
| raw_modem | QPSK1000-HF | awgn_20dB | rs_il | none | 128B | ✓ PASS | 0.0000 | 113778 | 9ms |
| raw_modem | QPSK1000-HF | awgn_20dB | rs_il | none | 223B | ✓ PASS | 0.0000 | 93895 | 19ms |
| raw_modem | QPSK1000-HF | clean | rs_il | none | 128B | ✓ PASS | 0.0000 | 204800 | 5ms |
| raw_modem | QPSK1000-HF | clean | rs_il | none | 223B | ✓ PASS | 0.0000 | 178400 | 10ms |
| raw_modem | QPSK1000-RRC | clean | none | lz4 | 128B | ✓ PASS | 0.0000 | 29257 | 35ms |
| raw_modem | QPSK1000-RRC | awgn_10dB | none | none | 128B | ✓ PASS | 0.0000 | 28444 | 36ms |
| raw_modem | QPSK1000-RRC | awgn_10dB | none | none | 223B | ✓ PASS | 0.0000 | 29246 | 61ms |
| raw_modem | QPSK1000-RRC | awgn_20dB | none | none | 128B | ✓ PASS | 0.0000 | 28444 | 36ms |
| raw_modem | QPSK1000-RRC | awgn_20dB | none | none | 223B | ✓ PASS | 0.0000 | 30237 | 59ms |
| raw_modem | QPSK1000-RRC | clean | none | none | 128B | ✓ PASS | 0.0000 | 29257 | 35ms |
| raw_modem | QPSK1000-RRC | clean | none | none | 223B | ✓ PASS | 0.0000 | 32436 | 55ms |
| raw_modem | QPSK1000-RRC | clean | none | none | 32B | ✓ PASS | 0.0000 | 23273 | 11ms |
| raw_modem | QPSK1000-RRC | clean | none | zstd | 128B | ✓ PASS | 0.0000 | 29257 | 35ms |
| raw_modem | QPSK1000-RRC | clean | rs | lz4 | 128B | ✓ PASS | 0.0000 | 16787 | 61ms |
| raw_modem | QPSK1000-RRC | awgn_10dB | rs | none | 128B | ✓ PASS | 0.0000 | 15754 | 65ms |
| raw_modem | QPSK1000-RRC | awgn_10dB | rs | none | 223B | ✓ PASS | 0.0000 | 13313 | 134ms |
| raw_modem | QPSK1000-RRC | awgn_20dB | rs | none | 128B | ✓ PASS | 0.0000 | 15284 | 67ms |
| raw_modem | QPSK1000-RRC | awgn_20dB | rs | none | 223B | ✓ PASS | 0.0000 | 13022 | 137ms |
| raw_modem | QPSK1000-RRC | clean | rs | none | 128B | ✓ PASS | 0.0000 | 15754 | 65ms |
| raw_modem | QPSK1000-RRC | clean | rs | none | 223B | ✓ PASS | 0.0000 | 15248 | 117ms |
| raw_modem | QPSK1000-RRC | clean | rs | zstd | 128B | ✓ PASS | 0.0000 | 17655 | 58ms |
| raw_modem | QPSK1000-RRC | awgn_10dB | rs_il | none | 128B | ✓ PASS | 0.0000 | 16254 | 63ms |
| raw_modem | QPSK1000-RRC | awgn_10dB | rs_il | none | 223B | ✓ PASS | 0.0000 | 14272 | 125ms |
| raw_modem | QPSK1000-RRC | awgn_20dB | rs_il | none | 128B | ✓ PASS | 0.0000 | 16516 | 62ms |
| raw_modem | QPSK1000-RRC | awgn_20dB | rs_il | none | 223B | ✓ PASS | 0.0000 | 14387 | 124ms |
| raw_modem | QPSK1000-RRC | clean | rs_il | none | 128B | ✓ PASS | 0.0000 | 17356 | 59ms |
| raw_modem | QPSK1000-RRC | clean | rs_il | none | 223B | ✓ PASS | 0.0000 | 15379 | 116ms |
| raw_modem | QPSK125 | clean | none | lz4 | 128B | ✓ PASS | 0.0000 | 8982 | 114ms |
| raw_modem | QPSK125 | awgn_10dB | none | none | 128B | ✓ PASS | 0.0000 | 7699 | 133ms |
| raw_modem | QPSK125 | awgn_10dB | none | none | 223B | ✓ PASS | 0.0000 | 8072 | 221ms |
| raw_modem | QPSK125 | awgn_20dB | none | none | 128B | ✓ PASS | 0.0000 | 7699 | 133ms |
| raw_modem | QPSK125 | awgn_20dB | none | none | 223B | ✓ PASS | 0.0000 | 8072 | 221ms |
| raw_modem | QPSK125 | clean | none | none | 128B | ✓ PASS | 0.0000 | 8904 | 115ms |
| raw_modem | QPSK125 | clean | none | none | 223B | ✓ PASS | 0.0000 | 9244 | 193ms |
| raw_modem | QPSK125 | clean | none | none | 32B | ✓ PASS | 0.0000 | 6564 | 39ms |
| raw_modem | QPSK125 | clean | none | zstd | 128B | ✓ PASS | 0.0000 | 8533 | 120ms |
| raw_modem | QPSK125 | clean | rs | lz4 | 128B | ✓ PASS | 0.0000 | 4676 | 219ms |
| raw_modem | QPSK125 | awgn_10dB | rs | none | 128B | ✓ PASS | 0.0000 | 4031 | 254ms |
| raw_modem | QPSK125 | awgn_10dB | rs | none | 223B | ✓ PASS | 0.0000 | 3590 | 497ms |
| raw_modem | QPSK125 | awgn_20dB | rs | none | 128B | ✓ PASS | 0.0000 | 4031 | 254ms |
| raw_modem | QPSK125 | awgn_20dB | rs | none | 223B | ✓ PASS | 0.0000 | 3512 | 508ms |
| raw_modem | QPSK125 | clean | rs | none | 128B | ✓ PASS | 0.0000 | 4697 | 218ms |
| raw_modem | QPSK125 | clean | rs | none | 223B | ✓ PASS | 0.0000 | 4111 | 434ms |
| raw_modem | QPSK125 | clean | rs | zstd | 128B | ✓ PASS | 0.0000 | 4655 | 220ms |
| raw_modem | QPSK125 | awgn_10dB | rs_il | none | 128B | ✓ PASS | 0.0000 | 4214 | 243ms |
| raw_modem | QPSK125 | awgn_10dB | rs_il | none | 223B | ✓ PASS | 0.0000 | 3686 | 484ms |
| raw_modem | QPSK125 | awgn_20dB | rs_il | none | 128B | ✓ PASS | 0.0000 | 4197 | 244ms |
| raw_modem | QPSK125 | awgn_20dB | rs_il | none | 223B | ✓ PASS | 0.0000 | 3626 | 492ms |
| raw_modem | QPSK125 | clean | rs_il | none | 128B | ✓ PASS | 0.0000 | 4676 | 219ms |
| raw_modem | QPSK125 | clean | rs_il | none | 223B | ✓ PASS | 0.0000 | 4111 | 434ms |
| raw_modem | QPSK250 | clean | none | lz4 | 128B | ✓ PASS | 0.0000 | 33032 | 31ms |
| raw_modem | QPSK250 | awgn_10dB | none | none | 128B | ✓ PASS | 0.0000 | 24976 | 41ms |
| raw_modem | QPSK250 | awgn_10dB | none | none | 223B | ✓ PASS | 0.0000 | 25855 | 69ms |
| raw_modem | QPSK250 | awgn_20dB | none | none | 128B | ✓ PASS | 0.0000 | 24381 | 42ms |
| raw_modem | QPSK250 | awgn_20dB | none | none | 223B | ✓ PASS | 0.0000 | 26235 | 68ms |
| raw_modem | QPSK250 | clean | none | none | 128B | ✓ PASS | 0.0000 | 31030 | 33ms |
| raw_modem | QPSK250 | clean | none | none | 223B | ✓ PASS | 0.0000 | 34308 | 52ms |
| raw_modem | QPSK250 | clean | none | none | 32B | ✓ PASS | 0.0000 | 25600 | 10ms |
| raw_modem | QPSK250 | clean | none | zstd | 128B | ✓ PASS | 0.0000 | 31030 | 33ms |
| raw_modem | QPSK250 | clean | rs | lz4 | 128B | ✓ PASS | 0.0000 | 18963 | 54ms |
| raw_modem | QPSK250 | awgn_10dB | rs | none | 128B | ✓ PASS | 0.0000 | 14222 | 72ms |
| raw_modem | QPSK250 | awgn_10dB | rs | none | 223B | ✓ PASS | 0.0000 | 12389 | 144ms |
| raw_modem | QPSK250 | awgn_20dB | rs | none | 128B | ✓ PASS | 0.0000 | 14222 | 72ms |
| raw_modem | QPSK250 | awgn_20dB | rs | none | 223B | ✓ PASS | 0.0000 | 12476 | 143ms |
| raw_modem | QPSK250 | clean | rs | none | 128B | ✓ PASS | 0.0000 | 17965 | 57ms |
| raw_modem | QPSK250 | clean | rs | none | 223B | ✓ PASS | 0.0000 | 15649 | 114ms |
| raw_modem | QPSK250 | clean | rs | zstd | 128B | ✓ PASS | 0.0000 | 17655 | 58ms |
| raw_modem | QPSK250 | awgn_10dB | rs_il | none | 128B | ✓ PASS | 0.0000 | 13474 | 76ms |
| raw_modem | QPSK250 | awgn_10dB | rs_il | none | 223B | ✓ PASS | 0.0000 | 11893 | 150ms |
| raw_modem | QPSK250 | awgn_20dB | rs_il | none | 128B | ✓ PASS | 0.0000 | 13653 | 75ms |
| raw_modem | QPSK250 | awgn_20dB | rs_il | none | 223B | ✓ PASS | 0.0000 | 11973 | 149ms |
| raw_modem | QPSK250 | clean | rs_il | none | 128B | ✓ PASS | 0.0000 | 17965 | 57ms |
| raw_modem | QPSK250 | clean | rs_il | none | 223B | ✓ PASS | 0.0000 | 15649 | 114ms |
| raw_modem | QPSK500-RRC | clean | none | lz4 | 128B | ✓ PASS | 0.0000 | 8127 | 126ms |
| raw_modem | QPSK500-RRC | awgn_10dB | none | none | 128B | ✓ PASS | 0.0000 | 7758 | 132ms |
| raw_modem | QPSK500-RRC | awgn_10dB | none | none | 223B | ✓ PASS | 0.0000 | 8183 | 218ms |
| raw_modem | QPSK500-RRC | awgn_20dB | none | none | 128B | ✓ PASS | 0.0000 | 7817 | 131ms |
| raw_modem | QPSK500-RRC | awgn_20dB | none | none | 223B | ✓ PASS | 0.0000 | 8221 | 217ms |
| raw_modem | QPSK500-RRC | clean | none | none | 128B | ✓ PASS | 0.0000 | 8063 | 127ms |
| raw_modem | QPSK500-RRC | clean | none | none | 223B | ✓ PASS | 0.0000 | 8495 | 210ms |
| raw_modem | QPSK500-RRC | clean | none | none | 32B | ✓ PASS | 0.0000 | 6095 | 42ms |
| raw_modem | QPSK500-RRC | clean | none | zstd | 128B | ✓ PASS | 0.0000 | 8063 | 127ms |
| raw_modem | QPSK500-RRC | clean | rs | lz4 | 128B | ✓ PASS | 0.0000 | 4433 | 231ms |
| raw_modem | QPSK500-RRC | awgn_10dB | rs | none | 128B | ✓ PASS | 0.0000 | 4285 | 239ms |
| raw_modem | QPSK500-RRC | awgn_10dB | rs | none | 223B | ✓ PASS | 0.0000 | 3982 | 448ms |
| raw_modem | QPSK500-RRC | awgn_20dB | rs | none | 128B | ✓ PASS | 0.0000 | 4491 | 228ms |
| raw_modem | QPSK500-RRC | awgn_20dB | rs | none | 223B | ✓ PASS | 0.0000 | 3982 | 448ms |
| raw_modem | QPSK500-RRC | clean | rs | none | 128B | ✓ PASS | 0.0000 | 4655 | 220ms |
| raw_modem | QPSK500-RRC | clean | rs | none | 223B | ✓ PASS | 0.0000 | 4036 | 442ms |
| raw_modem | QPSK500-RRC | clean | rs | zstd | 128B | ✓ PASS | 0.0000 | 4491 | 228ms |
| raw_modem | QPSK500-RRC | awgn_10dB | rs_il | none | 128B | ✓ PASS | 0.0000 | 4452 | 230ms |
| raw_modem | QPSK500-RRC | awgn_10dB | rs_il | none | 223B | ✓ PASS | 0.0000 | 3912 | 456ms |
| raw_modem | QPSK500-RRC | awgn_20dB | rs_il | none | 128B | ✓ PASS | 0.0000 | 4491 | 228ms |
| raw_modem | QPSK500-RRC | awgn_20dB | rs_il | none | 223B | ✓ PASS | 0.0000 | 3973 | 449ms |
| raw_modem | QPSK500-RRC | clean | rs_il | none | 128B | ✓ PASS | 0.0000 | 4655 | 220ms |
| raw_modem | QPSK500-RRC | clean | rs_il | none | 223B | ✓ PASS | 0.0000 | 3973 | 449ms |
| raw_modem | QPSK500 | awgn_10dB | concat | none | 128B | ✓ PASS | 0.0000 | 19692 | 52ms |
| raw_modem | QPSK500 | awgn_20dB | concat | none | 128B | ✓ PASS | 0.0000 | 19692 | 52ms |
| raw_modem | QPSK500 | clean | none | lz4 | 128B | ✓ PASS | 0.0000 | 113778 | 9ms |
| raw_modem | QPSK500 | awgn_10dB | none | none | 128B | ✓ PASS | 0.0000 | 73143 | 14ms |
| raw_modem | QPSK500 | awgn_10dB | none | none | 223B | ✓ PASS | 0.0000 | 81091 | 22ms |
| raw_modem | QPSK500 | awgn_20dB | none | none | 128B | ✓ PASS | 0.0000 | 78769 | 13ms |
| raw_modem | QPSK500 | awgn_20dB | none | none | 223B | ✓ PASS | 0.0000 | 81091 | 22ms |
| raw_modem | QPSK500 | clean | none | none | 128B | ✓ PASS | 0.0000 | 128000 | 8ms |
| raw_modem | QPSK500 | clean | none | none | 223B | ✓ PASS | 0.0000 | 127429 | 14ms |
| raw_modem | QPSK500 | clean | none | none | 32B | ✓ PASS | 0.0000 | 128000 | 2ms |
| raw_modem | QPSK500 | clean | none | zstd | 128B | ✓ PASS | 0.0000 | 128000 | 8ms |
| raw_modem | QPSK500 | clean | rs | lz4 | 128B | ✓ PASS | 0.0000 | 64000 | 16ms |
| raw_modem | QPSK500 | awgn_10dB | rs | none | 128B | ✓ PASS | 0.0000 | 40960 | 25ms |
| raw_modem | QPSK500 | awgn_10dB | rs | none | 223B | ✓ PASS | 0.0000 | 35680 | 50ms |
| raw_modem | QPSK500 | awgn_20dB | rs | none | 128B | ✓ PASS | 0.0000 | 40960 | 25ms |
| raw_modem | QPSK500 | awgn_20dB | rs | none | 223B | ✓ PASS | 0.0000 | 35680 | 50ms |
| raw_modem | QPSK500 | clean | rs | none | 128B | ✓ PASS | 0.0000 | 64000 | 16ms |
| raw_modem | QPSK500 | clean | rs | none | 223B | ✓ PASS | 0.0000 | 55750 | 32ms |
| raw_modem | QPSK500 | clean | rs | zstd | 128B | ✓ PASS | 0.0000 | 64000 | 16ms |
| raw_modem | QPSK500 | awgn_10dB | rs_il | none | 128B | ✓ PASS | 0.0000 | 40960 | 25ms |
| raw_modem | QPSK500 | awgn_10dB | rs_il | none | 223B | ✓ PASS | 0.0000 | 35680 | 50ms |
| raw_modem | QPSK500 | awgn_20dB | rs_il | none | 128B | ✓ PASS | 0.0000 | 40960 | 25ms |
| raw_modem | QPSK500 | awgn_20dB | rs_il | none | 223B | ✓ PASS | 0.0000 | 34980 | 51ms |
| raw_modem | QPSK500 | clean | rs_il | none | 128B | ✓ PASS | 0.0000 | 60235 | 17ms |
| raw_modem | QPSK500 | clean | rs_il | none | 223B | ✓ PASS | 0.0000 | 55750 | 32ms |
| raw_modem | QPSK500 | awgn_10dB | rs_strong | none | 128B | ✓ PASS | 0.0000 | 39385 | 26ms |
| raw_modem | QPSK500 | awgn_20dB | rs_strong | none | 128B | ✓ PASS | 0.0000 | 39385 | 26ms |
| raw_modem | QPSK500 | awgn_10dB | soft_concat | none | 128B | ✓ PASS | 0.0000 | 16254 | 63ms |
| raw_modem | QPSK500 | awgn_20dB | soft_concat | none | 128B | ✓ PASS | 0.0000 | 16516 | 62ms |
| raw_modem | SCFDMA16 | awgn_10dB | none | none | 128B | ✓ PASS | 0.0000 | 128000 | 8ms |
| raw_modem | SCFDMA16 | awgn_20dB | none | none | 128B | ✓ PASS | 0.0000 | 128000 | 8ms |
| raw_modem | SCFDMA16 | clean | none | none | 128B | ✓ PASS | 0.0000 | 512000 | 2ms |
| raw_modem | SCFDMA16 | clean | none | none | 32B | ✓ PASS | 0.0000 | — | 0ms |
| raw_modem | SCFDMA16 | awgn_10dB | rs | none | 128B | ✓ PASS | 0.0000 | 68267 | 15ms |
| raw_modem | SCFDMA16 | awgn_20dB | rs | none | 128B | ✓ PASS | 0.0000 | 68267 | 15ms |
| raw_modem | SCFDMA16 | clean | rs | none | 128B | ✓ PASS | 0.0000 | 204800 | 5ms |
| raw_modem | SCFDMA52 | awgn_20dB | none | none | 128B | ✓ PASS | 0.0000 | 341333 | 3ms |
| raw_modem | SCFDMA52 | clean | none | none | 128B | ✓ PASS | 0.0000 | 1024000 | 1ms |
| raw_modem | SCFDMA52 | clean | none | none | 32B | ✓ PASS | 0.0000 | — | 0ms |
| raw_modem | SCFDMA52 | awgn_20dB | rs | none | 128B | ✓ PASS | 0.0000 | 204800 | 5ms |
| raw_modem | SCFDMA52 | clean | rs | none | 128B | ✓ PASS | 0.0000 | 512000 | 2ms |
