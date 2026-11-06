---
project: openpulsehf
doc: docs/dev/reviews/review-late-ack-and-id-ride-2026-11-02.md
status: resolved
last_updated: 2026-11-02
---

# Review: no late ACK, a decode stops at its ACK deadline, the ID rides in each station's own keying (2026-11-02)

Adversarial review (Fable) of `fad19ea`, plus the follow-up `ccf3cff` that landed while it ran. It is
mandatory because it changes when the receiver keys (ACK and NACK) and when both stations key the station
ID (§97.119, REQ-REG-10).

## Prompt

The prompt asked for falsification, not agreement:
- **lockstep:** between the ladder, the NACK budget, HARQ, the evidence rules and the sender's retries;
- **the deadline:** whether it can be wrong, including on held loops and in harnesses faster than real
  time;
- **coverage:** whether it applies to non-ladder decodes, and whether phase 2 and HARQ are bounded;
- **the ride-along:** its order and its airtime;
- **regulatory:** the ID timer's arming;
- **the tests.**

It also asked "Does this block Release 1?".

## Verdict

**One blocker**, which `ccf3cff` did not fix: the deadline was set in the twin harness and cut decodes
there. The keying direction is sound: no ACK past the window, and the ID inside the station's own keying.
The §97.119 bound is unchanged.

Confirmed by reading:
- arming after a ride-along (`mark_identified`, then `note_tx`) is right;
- the receiver's ID airtime check matches what `transmit` sends, so a slow active mode is refused when it
  would push the ACK past the window;
- `ota_ack_timeout_ms` derives from the profile on both ends, and the handshake's ladder fingerprint
  suppresses OTA on a mismatch.

| # | Severity | Finding | Disposition |
|---|---|---|---|
| B1 | blocker | The real-time check tested the read that flushed the burst, which is small. The burst's audio had arrived earlier, in a faster-than-real-time read. So the deadline was set in the twin, and `twin_multi_fragment_file` with OTA failed deterministically: the four uncoded fragments were never decoded. | **Taken.** `burst_real_time` covers every read since the burst opened: a burst counts as real time only if no read held more audio than the time since the one before it. |
| S1 | should-fix | The deadline also bounded decodes that need no ACK: the uncoded fallback (station ID, file fragments, handshake, QSY) and the frames after it. | **Taken.** `non_ladder_decode` exempts the ranked and deferred fallback, phase 2's fallback entry and the frames that follow a fallback decode. New test `a_non_ladder_frame_still_decodes_past_the_deadline`; it fails without the exemption. With it, the twin multi-fragment test passes. |
| S2 | should-fix | The frame end was taken after the monitor's and the repeater's hand-off, so the deadline could be late by the monitor's decode time. | **Taken.** It is anchored on the moment of the read. |
| S3 | should-fix | A failed ID took the ACK, or the data attempt, down with it. On the sender, a failed attempt retransmits without listening for the ACK, which is the very collision being fixed. | **Taken.** A failed ID is logged and the ACK, or the ACK listen, goes ahead. The ID counts as sent only when it went out. |
| S4 | should-fix | The data-then-ID test skipped SL1 (MFSK16). | **Taken.** SL1 added; it passes. |
| S5 | should-fix | The sender's ID was taken before keying, so a refused keying lost it. | **Taken.** It is taken inside the keyed transmit. |
| S6 | note | A late decode emits no `OtaRateDecision`, and a failing low-rung burst on a Pi 4 will often end late, with no NACK and no demotion. Then A2 clause 2 can only be met by failures that finish inside about 6 s. | Stated in task 19. The run reviews count the "finished past the sender's ACK window" line. |
| N1 | nit | The 2 s keying allowance is fitted to one inventory (task 18's logs), and the apparatus is not committed. Falsifier: a rig with slower PTT or stream start. | Recorded here. Task 19 asks the station for the decode-to-first-sample times. |
| N2 | nit | Phase 2's acquisition and the HARQ soft demods were not deadline-gated. | **Taken.** Both skip ladder work past the deadline. |
| N4 | nit | The station-ID test's tolerance. | Already taken in `ccf3cff`. |
| N7 | nit | An uncoded frame decoded by the fallback returns early and never scans the rest of the burst for a ladder frame (seen with ID-then-data). | Parked in the work plan. On air, the tick arm's ID and a command-arm send are separate keyings. |

## Consumer

`server::run` (`crates/openpulse-daemon/src/server.rs`): the receive tick's OTA arm and `ota_send_with_ptt`.

## Prior art

The NACK budget's verdict (#1460) for "unkeyed evidence moves no ladder"; `listen_for_ack`'s in-stream
scan (#1315) for an ACK behind other energy; `CaptureGap` for the reads.

## Twins

The ARDOP TNC keys its ID on its own loop (the quiet rule, `due_reason`) and has no ride-along; UNCHECKED
whether its ARQ path needs the deadline.
