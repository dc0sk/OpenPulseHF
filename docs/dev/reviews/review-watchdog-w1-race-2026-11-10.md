---
project: openpulsehf
doc: docs/dev/reviews/review-watchdog-w1-race-2026-11-10.md
status: resolved
last_updated: 2026-11-10
---

# Review: the watchdog's W1 race fix (2026-11-10)

A spot-check of the follow-up to the runner watchdog
(`docs/dev/reviews/review-onair-runner-watchdog-2026-11-10.md`), on branch `fix/watchdog-w1-race`.
Reviewer: Fable, read-only.

## Prompt

**The incident (task 36 Part A on the stations, `RUN_CAP_SECS = 180`, no traffic).** The station-side
`timeout -k 5 180` equalled the cap.
- Both daemons ended on their own timeout a poll before W1.
- The watchdog logged "both daemons gone" without W1.
- `rigctld` stayed up.
- Nothing keyed, and PTT 0 was sent.

**The fix:** the station timeout becomes `RUN_CAP_SECS + 120`, and the gone-exit runs `do_cleanup`.

**It was asked:**
- Is the arithmetic expanded locally?
- Can the margin lengthen a transmission?
- Is running cleanup on the gone-exit risky?
- Are there other paths where the daemons exit before W1?
- Is there a residual race?
- **"Does it block Release 1?"**

## Verdict

**CHANGES (one doc line), code APPROVED. Does not block Release 1.** It is tooling for the M3 runner,
with nothing on the wire or in the keying path.
- **The expansion is local:** the station receives `timeout -k 5 300 …`.
- **The margin:** a transmission gets longer only with a dead controller, by at most 120 s (+5 s KILL
  grace). The wrapper's `rigctl T 0` runs on every exit.
- **The worst case is the cap + about 55 s:** one poll, plus the previous probes, plus cleanup. The
  station timers start a few seconds before the watchdog's `launch`, so A's effective margin is about
  110 s. That is still clear.
- **Cleanup on the gone-exit:** both stations were reachable one poll earlier. A hand-restarted daemon
  inside that window would be killed, but that is outside the contract. A stray `rigctld` is logged.
- **A crash of both** takes the same exit. Optional: log `timeout`'s rc. **A single crash** is
  pre-existing: park it.
- **Required:** the design's C5 still names the equal timeout. Change it to
  `timeout -k 5 (RUN_CAP_SECS + 120)`, with the reason.

## Disposition

- **The required doc change is applied.**
- **The rc logging is applied.** The wrapper writes `daemon exited rc=N`. The remote command was
  rendered and passed `bash -n`, with a broken-string control failing.
- **The single-daemon death is parked** in the work plan.

## Consumer

`scripts/run-onair-twin-ota.sh` (`launch_daemon`, `do_watchdog`), run by the station session.

## Prior art

The runner watchdog's review, and its C5 (the station-side bound).

## Twins

Only this runner keys rigs and leaves its daemons up (the watchdog design's `## Twins`).
