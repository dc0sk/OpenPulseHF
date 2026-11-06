---
project: openpulsehf
doc: docs/dev/design/notch-protects-the-ladder.md
status: resolved
last_updated: 2026-11-04
---

# The receive notch protects every rung the OTA receiver will try

Work plan M3; Release 1 `fast` profile (SL7–SL14). Found in the review of task 22, run f1b, on
2026-11-04.

## Problem — measured

**On air (task 22, run f1b, B receiving):**
- B's OFDM SNR readings were SL7 1.5, SL8 5.0 and SL9 8.5 dB (medians).
- On the same frames from the recording, the plugin's own estimator reads SL7 6.75, SL8 12.8 and
  SL10 13.4–14.5 dB. The recording is taken before the engine's receive front end.
- SL10 never decoded.

**The mechanism:**
1. The daemon's capture tick passes `handle.active_mode` to `accumulate_capture`. That is its base
   mode from the configuration (`BPSK250`), not the ladder's rung (`server.rs`, the rx tick).
2. `apply_rx_notch` protects `centre ± occupied_bandwidth(mode)/2`. For BPSK250 that is 1250–1750 Hz.
3. OFDM52 occupies 484–2516 Hz. Its edge subcarriers at 500 Hz and 2500 Hz (the last is a pilot)
   stand out from the protected band's edge, the notch takes them for out-of-band CW, and it nulls
   them.

**On the daemon's path** (`accumulate_capture` with `BPSK250` → `ota_decode_burst`; the stations'
notch configuration `notch_max` 10, `notch_q` 25; noise 0.02 RMS):

| rung | notch off | notch on | notches placed |
|---|---|---|---|
| SL7 OFDM52 | 15.6 dB | 4.9 dB | 490–520, 2430–2500 Hz |
| SL8 8PSK | 20.5 dB | 5.9 dB | 490, 500, 2500 Hz |
| SL9 16QAM | 21.2 dB | 11.4 dB | 490, 500, 2460, 2500, 3110 Hz |
| SL10 32QAM | 20.3 dB | 9.6 dB | 500, 510, 2500 Hz |

The notch-on readings are what B read on air. Every frame still decoded in the simulation; on air,
with less margin, SL10 did not.

## Decision — implemented

While an OTA session is active, `apply_rx_notch` protects the widest occupied band among the
caller's mode and the receiver's `rx_candidates()`.
- **On `robust` (SL1–SL6)** that is still about 500 Hz, so the notch's reach does not change.
- **On `fast`, once an OFDM rung is a candidate,** the protected band is the OFDM band (≈500–2500 Hz).
  An interferer inside it is in band for OFDM: a case for a QSY, not for a notch.
- **Not changed:**
  - the one-shot receive paths, which set `rx_mode` themselves;
  - the notch outside an OTA session;
  - the daemon's capture tick (the fix is at the seam, so every caller gets it).

## Tests

- `notch_protects_the_ladder` (`openpulse-modem`), through `accumulate_capture` → `ota_decode_burst`
  with the stations' notch configuration.
  - Every OFDM rung SL7–SL10 decodes, and reads within 1 dB of its notch-off SNR.
  - It also asserts the notch ran (`notch_blocks_processed`).
- **Sabotage:** the candidate loop disabled → SL7 reads 4.9 dB against 15.6, and the test fails.
- `scripts/slow-tests.sh notch` (REQ-QRM-01): the result is recorded with the commit.

## Consumer

- `ModemEngine::route_audio_stage(InputCapture)` → `apply_rx_notch`, on every capture path that runs
  the notch: the daemon's `server::run` rx tick, and ARDOP and KISS through `CaptureTicker`.
- Found with `grep -n "apply_rx_notch" crates/openpulse-modem/src/engine.rs`.

## Prior art

- **#556:** the notch at the single capture seam.
- **The `receive_with_timeout` fix** (engine, the `rx_mode` comment): a wrong mode makes an
  interferer un-notchable; this is the other direction, a wrong mode makes the signal notchable.
- **`docs/dev/design/ofdm-padding-whitening.md`:** the first half of f1's low OFDM reading.

## Twins

- **The ARDOP and KISS capture paths** pass their own mode. They are covered by the same seam when an
  OTA session is active. No test runs them with OFDM: UNCHECKED.
- **SL6 (QPSK250-D) under the notch:** the scratch run placed notches at 1040–1200 and 1800–2070 Hz,
  outside its 500 Hz protected band, on the signal's skirts. Its SNR reading on this path is `None` in
  the harness, so the cost is UNCHECKED. a2h's SL6 read a flat ~11 dB through 28 dB of attenuation,
  which a notch-limited reading would also explain.
