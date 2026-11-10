---
project: openpulsehf
doc: docs/dev/design/onair-runner-watchdog.md
status: resolved
last_updated: 2026-11-10
---

# The on-air runner stops both stations by itself: a watchdog with hard bounds

Work plan M3 (on air on 2 m). This changes when the rigs stop transmitting, because the watchdog runs
`cleanup`, which can stop a daemon mid-over. So it gets the mandatory review **before** it is built.
The maintainer chose to build it before any further keying (2026-11-10).
- **The review: Fable, 2026-11-10, APPROVE WITH CHANGES**
  (`docs/dev/reviews/review-onair-runner-watchdog-2026-11-10.md`). All six changes are taken (marked C1–C6
  below).
- **Implemented 2026-11-10** (see Implementation).

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

## The review's changes (all taken)

- **C1:** `run` refuses while `watchdog.pid` names a live process, so a stale watchdog cannot kill a new
  run mid-over.
- **C2:** `cleanup` commands PTT 0 **before** the daemon stop as well as after it. The daemon has no
  signal handler, so TERM drops it without its PTT guard.
- **C3:** key-ups are **false → true transitions**, not lines. `ptt-state` re-emits the current state,
  so a keyed daemon can write `active:true` twice. W4's timer resets whenever the count or the state
  changes.
- **C4:** W3 is not a fixed 12. `POST_KEYUPS_MAX` = 2 × (`TRAFFIC_DURATION` / `TRAFFIC_INTERVAL`) + 4,
  because task 31's normal drain had 13 key-ups after the window.
- **C5: a station-side bound.** Each daemon launches as
  `timeout -k 5 RUN_CAP_SECS openpulse-server; rigctl … T 0`, so a dead or hung controller cannot leave
  a station keying. The trailing `T 0` runs on every stop path.
- **C6:** `TRAFFIC_DURATION = 0` starts **no traffic**. GNU `timeout 0` disables the timeout, and
  twin-traffic's `COUNT = 0` is infinite, so the dry run as first designed would have keyed for ever.
- **Polling filters on the station** (`grep -o` of the PTT events only). The f2p audit file reached
  310 k lines.

**Corrections to the design above:**
- **The runner's sign-off wait was already dead.** `signed_off` and `last_tx` grep `logged TX metadata`,
  a `debug!` the default filter never emits, so `run` always waited the full `SIGNOFF_WAIT_MAX`. That is
  why this design depends on no log line.
- **"A dry run with nothing keyed" is not keyless.** With the daemons up, each session is live for its
  first 10 min (`idle-nack-gate.md`, option B), so a floor flicker can draw up to 3 NACKs. The dry run is
  therefore part of the next transmit task, under its go, and not a no-go demonstration.
- **The discovery beacon** keys through `keyed_transmit(ptt, None, …)` with no `ptt_changed` event, so
  W2–W4 are blind to it. It is off by default, and the runner's config does not enable it.

## Implementation (2026-11-10, branch `feat/onair-watchdog`)

`scripts/run-onair-twin-ota.sh`:
- **The limits:** `POST_TRAFFIC_SECS`, `RUN_CAP_SECS`, `RUN_CAP_MAX`, `POST_KEY_LIMIT_SECS`,
  `POST_KEYUPS_MAX`, `PTT_MAX_KEYED_SECS`;
- **`launch_daemon`:** the audit file set aside as `events.ndjson.prev`, and the `timeout` wrapper with
  PTT 0;
- **`do_cleanup`:** PTT 0 before the stop as well;
- **`do_run`:** the ceiling and stale-watchdog refusals, `start_watchdog`, and no traffic at 0;
- **the watchdog itself:** `traffic_should_start`, `wd_count_keyups`, `wd_cap_verdict`,
  `wd_station_verdict`, `wd_probe`, `start_watchdog`, `do_watchdog`;
- **the actions:** `watchdog` and `watchdog-self-test`.

**`watchdog-self-test`: 20 cases, PASS.** Each rule fires and does not fire on either side of its
threshold. Duplicate `active:true` is not a key-up. A zero-length window starts no traffic. **The probe's
own remote command is run by a local shell against a planted audit file.** Its first version failed
there: an escaped `awk` program arrived mangled, and every station would have read 0 key-ups, leaving
W2–W4 blind. The station now only filters, and the counting is local.

**Sabotage** (each restored from `HEAD`): with each of W1–W5 removed, with the de-duplication removed,
and with a zero-length window allowed to start traffic, **exactly that case goes red, 7 of 7.**

**Not yet run on the stations.** The first on-air use is the next transmit task, whose Part A arms it
with a short cap and expects W1 to fire.

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
