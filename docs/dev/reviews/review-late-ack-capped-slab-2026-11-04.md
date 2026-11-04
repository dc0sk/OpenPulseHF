---
project: openpulsehf
doc: docs/dev/reviews/review-late-ack-capped-slab-2026-11-04.md
status: resolved
last_updated: 2026-11-04
---

# Review: no ACK for a frame that ended past its window in a capped slab (2026-11-04)

Adversarial review (Fable) of `8177910`. This is the mandatory review for a change to when the
receiver keys. The design is finding K of `docs/dev/design/dcd-raised-floor.md`.

## Prompt

The prompt asked for falsification, not agreement:
- the frame-end bound on every path that records a decoded span (offset 0, the scan, phase 2, HARQ);
- whether the test can pass for the wrong reason;
- the lockstep after a withheld ACK;
- the capped flag's lifetime;
- the no-deadline case.

It also asked "Does this block Release 1?".

## Verdict

**It does not block Release 1.** The change never keys more than before. But the first version left
finding K open on the rungs where the capped regime lives.

## Findings and what was done

| # | Finding | Done |
|---|---|---|
| S1 | The frame end was bounded by the slice reserve (`frame_plan(raw_max, fec)`), 2–2.5× the frame. Late ACKs still went out: by up to 5.2 s at SL6, 10.5 s at SL5, and ≈83 s at SL2, where one lands in the window after next | **Taken.** The frame end is now the slice start plus `tx_airtime_seconds(payload, mode, fec)`, the decoded payload's own coded airtime. The reviewer's `+ acq_window` is left out: without it the end is earlier and the tail longer, which is the strict direction |
| S2 | The test could pass through the ordinary late path (a slow decode on a loaded runner), and it did not probe the loose band | **Taken.** Every case asserts that the decode finished before its deadline. A new case sets the window to the true tail − 2 s: answered under the old bound, not under the new one. **Sabotage:** restoring the slice-reserve bound fails `the_frames_end_is_its_airtime_not_its_slice_reserve` (1 failed, 2 passed) |
| N1 | The sender's window opens after the ride-along ID, which the receiver cannot see, so the rule is strict by the ID's airtime (~1.1 s at BPSK250), only on ID-carrying frames | Recorded; the safe direction |
| Q3 | Lockstep: the same as the late path. No ACK, no ladder move, the NACK budget untouched. In a stuck-cap regime the sender now abandons instead of making wrong progress | Confirmed; nothing to do |
| N4 | The fallback's early returns leave `last_flush_capped` set; a later hand-built burst reaches the rule with tail 0 | Not taken: benign, as the review says |
| Q5 | No deadline (not real-time audio) → unchanged | Confirmed |
| N3 | No ledger entry; the task 22 method rewrite was bundled into the keying commit | The ledger entry is added with the fix; the bundling stays in `8177910` (force push is not available) |

## Not covered

- A frame deep in a capped slab (25 s, 31.5 s in): in the synthetic slab it does not decode at all,
  because the energy rise fires at the hiss onset. That position is untested.
- Whether a late ACK actually marked a later frame delivered in a2g: UNCHECKED.
