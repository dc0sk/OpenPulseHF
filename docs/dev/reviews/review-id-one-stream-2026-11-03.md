---
project: openpulsehf
doc: docs/dev/reviews/review-id-one-stream-2026-11-03.md
status: resolved
last_updated: 2026-11-03
---

# Review: the sender's station ID shares its data frame's audio stream (2026-11-03)

Mandatory adversarial review (Fable) of `84456d8`: it changes how the sender keys (two frames in one
output stream inside one PTT hold) and when the station ID counts as sent (§97.119, REQ-REG-10).

## Prompt

The prompt asked for falsification, not agreement. It asked about:
- keying safety;
- §97 logging;
- state leaks (nesting, a panic, emit paths that bypass the seam);
- the junction between frames;
- the receiver's timing and its uncoded fallback;
- the moved log line;
- whether the receiver's ID-before-ACK is a copy of the same bug;
- test quality.

It also asked "Does this block Release 1?".

## Verdict

**Does not block Release 1** (the bug costs about 1 % of frames one retransmission). Two should-fixes
change what the fix claims, and one more guards the state. Checked and found sound:
- PTT is held for one drain instead of two;
- every frame is recorded once with its own sequence;
- CE-SSB and the limiter are stateless per frame;
- the 9 s window shifts the same way on both sides;
- the receiver's ID-before-ACK is not a copy of the bug: the sender's ACK listener scans in one stream.

## Findings and what was done

| # | Finding | Done |
|---|---|---|
| S1 | `id_sent` meant "queued": a burst whose write failed logged the ID, reset the ID clock, and lost the ID | `id_sent` follows the burst's result; a burst that was not written keeps the ID for the next attempt. Test `id_after_a_failed_write` (daemon): the output refused once, the ID is logged once, after the failure. Sabotage (the old rule): it is logged before the failure, and the test fails |
| S2 | The receiver's uncoded fallback might decode the trailing ID and return early with no ACK, when the frame is not at the burst's start | Falsifier run: SL5 and SL6, leads of 0, 0.25 and 0.7 s, first reads of 800 to 20 000 samples, with and without the ID. **Never the ID; identical with and without it.** But with a lead and 800-sample reads, SL5 and SL6 decoded **nothing**, with or without the ID. That was the ladder scan's energy-rise onsets, clipped to the walk's `scan_end` (`87ce9b6`). Fixed: they reach the burst's end; 24 of 24 decode with an ACK. Pinned by `ladder_frame_behind_a_lead` (modem); sabotage (clipped again) fails |
| S3 | No guard: a nested burst discards the outer one; a panic leaves the burst open and holds every later frame | A nested burst is refused; a panic clears the burst before unwinding. Tests in `transmit_burst` |
| N | `transmitting N byte frame` and `FrameTransmitted` now precede the burst's audio; the burst's `logged TX metadata` lines are the time | Noted for task reports |
| N | `onair-a2-mode-check.py` would classify a data frame whose window ends with the ID | A line followed within 0.1 s by another is skipped; a2f still agrees 241/241 |
| N | The per-frame `device` is ignored inside a burst; the sender's ID has no airtime pre-check (BPSK31 ≈ 7 s; SL1 66 s + 7 s < 180 s watchdog) | Accepted, noted |
