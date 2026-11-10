---
project: openpulsehf
doc: docs/dev/design/onair-runner-watchdog.md
status: draft
last_updated: 2026-11-10
---

# The on-air runner stops both stations by itself: a watchdog with hard bounds

Work plan M3 (on air on 2 m). This changes when the rigs stop transmitting, because the watchdog runs
`cleanup`, which can stop a daemon mid-over. So it gets the mandatory review **before** it is built.
The maintainer chose to build it before any further keying (2026-11-10). Review: pending.

## Problem — task 35

In on-air task 35 (run f2p, 2026-11-09) both daemons stayed up **unattended for about 15 h**.
- **Station A kept transmitting for 96 min** to a peer that never answered: 76 BPSK31 frames and 9 IDs,
  which were its 38 queued messages × 2 attempts.
- **Why nothing stopped it:**
  - The spec's stop rules were applied by the station session's own watcher, keyed on both stations'
    sign-off IDs.
  - A never signed off, by design: its last transmission was an interval ID, which clears the
    sign-off condition (`station_id.rs`).
  - So the rules never armed.
- **`scripts/run-onair-twin-ota.sh` has no bound of its own.** `run` returns after `SIGNOFF_WAIT_MAX`
  with both daemons still up, and only an explicit `cleanup` stops them.

The task 35 review (`docs/dev/onair-tasks.md` on `m3/onair`) says a hard bound would have fired at
about 20:34, an hour and a half before A's last frame.

**The lesson this design applies:** a stop rule must not wait for an event the design does not
guarantee. Every rule below runs on the controller's clock and on the stations' own PTT records, with
no dependence on a sign-off or on any line a log filter can drop.

## Design

**A watchdog process, started by `run` right after both daemons are up**, on the controlling host
(rpi51). It runs detached (`setsid nohup`), so it outlives `run` itself: idle tasks deliberately keep
the daemons up after `run` returns. It records its pid in `$OUTPUT_DIR/watchdog.pid`.

Every 5 s it reads each station's audit file (`$CFG_DIR/openpulse/audit/events.ndjson`, as written
since `audit_mode = true`). It counts the `{"type":"ptt_changed","active":true}` lines, and notes the
controller time at which each new one is first seen. **Rules, any of which runs `cleanup`:**

| rule | fires when | default |
|---|---|---|
| **W1, wall clock** | now ≥ daemon launch + `RUN_CAP_SECS` | warm-up + traffic + `POST_TRAFFIC_SECS` (900) |
| **W2, keying after the traffic** | a key-up at either station is first seen later than traffic end + `POST_KEY_LIMIT_SECS` | 900 s: the 10-min live window plus 5 min |
| **W3, too many key-ups after the traffic** | more than `POST_KEYUPS_MAX` key-ups at one station since the traffic end | 12 |
| **W4, a stuck key** | a station's last PTT event is `active:true`, unchanged for over `PTT_MAX_KEYED_SECS` | 100 s: an SL2 frame is 66.6 s, plus an ID |
| **W5, lost sight** | a station's audit file cannot be read for 3 consecutive polls (60 s with backoff) | — |

**How the limits work:**
- **An idle task** (34, 35) raises `POST_TRAFFIC_SECS` explicitly, for example 2 400 for a 30-min idle.
- **The ceiling:** the runner **refuses to start** with `RUN_CAP_SECS` above `RUN_CAP_MAX` = 4 h.
- **No disarm.** The watchdog exits only when both daemons are gone, after its own cleanup or a manual
  one. There is no command to disarm it, so it cannot be forgotten off.
- **Each firing** writes `WATCHDOG: cleanup (W<n>: <why>) at <UTC>` to the run report and to
  `$OUTPUT_DIR/watchdog.log`, then runs the existing `do_cleanup`.

**`cleanup` itself is unchanged.**
- **The order:** stop the daemon (TERM, then up to 5 s, then KILL); command PTT 0 through rigctld while
  it still runs; then stop rigctld.
- **The known cost** is that a daemon stopped mid-over leaves the rig keyed for up to about 5.5 s. The
  daemon has no SIGTERM handler, so its PTT guard does not run. PTT 0 is commanded after the stop.
- **The watchdog adds no new keying path.** It only ever ends transmission sooner. This is the review's
  question 1.

**The audit file:** the watchdog truncates it at launch, before the daemons start. That also closes
the task 34 artefact (an appended file mixing two runs). The previous file is kept as
`events.ndjson.prev`.

## Verification

- **`run-onair-twin-ota.sh watchdog-self-test`** runs the pure decision function
  (`watchdog_verdict`, which takes the clock, the launch and traffic-end times, and two synthetic event
  streams) through cases that must fire and must not fire:
  - W1 before and after the cap;
  - W2: a key-up at traffic end + 899 s does not fire, and one at + 901 s fires;
  - W3: 12 key-ups do not fire, 13 do;
  - W4: keyed for 99 s does not fire, 101 s fires; a key that has ended does not fire;
  - W5: 2 failed reads do not fire, 3 do;
  - the ceiling refusal.
- **Sabotage:** each rule's threshold removed in turn must turn its cases red.
- **A dry run on the stations, with nothing keyed:**
  1. `run` with `RUN_CAP_SECS = 60` and no traffic (`TRAFFIC_DURATION = 0`, daemons up);
  2. the watchdog fires W1, and `cleanup` reports both daemons stopped.

  That needs no transmit go, because nothing transmits. Recorded as task 36 Part A.

## What it does not do

- **It is not a substitute for an attended run.** It bounds the unattended case after the fact.
- **It does not bound a daemon that keys outside the runner** (a manual `openpulse-server`).
- **It does not fix the cross-message keying gap** (a sender keys 2 × its queue to a silent peer), nor
  give the daemon a SIGTERM handler. Both are separate items.

## Questions for the review

1. Is any path by which the watchdog makes a transmission longer or later, rather than shorter?
2. Are the defaults right? In particular W2's 900 s against the 10-min live window, W3's 12, and W4's
   100 s against the longest single keying (SL2 66.6 s + ID; the K=3 union ACK 5.9 s).
3. Is W5 right to fail closed (cleanup) when B is unreachable? Cleanup on an unreachable station
   cannot reach it either.
4. Is the controller clock with first-seen times sound, given that the events carry no timestamps?

## Consumer

`scripts/run-onair-twin-ota.sh` (`do_run` starts the watchdog; a new `watchdog-self-test` action), run
by the station session on rpi51. Found with `grep -n "^do_run\|^do_cleanup" scripts/run-onair-twin-ota.sh`.

## Prior art

- The runner's `abort_after_launch` (stop both if one daemon dies at launch).
- `do_cleanup`'s PTT-0-while-rigctld-runs ordering (task 11).
- The gate's rig-power lower-only check.
- External: UNCHECKED.

## Twins

Every script that launches `openpulse-server` (`grep -ln "openpulse-server\|DAEMON_COMM\|launch_daemon" scripts/*.sh`;
the same filter matches the runner's three `launch_daemon` uses):
- **`demo-hwloop-panel.sh`, `run-twin-station-audio.sh`:** they stop their daemons with an exit trap
  (`trap cleanup EXIT INT TERM`, `trap 'kill …' EXIT INT TERM`), and they are loopback/demo rigs.
- **`onair-rx-idle-floor.sh`:** refuses to start while a daemon runs.
- **`onair-station-check.sh`:** only checks for the binary.
- **`setup-twin-loopback.sh`:** loopback setup.

**`run-onair-twin-ota.sh` is the only one that keys rigs and leaves its daemons up after it
returns.**
