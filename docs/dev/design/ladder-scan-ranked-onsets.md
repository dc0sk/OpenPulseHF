---
project: openpulsehf
doc: docs/dev/design/ladder-scan-ranked-onsets.md
status: review
last_updated: 2026-11-03
---

# Rank the ladder scan's onsets before the walk

Work plan M3, A2. Found on air in task 19, run a2d; reproduced offline on 2026-11-03.

## Problem — measured

The OTA burst scan (`ota_decode_and_ack_inner`, the onset-major scan block,
`ota-scan-onset-major.md`) tries each ladder candidate at every onset from the burst's start, in steps
of `step` (128 samples at BPSK63). When a burst opens before its frame, the walk only reaches the frame
after all the onsets in between.

- **The run:** task 19, run a2d. Station B (Pi 4) received A's SL3 (BPSK63) frames. Eight in a row
  finished their decode past the ACK deadline (about 6.2 s after the frame's end, since `4fd5a13`), so
  none was answered and the ladder held at SL3 for 6.5 minutes.
- **The replay:** B's recording (`rx-b-a2d`), the first SL3 frame (ending at 300 s). The burst opened
  about 0.7 s before the frame, and the frame starts 8448 samples in. The walk reached it at its 66th
  onset: **68 attempts, 2.86 s on x86**, which is about 11 s on a Pi 4.
- **The same shape in task 18, run a2a:** a decode 8.8 s after the frame, which then was a late ACK
  and a desync.
- **For contrast, task 18's b8:** the Pi 4 decoded SL3 in 2.8 s, with the frame at the burst's start.

The uncoded fallback already ranks its onsets by the preamble's correlation (`ranked_fallback_onsets`,
`fallback-onset-ranking.md`). But only BPSK250 publishes a preamble template (`bpsk-plugin`,
`preamble_template`: its constants were derived for BPSK250 and do not transfer), so that ranking does
not reach BPSK63 or BPSK100.

## Decision

Before the walk, each ladder candidate tries a short ranked list of onsets, at each of its passes. The
walk then goes on, in its own order, without those entries.

1. **The preamble's correlation peaks,** where the mode publishes a template (BPSK250): the same
   ranking and K (4) as the uncoded fallback.
2. **Where the burst's energy rises to the frame's level** (`energy_rise_onsets`), for every mode:
   - take the first `step`-sample window whose RMS reaches `ENERGY_RISE_FRACTION` (0.5) of the burst's
     80th-percentile window RMS, and holds it for the next window;
   - try that onset, then one and two steps either side.

   No constants per mode. A frame fills most of a burst, so its windows sit near the 80th percentile,
   and noise or a flicker that opened the burst sits well below half of that.

Offset 0 at `afc_before` was attempt 0 and is not repeated. A burst whose frame starts at offset 0
gets at most five extra attempts near offset 0, and the walk's order is otherwise unchanged.

## Measured (x86, release, the replay)

- **a2d, the first SL3 frame:** 68 → 3 attempts, 2.86 → 0.40 s.
- **Every recording at hand, replayed with and without the ranking** (the same build, the ranking
  switched off for the comparison): the same frames decode on every one, and the total decode time
  drops.

| Recording | Frames decoded (without, with) | Total decode time, without → with |
|---|---|---|
| `rx-a-g2` | 2, 2 | 20.0 → 14.9 s |
| `rx-b-b2` | 4, 4 | 11.6 → 5.1 s |
| `rx-a-g3` | 2, 2 | 25.0 → 17.6 s |
| `rx-b-b3` | 3, 3 | 48.1 → 46.6 s |
| `rx-b-b4` | 4, 4 | 15.4 → 8.7 s |
| `rx-b-b5` | 0, 0 | 32.7 → 31.8 s |
| `rx-b-a2a` | 4, 4 | 187.7 → 187.1 s |
| `rx-b-a2d` | 4, 4 | 208.3 → 203.4 s |

The two A2 replays barely change in total: their time is spent on bursts that fail either way, after the
replay's receiver, which hears no ACK of its own, has moved its ladder away from the sender's.

## Risks and how each is checked

- **A wrong rise point** (a fade inside the frame, a burst opening on a strong interferer) costs at
  most five attempts before the walk goes on as before. It cannot lose a frame the walk would find.
- **Several frames in one burst:** the rise finds the first. The walk and `decode_following_frames` go
  on as before.
- **The cost of the ranking itself:** one RMS pass over the burst per candidate, negligible against
  one decode attempt.

## Tests

- `ladder_scan_ranked_onsets` (`openpulse-modem`): an SL3 frame behind 0.7 s of noise that opens the
  burst, through `accumulate_capture` and `ota_decode_burst`, decodes in **2 attempts** (budget 12).
  Sabotage, with the energy-rise ranking removed: **66 attempts**, and it fails.
- The replay A/B above.

## Consumer

`ota_decode_and_ack_inner`'s scan block (`crates/openpulse-modem/src/engine.rs`), reached from the
daemon's OTA receive (`nack_budget::decode_ota_burst` → `ModemEngine::ota_decode_burst`). Found with
`grep -n "attempts.sort_unstable" crates/openpulse-modem/src/engine.rs`.

## Prior art

- `ranked_fallback_onsets` and `pick_separated_peaks` (the uncoded fallback's preamble ranking, #1454,
  `fallback-onset-ranking.md`);
- `onsets_nearest` (the fallback rescue);
- the trigger point `lead`.

`grep -rn "energy_rise\|rise_onset" crates/` found nothing before this change.

## Twins

- **Phase 2's ladder entries** (`acquire_burst_correction_span`, then the walk) have the same shape.
  UNCHECKED whether they need the ranking too: in the a2d replay the frame decoded in the scan block
  once ranked.
- **The non-OTA `decode_burst` path** (CLI, monitor) is UNCHECKED.
