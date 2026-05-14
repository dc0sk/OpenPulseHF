---
title: "OpenPulseHF Use-Case Deployment Guide"
last_updated: "2026-05-14"
status: "draft"
---

# OpenPulseHF Use-Case Deployment Guide

This guide defines deployment profiles and validation steps for Item 8:
- field_relay (high fading)
- emergency (low SNR tolerance)
- station_relay (high BER margin)

It supports both:
- field/on-air validation (when regulatory approval is available), and
- lab-only fallback validation (no RF transmission required).

## 1. Use-Case Profiles

| Use case | Target environment | SNR operating band | FER target | Latency target | Preferred ladder | Fallback ladder |
|---|---|---:|---:|---:|---|---|
| field_relay | Mobile/portable relay, variable fading, intermittent QRM | 10-20 dB | <= 10% | p95 <= 2000 ms | QPSK500 -> QPSK1000-HF -> SCFDMA52 | BPSK250 -> QPSK500 |
| emergency | Low power, weak-signal, high reliability priority | 6-14 dB | <= 5% | p95 <= 2500 ms | BPSK250 -> QPSK500 (RS/RS-IL) | BPSK100/BPSK250-RRC |
| station_relay | Fixed stations, stronger links, throughput priority | 18-30 dB | <= 3% | p95 <= 1500 ms | QPSK1000-HF -> 8PSK1000-HF -> SCFDMA52-64QAM-P4 | QPSK500 -> QPSK1000-HF |

Notes:
- SCFDMA52-64QAM-P4 requires high SNR margin and should be treated as a high-quality-link mode.
- For bursty fading (Watterson/G-E), favor RS or RS-Interleaved over no-FEC for emergency traffic.

## 2. Mode Selection Guidance

Use this operating rule during live sessions:

1. Start in a conservative mode for the profile.
2. Require two consecutive successful frames before stepping up.
3. Step down immediately after one hard decode failure in emergency profile.
4. In field_relay and station_relay, allow one retry before stepping down.
5. If p95 cycle time exceeds target for three consecutive windows, step down one rung.

Recommended profile starts:
- field_relay: QPSK500 + RS
- emergency: BPSK250 + RS-Interleaved
- station_relay: QPSK1000-HF + RS

## 3. Field Deployment Checklist

Complete this checklist before any on-air run:

- Regulatory
- Verify frequency allocation and emission class for operating region.
- Confirm station license and operator authorization.
- Record callsign, control operator, jurisdiction, and legal max power.

- RF setup
- Confirm TX power cap and duty-cycle plan.
- Verify antenna match and SWR safety.
- Validate audio/PTT chain and monitor ALC behavior.

- Operational plan
- Pre-assign primary and fallback frequencies.
- Define test windows, operator roles, and abort conditions.
- Configure logging capture path and timestamp synchronization.

- Identity and traceability
- Include station ID in operator log header for each session.
- Preserve raw logs and generated summaries for audit and replay.

## 4. Data Collection Minimums

Target collection for Item 8 acceptance:

- Per use case: >= 10 sessions
- Combined total: >= 100 frames
- Per session capture:
- profile_name
- mode ladder used
- channel estimate (SNR class or measured estimate)
- FER, throughput, median latency, p95 latency
- retries and fallback transitions

Example CSV header:

profile_name,session_id,timestamp_utc,mode_start,mode_end,frames_total,frames_ok,fer,throughput_bps,median_ms,p95_ms,snr_est_db,channel_model,retries,fallbacks,notes

## 5. Lab-Only Fallback Validation

If on-air operation is not approved, run loopback/sim validation:

1. Run quick matrix baseline:
   cargo run -p openpulse-testmatrix --no-default-features
2. Run throughput benchmark set:
   cargo run -p openpulse-testmatrix --no-default-features -- --bench --bench-frames 50 --bench-payload 223
3. Run Item 7 cross-mode gate:
   cargo run -p openpulse-testmatrix --no-default-features -- --cross-mode-gate --bench-frames 50 --bench-payload 223
4. Run Item 6 HARQ-rate gate:
   cargo run -p openpulse-testmatrix --no-default-features -- --item6-gate --bench-frames 50 --bench-payload 223

Publish outputs under docs/test-reports/latest and compare against prior run reports.

## 6. Validation Report Template

Each use-case report should include:

- Configuration summary: profile, mode ladder, FEC policy, payload length.
- Observed metrics: throughput, FER, median latency, p95 latency.
- Predicted vs observed: explain any gap >= 10%.
- Failure analysis: channel condition, decoder failures, retry behavior.
- Recommendation: keep, tune thresholds, or lower mode ceiling.

## 7. Exit Criteria for Item 8

Item 8 is complete when all are true:

- Use-case profiles approved and frozen.
- Field checklist executed for each on-air campaign.
- Data minimums met (>= 10 sessions/use case, >= 100 total frames).
- Validation report published with throughput/FER/latency analysis.
