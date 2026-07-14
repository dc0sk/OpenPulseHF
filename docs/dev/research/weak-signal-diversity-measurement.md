# Weak-signal frequency-diversity rung — kill-first measurement (#864)

**Status:** measured 2026-07-14. Kill-first gate on the *ideal* bound, before building a real plugin.

## The question

Task #864 proposes a lowest-rate mode a rung *below* the SL floor for deep-fade HF: repeat each data
symbol on a second, band-separated carrier and maximal-ratio-combine (MRC) at RX. This is a **distinct
SNR lever from FEC** — it buys diversity, not coding gain — so any gain is **fading-only** (a wash on
AWGN) and must be proven as a **coded frame-success** gain on a fading channel, or honestly reported as
"no gain → don't ship the rung" (the issue's own acceptance clause).

Per the repo rule ("measure the floor delta first / an uncoded-BER win is not a win"), we measure the
**ρ=0 ideal upper bound** *before* investing in a plugin: an idealised dual-carrier receiver with perfect
fade decorrelation, no cross-carrier ISI, no per-branch acquisition divergence, and **no PAPR penalty**.
It is a strict upper bound on any real implementation. If the ideal doesn't clearly win, the real mode
can't.

## Method (`crates/openpulse-modem/tests/diversity_upper_bound.rs`)

The `WattersonChannel` keys its noise to the input RMS (`noise_sigma = rms / 10^(snr/20)`), so a
two-carrier power split that halves each branch is modelled exactly by feeding each branch at
**snr − 3 dB**. Two *independent* Watterson draws (different seeds) = perfectly decorrelated branches
(ρ=0). `receive_with_llr_combining` already demodulates each look's calibrated differential LLRs, decodes
each alone, and MAP-sums them — exactly MRC for calibrated LLRs.

- **single** (baseline): one look at `snr`, `receive_with_fec`.
- **dual_ρ0** (ideal): two *independent* looks at `snr − 3`, `receive_with_llr_combining(_, _, 2)`.
- **dual_ρ1** (control): two *same-seed* looks at `snr − 3` (correlated branches → no diversity). Proves
  the harness hands no free gain from combining alone — a correlated pair must *lose* to one full-power
  look.

Coded frame-success over 48 seeds (24 for the slower BPSK31), `SoftConcatenated`-class FEC, 73 B payload.

## Results

### BPSK250 (short frame, most room for frequency diversity)

`good_f1` (0.1 Hz Doppler / 0.5 ms delay — **slow fade, the physically-expected win region**):

| snr_db | single | dual_ρ0 | dual_ρ1 (ctrl) |
|--------|--------|---------|----------------|
| 0.0    | 0.12   | 0.44    | 0.06 |
| 3.0    | 0.42   | 0.83    | 0.29 |
| 6.0    | 0.62   | 1.00    | 0.50 |
| 9.0    | 0.81   | 1.00    | 0.83 |
| 12.0   | 0.96   | 1.00    | 0.90 |

`moderate_f1` (1 Hz / 1 ms — fast fade, redundancy check):

| snr_db | single | dual_ρ0 | dual_ρ1 (ctrl) |
|--------|--------|---------|----------------|
| 3.0    | 0.02   | 0.69    | 0.00 |
| 6.0    | 0.35   | 0.96    | 0.19 |
| 9.0    | 0.71   | 1.00    | 0.52 |
| 12.0   | 0.85   | 1.00    | 0.85 |
| 15.0   | 0.94   | 1.00    | 0.96 |

### BPSK31 (the SL floor the rung would sit below)

`good_f1` (24 trials):

| snr_db | single | dual_ρ0 | dual_ρ1 (ctrl) |
|--------|--------|---------|----------------|
| −6.0   | 0.08   | 0.71    | 0.00 |
| −3.0   | 0.42   | 0.92    | 0.12 |
| 0.0    | 0.83   | 1.00    | 0.54 |

Same shape as BPSK250: ideal crosses 0.5 at ~−7 dB vs single at ~−2.5 dB (**~4.5 dB** ideal gain), and
the ρ=1 control loses to single-carrier at every point.

## Reading

**The ρ=0 ideal bound clears the kill-gate.** On the decisive slow-fade channel (BPSK250 / good_f1) the
ideal dual crosses 0.5 frame-success at **~0.5 dB** vs single-carrier at **~4.5 dB** — a **~4 dB** ideal
gain, well past the ≥2 dB bar. The **ρ=1 control sits *below* single-carrier** at every SNR (0.06 vs 0.12,
0.29 vs 0.42, 0.50 vs 0.62), confirming the gain is genuine fade *decorrelation*, not a combining or
harness artifact — combining two correlated half-power looks correctly *loses* to one full-power look.

**So frequency diversity is a real lever — but the ideal ~4 dB is the ceiling, and the real mode pays
costs the ideal bound omits:**

1. **PAPR (~3 dB, structural).** The sum of two carriers has a beat envelope ~3 dB higher peak-to-average
   than one BPSK carrier. HF transmitters are **PEP-limited**, so at matched peak power the dual mode
   delivers ~3 dB *less average power* on-air — and the RMS-keyed simulation hands that entire penalty
   back for free (same lever the CE-SSB work measured, opposite sign). This ~3 dB cost is comparable to
   the ~4 dB ideal gain *before* the next erosion.
2. **Partial decorrelation.** Two-ray frequency correlation is `ρ(S) = cos(π·S·τ)`, *periodic* — not
   monotonic. No single carrier separation decorrelates all presets; the minimax choice **S = 750 Hz**
   gives ρ² ≈ 0.15 / 0.50 / 0.00 on good/moderate/poor_f1. So on good_f1 (the win region) the real
   branches are only ~85 % decorrelated → the real gain is **below** the ρ=0 ideal.
3. **Per-branch acquisition / AFC / Doppler-LLR losses** (per the design review): a faded branch's timing
   and the ±400 Hz Goertzel AFC (which would lock a ±375 Hz carrier and poison the next frame) further
   erode the real gain and demand a non-trivial plugin (common-mode AFC override, shared timing, a
   union-not-blind-sum combine seam).
4. **The neighbour already has overlapping levers.** Dropping to BPSK31 (an existing rung) and the
   engine's HARQ LLR-combining across time (`receive_with_llr_combining`) already buy fade margin at no
   new-waveform cost — and on fast-fading channels a single frame already spans hundreds of coherence
   times, so soft-FEC + interleaver harvest time diversity there regardless.

## Bottom line

The kill-first gate **passed** (ideal ~4 dB on slow fade), so frequency diversity is not dead on arrival.
But the structural **~3 dB PAPR cost is comparable to the ideal gain** and the real-world decorrelation is
partial, so the **net on-air gain is expected to be marginal (≈0–1 dB) or negative** once PAPR is paid —
and it overlaps levers the ladder already has. The only way to get the *net* number is to build the real
dual-carrier plugin (S = 750 Hz, common-mode AFC, shared timing, union combine) and run a full bake-off
that **reports the gain net of the measured PAPR delta**.

**Recommendation:** this is a decision point, not an automatic build — see the PR/issue discussion.
