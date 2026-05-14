---
project: openpulsehf
doc: docs/cli-mode-advisor-guide.md
status: living
last_updated: 2026-05-14
---

# CLI Mode Advisor Guide

## Purpose

`openpulse mode-advisor` recommends a speed level and modulation mode using the current SNR estimate.

This command is intended for operators who need a quick pre-session recommendation before starting adaptive transfers.

## Usage

```sh
openpulse mode-advisor --snr <dB>
```

Example:

```sh
openpulse mode-advisor --snr 12.0
```

Example output:

```text
snr_db=12.0 recommended_speed_level=SL6 recommended_mode=QPSK500 reason="Good SNR: QPSK500 should maintain low FER."
```

## Recommendation Ladder (HF)

Current command behavior uses the HPX HF ladder:

- SNR < 3 dB -> SL2 / BPSK31
- 3 dB <= SNR < 4 dB -> SL2 / BPSK31
- 4 dB <= SNR < 5 dB -> SL3 / BPSK63
- 5 dB <= SNR < 9 dB -> SL4 / BPSK250
- 9 dB <= SNR < 11 dB -> SL5 / QPSK250
- 11 dB <= SNR < 14 dB -> SL6 / QPSK500
- SNR >= 14 dB -> SL7 / 8PSK500

## Notes

- This is a static recommendation based on instant SNR only.
- Thresholds are derived from `SessionProfile::hpx_hf()` SNR floors to stay aligned with rate-control policy.
- Dynamic trend-aware recommendations and use-case weighting are planned in later Item 10 increments.
- Always validate recommendations against channel behavior (FER, retries, and latency) during operation.
