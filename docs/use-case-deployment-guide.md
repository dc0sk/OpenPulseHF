---
project: openpulsehf
doc: docs/use-case-deployment-guide.md
status: living
last_updated: 2026-10-15
---

# OpenPulseHF Use-Case Deployment Guide

This guide maps three deployment use cases onto the two session profiles that exist today, and
gives the validation steps for each:
- field_relay (high fading)
- emergency (low SNR tolerance)
- station_relay (high BER margin)

It supports both:
- field/on-air validation (when regulatory approval is available), and
- lab-only fallback validation (no RF transmission required).

Release 1 (`v0.17.0`, not yet cut) is tested in this order: virtual audio, hardware loopback, 2 m
on air, and HF only with the release candidate. Plan field work to match.

## 1. Use-Case Profiles

There is one rate ladder, `hpx_hf` (SL1–SL14), and two session profiles on it:

- **`fast`** (default): the whole ladder, up to about 2031 Hz occupied.
- **`robust`**: the same ladder capped at SL6 (`QPSK250-D`), at most 500 Hz, single-carrier, every
  rung coded.

A `fast` and a `robust` station interoperate. No other profile name is accepted. The ladder table
(modes, FEC, SNR floors) is in [mode-fec-ladder.md](mode-fec-ladder.md) and the manual
([§1.2.1](openpulse-manual.md#121-the-hpx_hf-ladder-primary-hf-profile)).

| Use case | Target environment | Profile | Rungs reachable | Notes |
|---|---|---|---|---|
| field_relay | Mobile/portable, variable fading, intermittent QRM | `robust` | SL1–SL6 (MFSK16, BPSK31–250, QPSK250-D) | Narrow and single-carrier; keeps a portable station inside 500 Hz |
| emergency | Low power, weak signal, reliability first | `robust` | SL1–SL6 | MFSK16 (SL1) is the deep-fade rung below BPSK31's 3 dB floor |
| station_relay | Fixed stations, stronger links, throughput first | `fast` | SL1–SL14 (adds OFDM52 rungs) | Needs up to ≈2031 Hz and a clean SSB passband |

Notes:
- Every rung is FEC-coded: `Rs` on SL1–SL6, soft concatenated on SL7–SL11, LDPC high-rate on
  SL12–SL14. There is no useful uncoded rung on a fading channel.
- The earlier use-case ladders (QPSK1000-HF, 8PSK1000-HF, SCFDMA52-64QAM-P4 and similar) are not on
  `hpx_hf`: coherent single-carrier mid rungs decoded about 0 % on a simulated moderate fade. Those
  modes remain selectable as fixed modes (`[modem] mode`) but are not part of Release 1.

## 2. Mode Selection Guidance

The rate controller is receiver-led and automatic; the operator chooses the profile and, if needed,
bounds:

1. Pick the profile from the table above (`[modem] profile`, or `ota_profile` for the daemon's OTA
   session; `[ardop] adaptive_profile` for the ARDOP TNC).
2. Sessions start at SL2 (`BPSK31`, the ladder's initial level). The controller climbs on decode
   evidence as well as SNR (after three consecutive clean decodes) and never demotes below a level
   that just decoded.
3. To bound it, set `[modem] ota_min_level` / `ota_max_level` (e.g. `"SL3"` / `"SL10"`), or lock it
   with `ota_lock_level`; at run time use `openpulse daemon ota-bounds` / `ota-lock`.
4. To damp oscillation, use `ota_aggressiveness = "conservative" | "balanced" | "aggressive"`, or
   the individual gates `ota_min_backlog` and `ota_upgrade_hold_frames`.
5. The ARDOP TNC floors its adaptive ladder at SL2 (it has no MFSK16), and the host's `ARQBW` caps
   it further.

For a pre-session estimate from a measured SNR, use `openpulse mode-advisor --snr <dB> --profile <name>`.

## 3. Field Deployment Checklist

Complete this checklist before any on-air run:

- Regulatory
  - Verify frequency allocation and emission class for operating region.
  - Confirm station license and operator authorization.
  - Record callsign, control operator, jurisdiction, and legal max power.
  - Set `[station] callsign` (the daemon refuses to start with an empty callsign or `N0CALL`) and
    leave `[station] auto_id_interval_secs` at 600 unless your rules differ.

- RF setup
  - Confirm TX power cap and duty-cycle plan.
  - Verify antenna match and SWR safety.
  - Validate the audio/PTT chain and monitor ALC behaviour (`openpulse calibrate drive` into a dummy load).
  - Measure the rig's key-up time before setting `[modem] ptt_leader_ms` (default 0).

- Operational plan
  - Pre-assign primary and fallback frequencies.
  - Define test windows, operator roles, and abort conditions.
  - Configure logging capture path and timestamp synchronization.

- Identity and traceability
  - Include station ID in operator log header for each session.
  - Preserve raw logs and generated summaries for audit and replay
    (`[observability] audit_mode = true`, then `openpulse audit-bundle`).

## 4. Data Collection Minimums

Target collection per use case:

- Per use case: >= 10 sessions
- Combined total: >= 100 frames
- Per session capture:
  - profile name (`fast` / `robust`)
  - speed levels and modes used
  - channel estimate (SNR class or measured estimate)
  - FER, throughput, median latency, p95 latency
  - retries and level transitions

Example CSV header:

profile_name,session_id,timestamp_utc,mode_start,mode_end,frames_total,frames_ok,fer,throughput_bps,median_ms,p95_ms,snr_est_db,channel_model,retries,fallbacks,notes

`openpulse daemon ota-status` gives a JSON snapshot of the live OTA session for these fields.

## 5. Lab-Only Fallback Validation

If on-air operation is not approved, run loopback/sim validation:

1. Ladder behaviour per profile over a simulated channel:
   ```sh
   cargo run -p openpulse-cli --no-default-features -- adaptive --profile robust --channel watterson-poor-f1 --snr 6 --frames 16
   cargo run -p openpulse-cli --no-default-features -- adaptive --profile fast --channel watterson-good-f1 --snr 18 --frames 16
   ```
2. Benchmark gate:
   ```sh
   cargo run -p openpulse-cli --no-default-features -- --backend loopback --log error benchmark run
   ```
3. Quick matrix baseline:
   ```sh
   cargo run -p openpulse-testmatrix --no-default-features
   ```
4. Throughput benchmark set:
   ```sh
   cargo run -p openpulse-testmatrix --no-default-features -- --bench --bench-frames 50 --bench-payload 223
   ```
5. Real audio path without a radio: the virtual and hardware loopback rigs in the manual
   ([§13.2](openpulse-manual.md#132-test-rigs)).

The test matrix's older `--item8-lab-dataset`, `--cross-mode-gate` and `--item6-gate` options still
run, but their use-case mode ladders predate `hpx_hf` and do not exercise the Release 1 ladder.

Publish outputs under `docs/dev/test-reports/` and compare against prior run reports.

## 6. Validation Report Template

Each use-case report should include:

- Configuration summary: profile, level bounds, payload length.
- Observed metrics: throughput, FER, median latency, p95 latency.
- Predicted vs observed: explain any gap >= 10%.
- Failure analysis: channel condition, decoder failures, retry behaviour.
- Recommendation: keep, change profile, or bound the ladder.

## 7. Exit Criteria

A use case is validated when all are true:

- Profile and any level bounds recorded and frozen for the campaign.
- Field checklist executed for each on-air campaign.
- Data minimums met (>= 10 sessions/use case, >= 100 total frames).
- Validation report published with throughput/FER/latency analysis.
