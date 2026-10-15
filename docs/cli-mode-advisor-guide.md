---
project: openpulsehf
doc: docs/cli-mode-advisor-guide.md
status: living
last_updated: 2026-10-15
---

# CLI Mode Advisor Guide

## Purpose

`openpulse mode-advisor` recommends a speed level and modulation mode for a given SNR, using the
`hpx_hf` ladder of the selected session profile.

Use it for a quick pre-session recommendation before starting adaptive transfers. It needs no
audio hardware and no config file.

## Usage

```sh
openpulse mode-advisor --snr <dB> [--profile fast|robust]
```

- `--snr <dB>` (required): the estimated signal-to-noise ratio.
- `--profile <name>`: session profile. Resolution order: `--profile`, then `[modem] profile` in
  `config.toml`, then the built-in default `fast`. Only `fast` and `robust` exist; any other name
  is rejected with the list of valid names.

Example:

```sh
openpulse mode-advisor --snr 12.0
```

Example output:

```text
profile=fast snr_db=12.0 recommended_speed_level=SL9 recommended_mode=OFDM52-16QAM reason="Using profile 'fast' floor: snr_db=12.0 meets SL9 floor (12.0 dB)."
```

## Recommendation ladder

The advisor picks the highest rung whose SNR floor is at or below `--snr`. Floors and modes come
from `SessionProfile` in `crates/openpulse-core/src/profile.rs`; the advisor reads them at run
time, so this table cannot drift from the code without the code's own test
(`recommends_expected_levels_for_thresholds`) failing.

| SNR (dB) | Level | Mode | FEC used by the ladder |
|---|---|---|---|
| < 3 | SL1 | MFSK16 | Rs |
| 3 to < 4 | SL2 | BPSK31 | Rs |
| 4 to < 4.5 | SL3 | BPSK63 | Rs |
| 4.5 to < 5 | SL4 | BPSK100 | Rs |
| 5 to < 7 | SL5 | BPSK250 | Rs |
| 7 to < 9 | SL6 | QPSK250-D | Rs |
| 9 to < 10 | SL7 | OFDM52 | SoftConcatenated |
| 10 to < 12 | SL8 | OFDM52-8PSK | SoftConcatenated |
| 12 to < 14 | SL9 | OFDM52-16QAM | SoftConcatenated |
| 14 to < 16 | SL10 | OFDM52-32QAM | SoftConcatenated |
| 16 to < 18 | SL11 | OFDM52-64QAM | SoftConcatenated |
| 18 to < 19 | SL12 | OFDM52-16QAM | LDPC high-rate |
| 19 to < 20 | SL13 | OFDM52-32QAM | LDPC high-rate |
| >= 20 | SL14 | OFDM52-64QAM | LDPC high-rate |

With `--profile robust` the ladder stops at SL6 (`QPSK250-D`, at most 500 Hz): any SNR of 7 dB or
more returns SL6.

SL1 has no floor of its own; below 3 dB the advisor returns it and the reason reads
`Using profile '<name>' defaults: ... mapped to SL1.`

## Notes

- This is a static recommendation from one SNR value. The live rate controller also climbs on
  decode evidence, not only on SNR, so an on-air session can sit at a different rung.
- The floors are on two scales: SL2–SL6 (single-carrier) read approximately true channel SNR;
  SL7–SL14 (OFDM) are symbol-domain SNR from the OFDM equalizer. Do not compare a reading from one
  scale against a floor on the other.
- The advisor does not print the FEC: each rung's FEC is fixed by the ladder (column above). To
  try a rung by hand with `transmit`/`receive`, pass the matching `--fec` on both ends (`rs`,
  `soft-concatenated`); the CLI's `--fec` has no value for LDPC high-rate (SL12–SL14).
- Validate recommendations against channel behaviour (FER, retries, latency) during operation.
