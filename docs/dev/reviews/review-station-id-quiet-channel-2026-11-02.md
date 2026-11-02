---
project: openpulsehf
doc: docs/dev/reviews/review-station-id-quiet-channel-2026-11-02.md
status: resolved
last_updated: 2026-11-02
---

# Review: no station ID over another station; drop the burst when keying (2026-11-02)

Adversarial review (Fable) of `803b110`. It is mandatory because it changes when and whether the
transmitter keys (the station ID and the NACK), and the station ID is a regulatory obligation
(§97.119, REQ-REG-10).

## Prompt

The prompt asked for falsification, not agreement:
- a path that transmits without an ID for more than 10 minutes, or never sends the end-of-exchange ID;
- starvation of the sign-off on a station whose squelch sits near its noise;
- a frame lost by abandoning a burst;
- the noise-floor and ring handling in `abandon_burst`;
- a busy check that mutes NACKs for ever, or reads our own transmission;
- twins in Release 1 crates;
- whether the tests go through production entries and fail without the change.

It also asked "Does this block Release 1?".

## Verdict

No blocker, and it does not block Release 1. What the review confirmed:
- **The regulatory bound holds.** The interval ID is forced at the interval whatever the channel does,
  so a busy channel only delays the sign-off, never the 10 minutes.
- **The sign-off waits only briefly** on a station whose squelch sits near its noise: a flicker plus
  the 100 ms carrier-detect hold.
- **The busy-check fallback is the last non-empty read's energy**, which the b7 log shows. It cannot
  mute for ever: the check runs again on every flush, and a muted failure spends no budget and moves no
  rung.
- **Twins:** the file-transfer drain and the discovery beacon already carrier-sense. The repeater's ID
  is deliberately unchanged, with a documented reason, and is outside Release 1.

Suites at `803b110`: daemon, ARDOP, KISS and repeater 318 passed; core 371 passed; 0 failed.

## Findings

| # | Severity | Finding | Disposition |
|---|---|---|---|
| S1 | should-fix | The daemon's `abandon_burst` wiring was not proven by a failing test: the engine test calls it directly, and the station-ID test passes without the daemon's calls. | **Taken.** `abandon_burst_when_keying` (daemon, `server::run`): a message keyed while another station's carrier fills the channel must log the engine's tripwire line `burst abandoned: this station keyed`. Sabotage, with the daemon's calls removed: fails. `ModemEngine::bursts_abandoned()` counts too. |
| S2 | should-fix | With an interval of 90 s or less, the early slot opened at once, so the station would ID after every frame. | **Taken.** The quiet window is at most half the interval. New core test. |
| S3 | should-fix (design, unconfirmed on air) | The early interval ID lands in the turnaround after an ACK, when the sender starts its next frame, which costs one frame per ~10 minutes. The daemon's sender does not carrier-sense. | **Parked** in the work plan. Far better than before, but worth a follow-up: put the ID in the ACK's keyed burst, or require a longer quiet. |
| N1 | nit | The first site abandoned the burst before an OTA send that may not key. | **Taken.** Removed. The command arm's site after a real transmit covers it. |
| N2 | nit | The ARDOP TNC's `release_capture` drops the stream but not the burst. | **Taken.** `release_capture` takes the engine and abandons the burst. |
| N3 | nit | `abandon_burst` left `s_permitted_latch` and the other per-burst state set. | **Taken.** It resets the same state a flush does, without recording a flush. |
| N4 | nit | The timer's doc, the config comment, the TOML template and the book still described the old sign-off timing; the work-plan parking row should close. | **Taken.** |

## Consumer

- `server::run`'s receive tick and command arm (`crates/openpulse-daemon/src/server.rs`).
- The ARDOP TNC's worker (`crates/openpulse-ardop/src/bridge.rs`).

Found with `grep -rn "signoff_due\|id_due(\|release_capture\|rx_stream = None" --include=*.rs crates`.

## Prior art

`StationIdTimer` (REQ-REG-10). The `#1263` deferral, which waits while the key is held by another
emission, is unchanged. The discovery beacon and the file-transfer drain already wait for
`is_channel_busy()`.

## Twins

- The repeater's `identify_if_due_at` is deliberately not carrier-sensed (its doc comment), and is
  outside Release 1.
- KISS has no ID timer.
- The CLI's `beacon` keys on a fixed interval without carrier sense. It is pre-existing and not on the
  on-air gate's path; UNCHECKED whether it should change.
