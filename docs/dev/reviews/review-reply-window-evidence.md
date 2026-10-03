---
project: openpulsehf
doc: docs/dev/reviews/review-reply-window-evidence.md
status: resolved
last_updated: 2026-10-02
---

# Adversarial review — reply-window evidence, v1 (#1456, #1460; M2)

One round by Fable, held before implementation. **Mandatory** (decision 8): the design changes when a
receiver keys NACKs. The reviewer was read-only and was prompted for falsification. Target: v1 of
[`design/reply-window-evidence.md`](../design/reply-window-evidence.md) (opening NACK, `T_open`,
budget retired). v2 in that file answers this record.

## Consumer

`ota_decode_and_ack_inner` → `on_rx_frame(RxOutcome::Failed)`; the daemon OTA arm's NACK keying
(`OTA_NACK_BUDGET`); `transmit_ota_ack`. Found by `grep -n "on_rx_frame(RxOutcome::Failed\|fn
transmit_ota_ack" crates/openpulse-modem/src/engine.rs` and `grep -n "OTA_NACK_BUDGET"
crates/openpulse-daemon/src/server.rs`.

## Prior art

The parked `nack-streak-decay.md` and its review; the #1452/#1454 guards; #1255; `run_ota_retry`,
`MAX_RETRIES`, `MAX_SILENT_ACK_WINDOWS`. Found by `grep -n "fn run_ota_retry\|MAX_RETRIES\|MAX_SILENT_ACK_WINDOWS"
crates/openpulse-daemon/src/server.rs`.

## Twins

ARDOP does not use the OTA controller; linksim runs lockstep with no idle. Both from the previous review.

## Prompt

Falsify, do not agree, with file:line evidence: the reply-window premise and `E`; the opening NACK's
length test and `T_open` against a weak opening contact; the storm cap; retiring `OTA_NACK_BUDGET`
(§97 bound, station ID); interaction with HARQ and the climb streak; the falsifier from the decay
review. Does this block Release 1? If not, say so.

## Verdict

**Release 1: no** (A2 on 2 m keeps real failures inside any window). **Not implemented as written**:
findings 1, 2 and 4 regress the weak opening contact the design exists for.

| # | Finding | Class |
|---|---|---|
| 1 | The opening NACK's length test is vacuous: `min_frame_samples` is preamble plus one symbol (1.06 s at SL2, 72 ms on OFDM), the same quantity as the #1452 recognition window. No engine function yields the real shortest frame (about 12 s coded at SL2) | BLOCKING |
| 2 | `T_open` = 5 min silences real traffic: a failed first frame after a lull, within 5 min of an opening NACK, gets silence twice and the ISS abandons. A weak-link chat loses every second message | BLOCKING |
| 3 | A fade-split first frame gives fragments shorter than any frame, so no opening NACK — the case the #1452 comment rejected a whole-frame bound for. Lost outright at SL1 | FIX |
| 4 | An uncounted opening NACK delays the step-down to the ISS's fourth and last send, so the SL1 recommendation rides a NACK the ISS uses only for its next message. Today it lands on send 3 | FIX |
| 5 | The storm cap's reset is unspecified; "without a decode" is the mute defect again for a second queued message | FIX |
| 6 | Two OTA stations storm at OFDM rungs: an FSK4 ACK (0.52 s) passes the 0.29 s test, so B NACKs A's NACK, A's window is open, and both count to a step-down | FIX |
| 7 | The reply-window premise holds. The listening clock pauses during own TX, so the window is listening time, not wall time. `onset` must come from the post-lead length, or ring audio from before our TX lands in the window | NOTE |
| 8 | Retiring the budget turns "3 then mute" into 12 NACKs/h forever on a band of long foreign overs, each arming a station ID. HARQ retention after the window test is right | NOTE |

**Reviewer's alternative**, adopted as the base of v2: keep window-gated counting; drop the opening
NACK, its length test and `T_open`; make the daemon budget leaky (one back per about 10 min of
listening time); outside-window failures key an uncounted NACK under that budget, with #1452 as the
only length guard. **Falsifier for it:** the 90-min idle keys at most 3 NACKs per 10 min — measure
the flicker count that passes today's guards on the recording first.

## Round 2 — v2 (reply window, half-frame floor, ACK recognition, leaky budget)

Held before implementation, by Fable, read-only. **Prompt:** falsify the half-frame floor (post-lead
length, fade splits, multi-frame bursts, mixed candidate sets), the reply window and its retroactive
count, ACK recognition, the leaky budget, both falsifiers; is there anything simpler; twins; does it
block Release 1.

**Verdict.** Release 1: no. Not implementable as written.

| # | Finding | Class | Outcome in v3 |
|---|---|---|---|
| 1 | The flicker lengths were whole-burst, not post-lead: 3248 = a 2448-sample lead + 800. Every evidence flicker is two or three 400-sample reads (0.1–0.15 s) | FIX | Re-measured post-lead: 800–1200 samples at SL5/SL6 |
| 2 | A half-frame floor silences a frame shredded into three pieces, the decay falsifier's own case | BLOCKING | Measured: at −3 dB a BPSK31 frame flushes as dozens of pieces, and today only one 6.2 s piece counts. The floor becomes 0.5 s, capped at half a frame |
| 3 | The ACK scan uses the session MAC key, so it cannot see a foreign ACK on a keyed session; unbounded, it scans a 66 s burst | FIX | Keyless ShortFEC codeword test, run only on bursts up to two ACK lengths |
| 4 | `reply_mark` is set only on the daemon path, so the window rule breaks `ota_rate_decision_events`, the lockstep tests and `ota_channel_adaptation` | FIX | Window dropped |
| 5 | Window edge cases: decode time, an un-keyed NACK, session restart | NOTE | Window dropped |
| 6 | Budget arithmetic holds | NOTE | — |
| 7 | Uncoded rungs: a 1-byte uncoded frame is short, so the floor gives little protection there | NOTE | Stated |
| 8 | HARQ retention before the guards is unchanged | NOTE | Unchanged; separate work-plan row |

**Reviewer's recommendation, adopted as v3:** the floor (re-measured) plus the leaky budget, with no
reply window. The window only narrowed the #1460 foreign-over residual, which is HF only and not
Release 1.

## Round 3 — the v3 implementation

Held on the diff before the PR, by Fable, read-only. **Prompt:** falsify the floor's quantity and
candidate lengths, the ACK scan, the budget's parity with the old count and its wiring; what the
tests do not prove; does it block Release 1.

**Verdict.** Release 1: no. Implementable; no blocking finding. Budget parity confirmed: the old code
keyed 3 NACKs, the new one keys 3, and every decoded ladder frame is still answered.

| # | Finding | Class | Outcome |
|---|---|---|---|
| 1 | The ACK scan skips the CRC that `fsk4_ack_at` checks | FIX | **Not taken.** A keyed ACK carries its MAC in that byte, so a CRC check would hide foreign keyed ACKs again (round 2, finding 3). The false-accept rate without it is about 3.5e-5 per short burst. Stated in the doc comment |
| 2 | The scan started at the lead, so an ACK whose first read lands in the ring is missed | FIX | Scans the whole burst |
| 3 | The SL1 sub-floor ACK (FSK4 + three K=3 MFSK16 copies, about 5.4 s) is too long to be scanned | FIX (doc) | Stated in the design; the budget bounds it |
| 4 | A candidate whose length cannot be measured is dropped from the floor; the first failure at a slow rung modulates a whole frame once | NOTE | — |
| 5 | The leak's epoch is the first failure, not the mute | NOTE | Matches the design |
| 6 | The daemon wiring has unit tests only | FIX (test) | Parked in the work plan: a twin test needs deterministic failing ladder bursts |
| 7 | No existing "still evidence" test had a fragment between the recognition window and 0.5 s | NOTE | The 1367-test run agrees |
| 8 | The failing-frame test passed on any answered burst | NOTE | Now requires the ≥ 30 000-sample frame piece |
