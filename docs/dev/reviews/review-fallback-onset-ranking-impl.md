---
project: openpulsehf
doc: docs/dev/reviews/review-fallback-onset-ranking-impl.md
status: resolved
last_updated: 2026-10-05
---

# Review: fallback onset ranking, implementation (2026-10-04)

Adversarial review (Fable) of the implementation of `docs/dev/design/fallback-onset-ranking.md`,
commits `394dfc31` and `553d3bb8` as reviewed (rebuilt as `c5c3446b` with the same tree). The design
itself was reviewed on 2026-10-02. Read-only: no build, no file changes, no state-changing git.

## Prompt

Falsify, do not agree. Read `git diff 9c40f062..HEAD` and the design. Apparatus sent with it: the K
measurement (`fallback_onset_rank_measurement::measure_fallback_onset_ranks`, 226 decodable frames
in real idle recordings, all rank 0, and the harness bug already fixed), the cost probe
(`receive_cost_scaling`, x86, `PROBE_READ=4096`), the production-path test and its K = 0 sabotage,
and the fact that no real on-air frame decodes against the current wire format. Questions:

1. Can a frame that decoded before now fail, or be classified differently (ladder vs non-ladder, ACK
   vs none)? Ranks ≥ K behind the coded scan, `fallback_is_a_candidate` profiles, template-less
   modes, short bursts, the DDC arm, multi-frame bursts (#1461), AFC restore, HARQ ordering.
2. Is the K apparatus sound and does its regime match production? Is ±16 samples equivalent to
   "the ranked decode succeeds"? What input would falsify K = 4?
3. DSP: energy floor, rotation sign, profile range against `scan_end`, separation against the
   period-4 preamble's aliases, ties.
4. Does the counter make K observable on air?
5. Does this block Release 1? If not, say so. Classify each finding blocking / should-fix / park.

## Verdict

**Does not block Release 1.** Ordering, AFC restore, HARQ ordering and ladder/ACK classification hold
for a single frame. Findings, most severe first:

1. **Should-fix — multi-frame burst (#1461).** Ranked onsets were attempted in ρ order; a keying of
   two control frames carries one identical preamble each, so a better-correlating second frame was
   decoded first and `decode_following_frames` ran forward from it: the first frame was lost, not
   delayed. **Fixed:** attempts run earliest first, the counter keeps the correlation rank.
   `the_first_of_two_frames_in_one_keying_is_not_lost` (second frame 6 dB louder) fails with the
   sort removed (`FRAG B` delivered, `FRAG A` lost).
2. **Should-fix — separation.** The preamble's symbols run `++--`; ρ is a magnitude, so the copies
   ±2 and ±4 symbols off the peak score ≈0.94 and ≈0.87 of it and a one-symbol separation filled
   ranks 1–3 with the same frame. **Fixed:** peaks at least one preamble span apart.
3. **Park — K regime.** `ONSET_BOUND` was pinned to a 4 096-sample read. **Done in this PR:**
   re-measured at 12 288 and 49 152; all 449 decodable frames ranked first. Parked: a tone at
   fc ± baud/4 stronger than the frame (the notch normally removes it first).
4. **Park — observability.** Rank hits were `debug!`. **Done:** rank ≥ 1 logs at `info`, a miss at
   `warn`. Parked: the counter in daemon diagnostics.
5. **Park — scope.** Only BPSK250 publishes a template; a station on BPSK31/63/100 keeps the
   exhaustive scan. Recorded in the design and the work plan.

Checked and found sound: classification and the `fallback_is_a_candidate` gate; short bursts and
template-less modes fall to the old path unchanged; AFC restore on every failure; both fallback stages
before phase 2 and HARQ; DSP rotation sign and ρ identical to `search_normalized_over_frequency`
(pinned by a unit test); profile range equal to the exhaustive scan's; ties deterministic.

## Consumer

`ota_decode_and_ack_inner` → `ranked_fallback_onsets` / `decode_at_ranked_onsets` → the deferred
`decode_burst_phase1`, reached on the daemon receive path through `server.rs`'s
`ota_decode_burst(&burst, …, Some(&mode))`. Found by
`grep -n "ota_decode_burst\|ranked_fallback_onsets\|decode_at_ranked_onsets" crates/openpulse-modem/src/engine.rs crates/openpulse-daemon/src/server.rs`.

## Prior art

`PreambleVeto`, `preamble_rho`, `preamble_search_plan` (the matched filter and grid reused here) and
`IqMatchedFilter::rho_profile` (single-frequency profile); #1118 and #1138 set the fallback and scan
order this changes. Found by
`grep -n "fn preamble_rho\|fn preamble_search_plan\|fn rho_profile" crates/openpulse-modem/src/engine.rs crates/openpulse-dsp/src/acquisition.rs`.

## Twins

`decode_burst_inner` (the CLI and TNC entry) runs the same exhaustive phase-1 scan on its own bursts;
it is off the daemon's per-frame path and is unchanged. `respond_arq_ota` passes no fallback mode, so
it is unchanged too (its suite, `ota_channel_adaptation`, ran 3/3 in 1 657 s against 1 664 s before).
Found by `grep -n "scan_burst_onsets(\|ota_decode_and_ack(" crates/openpulse-modem/src/engine.rs`.
