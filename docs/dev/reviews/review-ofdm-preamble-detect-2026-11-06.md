---
project: openpulsehf
doc: docs/dev/reviews/review-ofdm-preamble-detect-2026-11-06.md
status: resolved
last_updated: 2026-11-06
---

# Review: a level-independent carrier detect for the OFDM rungs (2026-11-06)

Adversarial design review (Fable) of
[`ofdm-preamble-detect.md`](../design/ofdm-preamble-detect.md), before implementing. It is mandatory
twice over: it changes the plugin trait, and it changes when bursts open, and so when the receiver keys
an ACK or NACK.

## Prompt

The prompt asked for falsification, not agreement:
- **the separation:** re-measure `RHO_OPEN` 0.55 against the positives and the negatives;
- **the confirm and the hold:** whether a tone or a BPSK frame can satisfy the CP confirm or hold;
- **the seam and its interactions:** the pre-trigger ring and lead, the onset bound, the noise-floor
  commit, the cold start, the ACK deadline, CSMA and the station-ID quiet clock;
- **the trait:** its shape and its versioning;
- **cost:** the Pi estimate;
- **the 9 open questions** of the note;
- **the gates:** whether each fails without the wiring;
- **the artifact rules:** `## Consumer`, `## Prior art`, `## Twins`, and the asset rule.

It also asked "Does this block Release 1?".

## Verdict

**APPROVE WITH CHANGES.** The primitive is right: a normalised preamble correlation, confirmed on the CP
grid it sets, separates these frames from everything measured. Nine changes were required; all are
applied in the design note.

| # | Required change | Where in the note |
|---|---|---|
| 1 | Gate 8 is vacuous: `veto_membership_pin` registers only bpsk/qpsk/psk8. Register `ofdm-plugin` and keep `MODES_WITH_VETO = ["BPSK250"]`. Publishing `preamble_template` would switch on the veto and `ranked_fallback_onsets`, not change the pin. | Gates 8; What it interacts with |
| 2 | Cost at 800-sample reads is 4.78 ms per read (48 ms/s, 3.2× the table) with the 1 727-sample carry. Carry `SYM_LEN − 1` for ρ, the 5-symbol carry for γ only. Pi ~5–7 ms per tick, measured on air with a per-read duration counter. | State machine step 1; Cost |
| 3 | State the confirm's miss on all frames, not decode-conditioned, in the constants table; gate 3 counts confirm rejections among frames decoded at 9 dB. Keep 0.50. | Constants; gate 3 |
| 4 | The `was_cold` discard (#1254) applies to P-opened bursts today; add a `p_opened` flag read there, and the cold-floor sabotage. | Noise-floor commit; gate 1, S6 |
| 5 | P's burst head must bypass three open sites (`rx_burst_first_block`, the read append, the `seam_s` ring copy); `hold_bound` reads the candidate `(mode, fec)` tuples itself. | Pre-trigger ring; state machine step 3 |
| 6 | Scope is OTA candidates only for Release 1, as a decision. | Where it runs |
| 7 | Correct the clip, the ±128 alias, the impulse score, the release tail and the Pi line. | Preamble reference; corrections; falsifiers; ACK deadline; Cost |
| 8 | Add an end-to-end ACK twin test at AGC level, and run the own-echo rule with an OFDM candidate. | Gates 11, 12 |
| 9 | Drop the unchecked 802.11 citation. | Prior art |

The 9 open questions are answered in the note's "Decisions" section.

**Key measurements** (the reviewer's own probes unless marked):
- **Template:** ndot(OFDM52, OFDM52-64QAM) = 1.000000. Frequency selectivity: 6.25 Hz → 0.908,
  12.5 Hz → 0.660, 31.25 Hz → 0.172. ρ(clipped, unclipped) = 0.9987 (PAPR 12.00 vs 12.69 dB). Self-ρ
  at the 128 alias 0.601.
- **f2g positives:** ρ 0.840–0.880, all best at −12.5 Hz; told offset only 0.602–0.686; γ₄
  0.863–0.926; γ₈ < 0.30 within 6–8 symbols of the end (the note said 5–7).
- **f2g negatives:** 29 859 windows, max 0.474.
- **rx-b-f1d:** 98 opens, ρ 0.679–0.790; negatives max 0.475.
- **rx-a-f1d own echo:** A's own frames at ρ 0.907–0.942.
- **Corpus:** 17 files, max 0.419.
- **Synthetic negatives:** a stronger tone lowers ρ (0.383 → 0.199 from +0 to +40 dB); CW ≤ 0.394;
  impulses (10 clicks/s, +10 dB over hiss) 0.419.
- **Overlap:** two frames overlapping by half at +20 dB: the second re-opens at 0.738.
- **Fade, joint open** (`moderate_f1`, SL7 64 B, 40 seeds): 30/37/40/40/40 at 6/9/12/15/20 dB.
  γ₄ < 0.50 among ρ ≥ 0.55: 8/38 at 6 dB (min 0.342), 1/38 at 9 dB (min 0.493), 0/40 at 12–20 dB.
- **False opens:** over 86 177 windows (5 515 s), 273 at ≥ 0.40, 5 at ≥ 0.45, 0 at ≥ 0.50. About ×55
  per 0.05 step, so ≥ 0.55 extrapolates to about one window per 900 h on this link's mix: a two-point
  tail extrapolation, 2 m only. The confirm multiplies by about 1/3 700 on hiss, but only by about 0.9
  under narrowband traffic.
- **`RHO_OPEN` 0.50** would gain 2/40 frames at 9 dB and 0 at 6 dB, for about ×55 in false opens.
- **Timing (Ryzen 9 3900X):** 17.1 ms/s at the 9-point grid, 3.2 ms/s told offset only; 4.78 ms per
  800-sample read with the carry as first specified.

**Does this block Release 1? Yes.** With AGC FAST required on A, B→A SL7 cannot be gathered, so A2 on
`fast` cannot pass B→A. The maintainer's decision to build before HF rests on nothing the measurements
contradict; HF confirms, with the daemon's false-open counter as the instrument.

## Consumer

See the design note's `## Consumer`: `server::run`'s receive tick → `accumulate_capture` →
`accumulate_routed` → `route_audio_stage(InputCapture)` → `update_dcd_at_seam`. Required changes 4 and 5
add the `was_cold` discard and the three burst-open sites in `accumulate_routed` as consumers of
P's verdict; decision 4 adds the ladder-evidence path (`FlushSpans.p_confirmed`).

## Prior art

See the design note's `## Prior art`: the #1049 veto and #1060 calibration (same correlator, kept
apart), the #1062 stand-down (not adopted), BPSK250's PN-63 template, the acquisition chain's
`EnergyGate`, and the reference modems (modem73/aicodix, ROBUST, codec2 DATAC). The 802.11 citation
was UNCHECKED and is dropped (change 9).

## Twins

See the design note's `## Twins`: the ARDOP TNC and KISS (no OTA session, so P does not run under the
Release 1 scope), the CLI `receive` family, the twin-daemon harness (gates 9 and 11), the own-echo rule
(gate 12), and `channel_still_busy`, which stays on total power.
