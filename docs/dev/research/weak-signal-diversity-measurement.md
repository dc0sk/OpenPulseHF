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

## Real-waveform net measurement

`crates/openpulse-modem/tests/diversity_real_waveform.rs` builds the **real** dual-carrier waveform (same
FEC-framed bits on `FC ± 375 Hz`, `SEP = 750 Hz`), passes it through **one** Watterson `good_f1` channel
(so the two carriers see the real, partially-correlated fading + real cross-carrier interference), and
measures the PAPR delta separately. Decode reuses the engine's audio-free union seam
`combine_and_decode_llrs`. Frame-success is matched-average-power by construction (the RMS-keyed channel
normalises the 1/√2 split away); PAPR is scale-invariant, measured on the raw `low + high` sum.

**BPSK250 on good_f1 (48 trials) — ΔPAPR = +2.61 dB:**

| snr_db | single | dual (real) |
|--------|--------|-------------|
| 0.0    | 0.27   | 0.27 |
| 3.0    | 0.54   | 0.73 |
| 6.0    | 0.73   | 0.94 |
| 9.0    | 0.90   | 0.98 |
| 12.0   | 0.96   | 1.00 |

**BPSK31 on good_f1 (24 trials) — ΔPAPR = +2.60 dB:**

| snr_db | single | dual (real) |
|--------|--------|-------------|
| −6.0   | 0.17   | 0.58 |
| −3.0   | 0.67   | 0.96 |
| 0.0    | 0.88   | 1.00 |
| 3.0    | 1.00   | 1.00 |

**Matched-average-power frequency-diversity gain** (horizontal shift): ~1 dB at the 0.5 crossing for
BPSK250 (growing to ~3–4 dB at high reliability), ~2.6 dB at the crossing for BPSK31 (to ~4 dB at high
reliability). The real waveform loses ~2 dB vs the ρ=0 ideal — exactly the partial decorrelation
(ρ² ≈ 0.15 at S = 750 on good_f1) + cross-carrier ISI predicted.

**PAPR cost:** the two-tone beat raises PAPR from 1.44 dB (single BPSK) to 4.05 dB — **ΔPAPR ≈ +2.6 dB**,
paid as reduced average power on a PEP-limited HF transmitter.

## Bottom line — measured, do not ship the rung

**Net on-air gain ≈ (matched-power gain) − ΔPAPR ≈ break-even** (−0.5 to +1 dB depending on baud and
target reliability). The ~2.6 dB PAPR cost consumes almost the entire real matched-power diversity gain.

The kill-first *ideal* cleared the gate, but the real waveform does not survive its own PAPR: the
frequency-diversity rung buys essentially nothing net on-air that the ladder does not already have more
cheaply —

- **dropping the baud** (BPSK250 → BPSK31) buys ~6–7 dB of margin at 8× airtime, far more than the
  diversity rung's ~0–1 dB net at 2× bandwidth;
- **HARQ time-diversity** (`receive_with_llr_combining` across retransmissions) already harvests
  independent fade states at no new-waveform cost.

**Recommendation: do not ship a frequency-diversity rung.** Close #864 as *measured — net too marginal to
justify a new waveform, 2× the occupied bandwidth, and the plugin complexity (common-mode AFC, shared
per-branch timing, union seam)*. The reproducible measurements (`diversity_upper_bound.rs`,
`diversity_real_waveform.rs`) and the reusable `combine_and_decode_llrs` engine seam remain; revisit only
if a use case appears where the ~1 dB high-reliability net and 2× bandwidth are both acceptable and the
PAPR can be recovered (e.g. a constant-envelope diversity waveform rather than a two-tone sum).
