---
project: openpulsehf
doc: docs/dev/reviews/review-1257-leader-delay.md
status: resolved
last_updated: 2026-10-02
---

# Adversarial review — PTT leader delay implementation (#1257)

Mandatory class: it changes when the first sample leaves relative to the PTT edge. The design was
reviewed on 2026-09-14 (the design-pass comment on #1257). This round, by Fable, reviewed the
implementation: read-only, prompted for falsification, with a sabotage probe in a scratch copy.

## Consumer

Every keying front end through `openpulse_radio::SharedPtt::key_as`: daemon (`server.rs` builds the
one `SharedPtt`), ARDOP (`lib.rs` from `ArdopConfig::ptt_leader`, including the host `PTT TRUE`
command), KISS (`lib.rs`), CLI `transmit` (`--ptt-leader-ms`). Found by
`grep -rn "SharedPtt::new" --include=*.rs crates apps` (production rows listed in the PR).

## Prior art

#1250's review (keying is a burst property, decided by the caller); #1263 (the generation token);
#1299 (the CLI moved onto `SharedPtt`). Found by `git log --oneline -S "generation" -- crates/openpulse-radio/src/shared_ptt.rs`.

## Twins

The cross-band repeater's `rig_b` (`openpulse-repeater/src/lib.rs:127`) and `calibrate`'s two
`SharedPtt`s are separate keying funnels that do not take the leader; both are now documented as not
covered. Found by the same grep.

## Prompt

Falsify, do not agree, with file:line: (1) does every production keying path get the leader, and does
anything bypass `key_as`; (2) is it charged once per key transition everywhere; (3) does the sleep
block a tokio worker or extend another lock, and does it eat an ACK or turnaround budget; (4) the
watchdog/ownership interaction — `asserted_at` is stamped before the sleep, and can a release during
the leader leave the caller transmitting; (5) do the tests discriminate; (6) are the docs accurate.
Positive controls on every grep. Does this block Release 1? If not, say so.

## Verdict

**Fix first → all fixed.** Does not block Release 1: the default is 0, so no shipped path changes until
an operator opts in.

1. **Defect: ownership race (#1263 reopened).** `keyed()` read the generation after the sleep, and
   `key_as` returned Ok without re-checking. Probe: release during the leader, then a second key — the
   first caller got Ok, and dropping its guard released the second caller's key. **Fixed:** the
   generation is captured under the lock before the wait and re-checked after it; a moved generation or
   a released key returns the new `PttError::ReleasedDuringLeader`. New test
   `a_key_released_during_the_leader_is_not_returned_as_owned`; sabotaged (re-check disabled) it fails.
2. `the_leader_does_not_hold_the_lock` discriminated by accident of a race. **Rewritten** to time the
   read from the `PTT TRUE` notify; sabotaged (sleep under the lock) it now fails on the wait assertion
   ("waited 400 ms").
3. `rig_b` and `calibrate drive` silently ignore the setting, and the book said "every front end".
   **Documented** in the book, the config field and template, and the CLI help.
4. `calibrate.rs` cited a pinning test, `calibrate_ptt_is_not_inflated_by_a_leader`, that does not
   exist (pre-existing comment). **Corrected** to say no test pins it.
5. `openpulse transmit`'s airtime-vs-watchdog refusal ignored the leader. **Fixed:** the leader is added
   to the airtime.

Also checked, no change needed: no per-fragment re-key (OTA retries re-key per attempt, which is
correct); the daemon keys on the main-loop future and ARDOP under the engine lock, so a leader stalls
those for its length. That is the same class as the existing TX stall (#1301) and fine at realistic
50–300 ms. The ardopcf correction was checked against `Modulate.c`; the "120–2500 ms" range is
UNCHECKED.
