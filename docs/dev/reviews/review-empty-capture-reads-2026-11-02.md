---
project: openpulsehf
doc: docs/dev/reviews/review-empty-capture-reads-2026-11-02.md
status: resolved
last_updated: 2026-11-02
---

# Review: an empty capture read no longer ends a burst (2026-11-02)

Adversarial review (Fable) of `63a1f52`, the daemon's empty-read rule. It is mandatory because it
changes when a received burst ends, and so when the daemon keys a NACK or an ACK.

## Prompt

The prompt asked for falsification, not agreement:
- a path that keys more often, or over the other station;
- a burst left open for ever by a stalled device;
- the interaction with `rx_pending` and with dropping and reopening the stream around a transmit;
- harness timing;
- twins on other receive paths;
- what inventory 250 ms was fitted to, and what would falsify it;
- whether the test fails without the change.

It also asked "Does this block Release 1?".

## Verdict

No blocker, and it does not block Release 1. It removes an on-air blocker.

What the review confirmed:
- **No new keying path.** For a real transmission the burst still ends on the first quiet read after
  the carrier, within one tick. The 250 ms path matters only when the device delivers nothing: a stream
  that has ended, or a dead device. A stalled device still flushes after 250 ms and still reaches the
  fault and reopen path.
- **The daemon suite passed**, 220 tests.
- **250 ms is justified by the logs.** Across 752 empty-read runs in the task 16 logs, the longest
  span with no audio that later resumed was 37–45 ms. Every span over 250 ms was the stream reopening
  after our own transmit.
  - Falsifier: a capture device with a callback period of 250 ms or more.
  - Tripwire: in the next on-air logs, flushes with `read=0` and `energy` at or above `squelch` must be
    close to zero.

## Findings

| # | Severity | Finding | Disposition |
|---|---|---|---|
| 5 | should-fix | **Twin:** `CaptureTicker::tick` hands every read, empty or not, to `accumulate_capture`. The ARDOP TNC bridge (Release 1) and the KISS bridge receive through it. The fix also sat in one caller, against the RX seam checklist. | **Taken.** `CaptureGap` in `capture_ticker.rs` is used by both `CaptureTicker` and the daemon's `rx_ticker`. New test `capture_ticker_empty_reads`; sabotage: the frame splits into 12 bursts, the first 2000 of 9504 samples. |
| 3 | nit / parked | The stream drops before a transmit never abandon the engine's open burst (pre-existing, #1319 twin). | Parked in the work plan. Not resetting the gap clock on reopen is not a regression. |
| 4 | nit | The test doc and the task 16 note said the twin harness ends bursts on empty reads; the twin rig fills silence. | **Taken** in the task 16 note. |
| 6 | note | 250 ms is fitted to one device class (PulseAudio on two Pis). | The constant's doc names the inventory and the falsifier. |
| — | should-fix (paperwork) | No ledger entry, acceptance row or sharp edge. | **Taken.** |
| 7 | unconfirmed | The daemon test could flake under heavy load, if a tick stalls for 250 ms mid-frame. It recovers because the frame repeats. | Noted. |

## Consumer

`server::run`'s receive tick (`crates/openpulse-daemon/src/server.rs`) and `CaptureTicker`
(`crates/openpulse-modem/src/capture_ticker.rs`), consumed by `crates/openpulse-ardop/src/bridge.rs` and
`crates/openpulse-kiss/src/bridge.rs`. Found with `grep -rn "CaptureTicker\|accumulate_capture" --include=*.rs crates`.

## Prior art

`CaptureTicker` already owned the held-stream pattern for the same consumers (#1297, #1310), so the gap
rule sits there. The engine's own rule (`accumulate_routed` counts only a non-empty read as carrier) is
unchanged, because the loopback and replay streams rely on it.

## Twins

The reviewer found `CaptureTicker`, and it is fixed. Not twins:
- `listen_for_ack` appends every read and does no burst gating;
- the CLI's open-read-drop receive (#1297) is broken on cpal for a different reason.
