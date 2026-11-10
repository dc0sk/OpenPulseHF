---
project: openpulsehf
doc: docs/dev/reviews/review-onair-runner-watchdog-2026-11-10.md
status: resolved
last_updated: 2026-11-10
---

# Review: the on-air runner's watchdog (2026-11-10)

- **Subject:** `docs/dev/design/onair-runner-watchdog.md` (draft), on branch `feat/onair-watchdog` at
  `origin/main` a520e6f9.
- **This is the mandatory review** (keying class: it changes when PTT is released), held before
  building.
- **Reviewer:** Fable, read-only.

## Prompt

The reviewer was given:
- the draft;
- the runner (`do_run`, `wait_for_signoff`, `signed_off`, `do_cleanup`, `launch_daemon`,
  `abort_after_launch`, `write_config`);
- the daemon's audit writer, `station_id.rs`, and the task 35 review.

It was asked to falsify:
1. any path by which the watchdog makes a transmission longer or later (cleanup mid-over, a concurrent
   cleanup, a stale watchdog, the sequential stop);
2. the audit-file signal: every keying path, flushing, and truncation;
3. the rules' defaults against the evidence of tasks 31–35;
4. what a dead controller leaves;
5. the self-test and the dry run;
6. any simpler bound.

It was also asked: **"Does this block Release 1? Say so."**

## Verdict

**APPROVE WITH CHANGES** (six). It does not block Release 1. It is M3 process tooling, and the
precondition of the next transmit go, as the maintainer set.

**Framing corrections:**
- **The runner's sign-off detection is already dead.** `signed_off` and `last_tx` grep
  `logged TX metadata`, a `debug!` in `openpulse_modem::engine`. All six evidence reports read "sign-off
  IDs … no, no".
- **Only f2o and f2p carry per-keying timing.** Tasks 32–33's logs are at info level.

**Q1, can it lengthen a transmission?** No keying path is added, and `cleanup` only shortens one. Three
edges:
- **a stale watchdog and a second `run`** (C1);
- **the PTT-0 order:** the daemon has no signal handler, so `T 0` only after the stop leaves the rig
  keyed for the stop's duration, up to 5.5 s (C2);
- **the sequential A-then-B stop:** B can answer A's cut burst with one union NACK (about 6 s) inside
  its live window. That is bounded. Concurrent cleanups are idempotent.

**Q2, the audit signal, is sound.**
- Every `engine.transmit*` goes through `keyed_transmit`, enforced by
  `tests/ptt_keys_every_daemon_transmit.rs`. Its `SharedPtt` observer emits `PttChanged` on a real
  transition.
- **The exception:** the discovery beacon keys with no event. It is off by default.
- **Flushing:** `EventRecorder::record` flushes per line.
- **Duplicates:** count transitions, not lines (C3).
- **Truncation:** nothing reads the appended history.

**Q3, the defaults:**
- **W2 at 900 s holds.** Post-window key-ups in tasks 31–34 ended at most 2 min after the traffic, and
  in f2p W2 would have fired at 20:38:47.
- **A fixed W3 of 12 fires on a normal run:** task 31 had 13 key-ups after its window (C4).
- **W4 at 100 s holds for OTA.** Check filexfer bursts before any filexfer task.
- **W5 failing closed is right**, though cleanup cannot reach the station that is out of sight, so a
  station-side bound is needed (C5).

**Q4, a dead controller:** a station-side `timeout` wrapper, which is one line (C5).

**Q5, the self-test as designed is not safe.** `TRAFFIC_DURATION = 0` reaches `timeout 0`, which
disables the timeout (verified), and `COUNT = 0` is infinite. The dry run would have keyed (C6). Add
"a duplicate `active:true` is not a key-up".

**Q6, a simpler bound:** the `timeout` wrapper gives W1 station-side. The watchdog is still needed for
W2–W4, which read the PTT record.

**The changes:**
1. `run` refuses while `watchdog.pid` names a live process.
2. `do_cleanup` commands `T 0` before the daemon stop and after it.
3. Count false → true transitions. W4's timer resets when the count changes.
4. Drop W3, or derive `POST_KEYUPS_MAX` = 2 × (`TRAFFIC_DURATION` / `TRAFFIC_INTERVAL`) + 4.
5. A station-side bound at launch: `timeout -k 5 RUN_CAP_SECS openpulse-server; rigctl T 0`.
6. `TRAFFIC_DURATION = 0` must skip the traffic launch entirely.

Also: poll with remote-side filtering.

## Disposition

All six changes are taken, and so is the remote-side filtering (see the design's "The review's changes"
and its Implementation). The parallel stop of both stations (Q1, third edge) is not taken: the
sequential stop is bounded.

## Consumer

`scripts/run-onair-twin-ota.sh`, run by the station session. See the design note.

## Prior art

`abort_after_launch`, and `do_cleanup`'s PTT-0 ordering (task 11). See the design note.

## Twins

The other daemon-launching scripts stop their daemons with an exit trap. See the design note's
`## Twins`.
