---
project: openpulsehf
doc: docs/dev/reviews/review-task36-onair-2026-11-10.md
status: resolved
last_updated: 2026-11-10
---

# Review: on-air task 36, the watchdog's first run and f2q (2026-11-10)

The full review text is in `docs/dev/onair-tasks.md` on `m3/onair`, "Review of task 36". This record
carries what it changed on `main`. Reviewer: Fable, read-only, at `main` fb9323a2 and `m3/onair`
e163c844.

## Prompt

The reviewer was asked to falsify five things from the f2q evidence and `watchdog.log`:
1. that the watchdog behaved as designed, including `rc=143`;
2. that the 30 `failed burst is an ACK` lines at A are #1531's receiver side exercised on air, although
   B keyed no NACK;
3. whether #1532's on-air check needs more airtime;
4. what the work plan's #1532 row should say;
5. whether A2 on 2 m needs re-scoring after #1531–#1535.

It was also asked: **"Does anything block Release 1?"**

## Verdict

**Nothing blocks Release 1.**
- **The watchdog works as designed.** W1 fired in both parts, `rc=143` is SIGTERM from cleanup, and the
  log holds three starts.
- **#1531's receiver side is shown.** The 30 lines are union-ACK pieces in `ota_decode_burst`'s failed
  branch, at 12 400–15 000 samples. They passed #1454 and #1456 with a live peer, and without #1531
  they would have been NACKed. The spec's "weak if no NACK" was wrong for a type-blind guard.
- **A cost to flag:** each piece took about 1.5 s of failed-burst processing at the Pi 5.
- **#1532: close the on-air check.** What is shown and not shown is stated in the work-plan row.
- **A2:** 2 m stands on `a1dafca7`. Record the build delta. The re-score is the HF RC run.
- **Defects found:**
  - the runner's sign-off wait is blind (ANSI escapes in the live log);
  - a misleading "channel still busy" log line.

## Disposition

- **The 1.5 s figure is narrowed, then measured.** It is the whole failed-burst processing per piece. A
  probe the same day attributed it: the #1123 uncoded fallback is 830–1 160 ms on x86, and the guard 19 +
  60 ms.
- **#1531's documented "worst case" is corrected in place** to "on noise", in the design note, the
  ledger and the work-plan row.
- **Applied on this branch:** #1532's row closed, the A2 build-delta note, and four parking rows (the
  sign-off wait, the real-piece cost, the busy-log line, the rc text).

## Consumer

`docs/dev/design/union-ack-recognition.md`, `docs/dev/project/workplan.md`, and
`docs/dev/project/traceability.md`.

## Prior art

`review-union-ack-recognition-2026-11-08.md` (change 7, the stall bound), and `idle-nack-gate.md`.

## Twins

UNCHECKED: no code changed.
