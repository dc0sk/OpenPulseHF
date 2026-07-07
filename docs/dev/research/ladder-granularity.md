---
project: openpulsehf
doc: docs/dev/research/ladder-granularity.md
status: living
last_updated: 2026-07-08
---

# Adaptive ladder: granularity + agility — research

How to make the adaptive rate/mode ladder finer (fewer throughput cliffs) and more agile (faster,
smoother, fewer retries). From a research pass over `profile.rs`, `rate.rs`, `ota_rate.rs`,
`rate_policy.rs`, `harq.rs`, `fec.rs`, and the plugin mode lists. No code was changed.

## Current `hpx_hf` ladder and its gaps

Net bps = gross × code rate; SC = SoftConcatenated ≈ 0.437.

| SL | Mode | FEC | net bps | floor | ceiling |
|----|------|-----|---------|-------|---------|
| 2 | BPSK31 | — | 31 | 3 | 8 |
| 3 | BPSK63 | — | 62 | 4 | 9 |
| 4 | BPSK250 | — | 250 | 5 | 11 |
| 5 | QPSK250 | — | 500 | 9 | 12 |
| 6 | QPSK500 | — | 1000 | 11 | 13 |
| 7 | 8PSK500 | Rs | 1312 | 12 | 15 |
| 8 | SCFDMA52-8PSK | SC | 1895 | 14 | 17 |
| 9 | SCFDMA52-16QAM | SC | 2527 | 16 | 21 |
| 10 | SCFDMA52-32QAM | SC | 3159 | 17 | 23 |
| 11 | SCFDMA52-64QAM | SC | 3790 | 22 | — |

**Gaps / cliffs:** SL3→SL4 is a **4× throughput cliff** at 1 dB apart; SL4→SL5 is a **4 dB dead zone**
stuck at 250 bps; SL10→SL11 is a **5 dB gap**; nothing above 22 dB (3.8 kbps peak while the channel
could carry ~2× with lighter coding); no coded weak-signal rung below 3 dB.

**Under-used mode strings** (registered in plugins, in no profile): `BPSK100`, `BPSK250-RRC`,
`QPSK125`, `QPSK1000-HF(-RRC)`, `8PSK1000-HF(-RRC)`, `SCFDMA52-64QAM-P4` (denser pilots),
`SCFDMA26-8/16/32QAM` (~1 kHz, FDE-robust), and the `PILOT-*2000-RRC` family.

## Proposed finer `hpx_hf` ladder

All rungs ≤ ~2.7 kHz; `SpeedLevel` already spans SL1–SL20, so there is headroom.

| SL | Mode | FEC | ~net bps | ~floor | note |
|----|------|-----|----------|--------|------|
| 2 | BPSK31 | Rs *(new)* | 27 | ~1.5 | coded weak-signal rung before chirp — recalibrate |
| 3 | BPSK63 | — | 62 | 4 | unchanged |
| 4 | **BPSK100** | — | 100 | ~4.5 | existing, unused — breaks the 4× cliff |
| 5 | BPSK250 | — | 250 | 5.5 | shifted |
| 6 | **QPSK250** | **Rs** *(new MODCOD)* | 437 | ~7.5 | fills the 5→9 dead zone |
| 7 | QPSK250 | — | 500 | 9 | unchanged |
| 8 | QPSK500 | — | 1000 | 11 | unchanged |
| 9 | 8PSK500 | Rs | 1312 | 12 | unchanged (SL7 gap-filler already reassessed) |
| 10 | **SCFDMA26-32QAM** | SC | 1579 | 13 | existing; FDE-robust, ~1 kHz; floor known from `hpx_wideband_hd` |
| 11 | SCFDMA52-8PSK | SC | 1895 | 14 | unchanged |
| 12 | SCFDMA52-16QAM | SC | 2527 | 16 | unchanged |
| 13 | SCFDMA52-32QAM | SC | 3159 | 17 | unchanged |
| 14 | **SCFDMA52-64QAM-P4** | SC | 3571 | ~19–20 | existing, unused — denser pilots split the 17→22 gap |
| 15 | SCFDMA52-64QAM | SC | 3790 | 22 | unchanged |
| 16 | **SCFDMA52-16QAM** | **LdpcHighRate** | 5137 | ~24 | new MODCOD rung, no new modulation |
| 17 | **SCFDMA52-64QAM** | **LdpcHighRate** | 7705 | ~27 | doubles peak throughput on excellent channels |

**One genuinely new mode worth adding:** `SCFDMA26-QPSK` (26 SC, QPSK, ~631 net, ~1 kHz, est. floor
~7 dB) — a *fading-tolerant* FDE rung in the 7–11 dB band where today only equalizer-less
single-carrier modes live (which fail moderate_f1 outright). The plugin already has 26-SC plumbing, so
it is a constellation-table addition.

> Any mode/FEC row change alters `SessionProfile::fingerprint()` (an intentional interop version break);
> floor changes alone do not.

## FEC granularity (MODCOD)

The FEC family already spans code rates **1.0 / 0.889 / 0.875 / 0.749 / 0.5 / 0.437 / 0.333** — ~6–8 dB
of effective rungs per modulation — yet `hpx_hf` uses only three (`None`, `Rs`, `SoftConcatenated`), and
most profiles use `fec_modes: [None; 21]`.

- **SL2–SL6 are uncoded** → slow-fading-only per the harness. `Ldpc` on QPSK250/BPSK250 (as `hpx_modcod`
  already does) buys lower floors *and* moderate_f1 survivability.
- `RsStrong` (0.749) is a natural intermediate rung between `Rs` and `SoftConcatenated`, currently used
  only by HARQ retries, never as a ladder MODCOD.
- The pilot ladders have 5–6 dB inter-rung gaps that MODCOD interleaving would halve with zero DSP work.
- `HarqPolicy` grades FEC by SNR/fading per retry but is disconnected from the ladder's per-rung
  `fec_modes` — unify so the rung defines the base FEC and HARQ only escalates, preventing the two from
  fighting.

## Agility — assessment + fixes

Already good: OTA fast-downshift is multi-step and SNR-directed; up is asymmetric single-step,
ceiling-gated; the lockstep candidate set makes downshifts desync-safe; A2/A3 gates + `ack_up_requires_
snr_candidate_at` guard the dense rungs; Memory-ARQ soft combining exists.

Weaknesses → fixes:

- **Raw instantaneous SNR drives all decisions** → one noisy estimate triggers an immediate (multi-rung,
  in OTA) drop. **Add an EWMA + variance tracker**; require the *smoothed* SNR (or 2 consecutive samples)
  below floor unless the breach is > 3 dB; scale the up-margin by k·σ (flat channels climb fast,
  fluttery ones hold).
- **No per-rung failure memory → re-climb oscillation.** **Add an OLLA outer loop**: a per-rung dynamic
  floor offset, +Δ (~1.5 dB) on failure at that rung, decaying with time/successes — self-heals
  mis-calibrated floors on-air without re-running the harness.
- **Legacy `RateAdapter` lacks multi-step downshift** — port `level_for_snr` from `OtaRateController` or
  retire the legacy path onto it.
- **Slow recovery after deep fades** (1 rung per decoded frame) → allow a 2-step recommendation when
  smoothed SNR also clears the next rung's ceiling (candidate set grows to 3, bounded), or use a short
  sender "probe frame".
- **Inconsistent hysteresis** — ceiling−floor(next) is +4 dB at SL2/3/9 but +1 dB elsewhere; the +4 dB
  rungs over-dwell at the lowest throughput. **Normalize to `ceiling(L) = floor(L+1) + 2 dB`**
  (local-only, fingerprint-safe).
- **A3 hold counts AckUp attempts, not time** — make it frame-/time-based.
- **NACK threshold ignores soft combining** — keep 3 only when combining is active, else 2; let a NACK
  whose SNR *is* below floor bypass the counter (OTA already does; legacy doesn't).
- **`RateTrigger::SnrCeiling` is never emitted** — upgrade candidacy is invisible to host telemetry.

## Do next (ranked)

1. **Fill `hpx_hf` gaps with existing modes + 2 MODCOD rungs** (BPSK100, QPSK250+Rs, SCFDMA26-32QAM,
   SCFDMA52-64QAM-P4, 16/64QAM+LdpcHighRate) — biggest throughput-vs-SNR smoothing for near-zero DSP.
   **M; requires calibration re-run** per new (mode, FEC) pair.
2. **OLLA per-rung dynamic floor offset in `OtaRateController`** — kills re-climb oscillation, self-heals
   stale floors. **M; no harness re-run** (runtime-only).
3. **SNR smoothing (EWMA + σ-scaled margin) before floor/ceiling checks** — stops single-sample
   multi-rung drops; cheapest agility win. **S; no re-run.**
4. **Normalize hysteresis to floor(L+1)+2 dB across `hpx_hf`** — faster climbs off the 31/62 bps rungs;
   local-only, fingerprint-safe. **S.**
5. **FEC on the low rungs (Ldpc/Rs on SL2–SL6)** for moderate_f1 survivability + ~3 dB lower floors.
   **M; requires the Watterson calibration re-run.**
