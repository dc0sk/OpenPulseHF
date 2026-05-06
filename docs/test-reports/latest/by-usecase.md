---
title: "OpenPulseHF Test Matrix — By Use Case"
date: "2026-05-06T17:09:23Z"
git_commit: "04a6864"
tier: "quick"
total_cases: 76
passed: 75
failed: 1
duration_s: 27
generator: "openpulse-testmatrix"
---

# Results by Use Case

| Use Case | Mode | Channel | FEC | Compression | Payload | Result | BER | Duration |
|---|---|---|---|---|---|---|---|---|
| raw_modem | BPSK31 | clean | no | none | 32B | ✓ PASS | 0.0000 | 2308ms |
| raw_modem | BPSK31 | clean | no | lz4 | 32B | ✓ PASS | 0.0000 | 2314ms |
| raw_modem | BPSK63 | clean | no | none | 32B | ✓ PASS | 0.0000 | 572ms |
| raw_modem | BPSK63 | clean | no | lz4 | 32B | ✓ PASS | 0.0000 | 588ms |
| raw_modem | BPSK100 | clean | no | none | 32B | ✓ PASS | 0.0000 | 227ms |
| raw_modem | BPSK100 | clean | no | lz4 | 32B | ✓ PASS | 0.0000 | 238ms |
| raw_modem | BPSK250 | clean | no | none | 32B | ✓ PASS | 0.0000 | 40ms |
| raw_modem | BPSK250 | clean | no | lz4 | 32B | ✓ PASS | 0.0000 | 38ms |
| raw_modem | QPSK125 | clean | no | none | 32B | ✓ PASS | 0.0000 | 38ms |
| raw_modem | QPSK125 | clean | no | lz4 | 32B | ✓ PASS | 0.0000 | 38ms |
| raw_modem | QPSK250 | clean | no | none | 32B | ✓ PASS | 0.0000 | 10ms |
| raw_modem | QPSK250 | clean | no | lz4 | 32B | ✓ PASS | 0.0000 | 9ms |
| raw_modem | QPSK500 | clean | no | none | 32B | ✓ PASS | 0.0000 | 2ms |
| raw_modem | QPSK500 | clean | no | lz4 | 32B | ✓ PASS | 0.0000 | 2ms |
| raw_modem | QPSK1000 | clean | no | none | 32B | ✓ PASS | 0.0000 | 0ms |
| raw_modem | QPSK1000 | clean | no | lz4 | 32B | ✓ PASS | 0.0000 | 0ms |
| raw_modem | 8PSK500 | clean | no | none | 32B | ✓ PASS | 0.0000 | 2ms |
| raw_modem | 8PSK500 | clean | no | lz4 | 32B | ✓ PASS | 0.0000 | 1ms |
| raw_modem | 8PSK1000 | clean | no | none | 32B | ✓ PASS | 0.0000 | 0ms |
| raw_modem | 8PSK1000 | clean | no | lz4 | 32B | ✓ PASS | 0.0000 | 0ms |
| raw_modem | FSK4-ACK | clean | no | none | 5B | ✓ PASS | 0.0000 | 0ms |
| raw_modem | FSK4-ACK | awgn_20dB | no | none | 5B | ✓ PASS | 0.0000 | 3ms |
| raw_modem | FSK4-ACK | awgn_10dB | no | none | 5B | ✓ PASS | 0.0000 | 2ms |
| raw_modem | BPSK250 | clean | no | none | 128B | ✓ PASS | 0.0000 | 121ms |
| raw_modem | BPSK250 | clean | yes | none | 128B | ✓ PASS | 0.0000 | 244ms |
| raw_modem | BPSK250 | awgn_20dB | no | none | 128B | ✓ PASS | 0.0000 | 141ms |
| raw_modem | BPSK250 | awgn_20dB | yes | none | 128B | ✓ PASS | 0.0000 | 248ms |
| raw_modem | BPSK250 | awgn_10dB | no | none | 128B | ✓ PASS | 0.0000 | 136ms |
| raw_modem | BPSK250 | awgn_10dB | yes | none | 128B | ✓ PASS | 0.0000 | 246ms |
| raw_modem | QPSK125 | clean | no | none | 128B | ✓ PASS | 0.0000 | 114ms |
| raw_modem | QPSK125 | clean | yes | none | 128B | ✓ PASS | 0.0000 | 214ms |
| raw_modem | QPSK125 | awgn_20dB | no | none | 128B | ✓ PASS | 0.0000 | 142ms |
| raw_modem | QPSK125 | awgn_20dB | yes | none | 128B | ✓ PASS | 0.0000 | 267ms |
| raw_modem | QPSK125 | awgn_10dB | no | none | 128B | ✓ PASS | 0.0000 | 146ms |
| raw_modem | QPSK125 | awgn_10dB | yes | none | 128B | ✓ PASS | 0.0000 | 252ms |
| raw_modem | QPSK250 | clean | no | none | 128B | ✓ PASS | 0.0000 | 32ms |
| raw_modem | QPSK250 | clean | yes | none | 128B | ✓ PASS | 0.0000 | 58ms |
| raw_modem | QPSK250 | awgn_20dB | no | none | 128B | ✓ PASS | 0.0000 | 42ms |
| raw_modem | QPSK250 | awgn_20dB | yes | none | 128B | ✓ PASS | 0.0000 | 76ms |
| raw_modem | QPSK250 | awgn_10dB | no | none | 128B | ✓ PASS | 0.0000 | 48ms |
| raw_modem | QPSK250 | awgn_10dB | yes | none | 128B | ✓ PASS | 0.0000 | 75ms |
| raw_modem | QPSK500 | clean | no | none | 128B | ✓ PASS | 0.0000 | 8ms |
| raw_modem | QPSK500 | clean | yes | none | 128B | ✓ PASS | 0.0000 | 15ms |
| raw_modem | QPSK500 | awgn_20dB | no | none | 128B | ✓ PASS | 0.0000 | 13ms |
| raw_modem | QPSK500 | awgn_20dB | yes | none | 128B | ✓ PASS | 0.0000 | 24ms |
| raw_modem | QPSK500 | awgn_10dB | no | none | 128B | ✓ PASS | 0.0000 | 13ms |
| raw_modem | QPSK500 | awgn_10dB | yes | none | 128B | ✓ PASS | 0.0000 | 24ms |
| raw_modem | QPSK1000 | clean | no | none | 128B | ✓ PASS | 0.0000 | 2ms |
| raw_modem | QPSK1000 | clean | yes | none | 128B | ✓ PASS | 0.0000 | 4ms |
| raw_modem | QPSK1000 | awgn_20dB | no | none | 128B | ✓ PASS | 0.0000 | 5ms |
| raw_modem | QPSK1000 | awgn_20dB | yes | none | 128B | ✓ PASS | 0.0000 | 9ms |
| raw_modem | QPSK1000 | awgn_10dB | no | none | 128B | ✗ FAIL | — | 5ms |
| raw_modem | QPSK1000 | awgn_10dB | yes | none | 128B | ✓ PASS | 0.0000 | 9ms |
| raw_modem | 8PSK500 | clean | no | none | 128B | ✓ PASS | 0.0000 | 5ms |
| raw_modem | 8PSK500 | clean | yes | none | 128B | ✓ PASS | 0.0000 | 11ms |
| raw_modem | 8PSK500 | awgn_20dB | no | none | 128B | ✓ PASS | 0.0000 | 9ms |
| raw_modem | 8PSK500 | awgn_20dB | yes | none | 128B | ✓ PASS | 0.0000 | 16ms |
| raw_modem | 8PSK500 | awgn_10dB | no | none | 128B | ✓ PASS | 0.0000 | 9ms |
| raw_modem | 8PSK500 | awgn_10dB | yes | none | 128B | ✓ PASS | 0.0000 | 16ms |
| raw_modem | 8PSK1000 | clean | no | none | 128B | ✓ PASS | 0.0000 | 1ms |
| raw_modem | 8PSK1000 | clean | yes | none | 128B | ✓ PASS | 0.0000 | 4ms |
| raw_modem | 8PSK1000 | awgn_20dB | no | none | 128B | ✓ PASS | 0.0000 | 3ms |
| raw_modem | 8PSK1000 | awgn_20dB | yes | none | 128B | ✓ PASS | 0.0000 | 7ms |
| raw_modem | 8PSK1000 | awgn_10dB | no | none | 128B | ✓ PASS | 0.0000 | 5ms |
| raw_modem | 8PSK1000 | awgn_10dB | yes | none | 128B | ✓ PASS | 0.0000 | 7ms |
| adaptive_hpx500 | HPX500 | clean | no | none | 64B | ✓ PASS | — | 5597ms |
| adaptive_hpx2300 | HPX2300 | clean | no | none | 64B | ✓ PASS | — | 15ms |
| adaptive_hpx500 | HPX500 | awgn_20dB | no | none | 64B | ✓ PASS | — | 5522ms |
| adaptive_hpx2300 | HPX2300 | awgn_20dB | no | none | 64B | ✓ PASS | — | 25ms |
| adaptive_hpx500 | HPX500 | awgn_10dB | no | none | 64B | ✓ PASS | — | 5392ms |
| adaptive_hpx2300 | HPX2300 | awgn_10dB | no | none | 64B | ✓ PASS | — | 23ms |
| ardop | BPSK250 | clean | no | none | 64B | ✓ PASS | — | 27ms |
| kiss | BPSK250 | clean | no | none | 64B | ✓ PASS | — | 27ms |
| b2f | BPSK250 | clean | no | none | 64B | ✓ PASS | — | 486ms |
| b2f | BPSK250 | awgn_20dB | no | none | 64B | ✓ PASS | — | 567ms |
| b2f | BPSK250 | awgn_10dB | no | none | 64B | ✓ PASS | — | 565ms |
