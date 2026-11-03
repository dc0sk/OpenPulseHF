---
project: openpulsehf
doc: docs/dev/design/ladder-scan-ranked-onsets.md
status: accepted
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
  onset: **68 attempts, 2.4–2.9 s on x86** (three replays), which is about 11 s on a Pi 4.
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

   No constants per mode. **Fitted to one burst** (a2d's first SL3 frame, the review's measurement): the
   opener's windows peak at 0.43 of the P80, the frame's sit at 0.68–0.75 (the PN transitions dip the
   RMS). Every fraction from 0.4 to 0.9 finds the frame's edge there; 0.3 finds the opener. So 0.5 has
   about 1.3 dB of margin below and 2.7 dB above.

   **Its regime is a strong signal** (2 m, A2). The energy gate opens at three times the floor, so an
   opener is at least 4.8 dB above it; a frame near its rung's cliff is then not 6 dB above the opener,
   and the rise lands on the opener. That case costs the ranked attempts and then the full walk, as
   before.

Offset 0 at `afc_before` was attempt 0 and is not repeated. The preamble peaks are tried in time order,
not correlation order, so the earliest frame comes first if a keying ever holds two (#1461).

**The bound on extra attempts.** The energy-rise entries lie on the walk's grid and are only reordered.
They cost extra attempts only when the frame decodes later in the walk than they would have run: at
most 5 onsets × 2 passes × 2 candidates = 20. The preamble peaks (BPSK250 candidates) are sample-exact,
off the grid, so they are additions: up to 4 × 2 more per BPSK250 candidate. That case is unmeasured;
no recording at hand has a BPSK250 ladder frame.

## Measured (x86, release, the replay)

- **a2d, the first SL3 frame:** 68 → 3 attempts, 2.86 → 0.40 s.
- **Every recording at hand, replayed with and without the ranking** (the same build, the ranking
  switched off for the comparison by a temporary environment switch): the same frames decode on every
  one, and the total decode time drops. The replay harness and the switch were scratch code at
  `87ce9b6` and are not in the tree: the recordings are not in the repository, so the table cannot be
  re-run from it.

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

On every a2d frame, the ranked decode lands one or two steps (128–256 samples) later than the walk's
first decodable onset, so the SNR window of #1142 (`decoded_span`) shifts by up to two symbols. The ACK
decisions were the same in the A/B.

The two A2 replays barely change in total: their time is spent on bursts that fail either way, after the
replay's receiver, which hears no ACK of its own, has moved its ladder away from the sender's.

## Risks and how each is checked

- **A wrong rise point** (a fade inside the frame, a burst opening on a strong interferer, a frame
  near its cliff) costs the ranked attempts (the bound above) before the walk goes on as before. It cannot lose a frame the walk would find.
- **Several frames in one burst:** the rise finds the first. The walk and `decode_following_frames` go
  on as before.
- **The cost of the ranking itself:** one RMS pass over the burst per candidate, negligible against
  one decode attempt. With a BPSK250 candidate, the preamble profile is computed twice per burst (the
  fallback's block and this one); accepted.

## Tests

- `ladder_scan_ranked_onsets` (`openpulse-modem`): an SL3 frame behind 0.7 s of noise that opens the
  burst, through `accumulate_capture` and `ota_decode_burst`, decodes in **2 attempts** (budget 12).
  Sabotage, with the energy-rise ranking removed: **66 attempts**, and it fails.
- `an_sl3_frame_behind_a_loud_opener_is_found_where_the_energy_rises`: the same, with the opener at 0.4
  of the frame's RMS (a2d's level): **2 attempts**. Sabotage, `ENERGY_RISE_FRACTION` = 0.3: **107
  attempts**, and it fails. This pins the constant from below.
- `energy_rise_onsets_edges` (unit): a rise at window 0, past the scan's end, a burst too short to rank,
  and a flat one.
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
