---
project: openpulsehf
doc: docs/dev/reviews/review-phase2-fallback-non-ladder.md
status: resolved
last_updated: 2026-10-03
---

# Adversarial review — the OTA phase-2 fallback is non-ladder traffic (M2)

One round by Fable, on the diff before the PR. **Mandatory** (decision 8): the change removes an ACK
keying. The reviewer was read-only and was prompted for falsification.

## Consumer

`ota_decode_and_ack_inner` (phase-2 block) → the daemon OTA arm, which keys an ACK only for
`ack: Some` and chains `res.more`. Found by `grep -n "'settle_scan" crates/openpulse-modem/src/engine.rs`
and `grep -n "res.more\|res.ack" crates/openpulse-daemon/src/server.rs`.

## Prior art

Phase 1's fallback early return (#1123, #1118) and `decode_following_frames` (#1461); the
`a_fallback_decode_retains_no_harq_llrs` test. Found by `grep -n "ota fallback decoded\|fn decode_following_frames\|fn a_fallback_decode_retains_no_harq_llrs" crates/openpulse-modem/src/engine.rs`.

## Twins

Phase 1's fallback arm, which this now mirrors. The CLI receive path has no OTA ladder.
Found by reading the same function.

## Prompt

Falsify, with file:line evidence:
1. Is the early return complete: AFC state, `decoded_span`, HARQ, `last_flush_*` and the
   #1454/#1456 guards?
2. Is `start` the right onset for the following frames?
3. Can the ordering let the fallback win a real ladder frame?
4. Does any real ladder frame now lose its ACK?
5. Does the test prove what it claims?

Does this block Release 1? If not, say so.

## Verdict

**Release 1: no.** The fix is correct and mirrors phase 1 exactly.

| # | Finding | Class | Outcome |
|---|---|---|---|
| 1 | The rung assertion was vacuous: the test locked the level, and a locked controller never moves | FIX | Runs unlocked at the SL2 entry rung and asserts the confirmed rung. Without the fix it fails with SL2 → SL1 |
| 2 | The multi-frame test lacked the phase-2 guard | NOTE | Added `settles > 0` and `ack.is_none()` |
| 3 | The early return is complete: AFC commit on success as on the ladder path, no HARQ retention, guards bypassed for a decode, daemon chains `more` | NOTE | — |
| 4 | `start` is the same onset phase 1 hands over; there is no drop or duplicate | NOTE | — |
| 5 | Ordering adds no wrong-winner path: the fallback comes last and only when it is not a candidate | NOTE | — |
