---
project: openpulsehf
doc: docs/dev/design/nack-streak-decay.md
status: draft
last_updated: 2026-10-02
---

# The NACK streak decays when failures stop looking like a peer's retransmissions (#1456, #1460)

> **Parked 2026-10-02 after review** ([`reviews/review-nack-streak-decay.md`](../reviews/review-nack-streak-decay.md)).
> Not implemented. At SL2, decay cuts idle-flicker demotions only from about 4.0/h to 2.45/h, and the
> daemon's `OTA_NACK_BUDGET` mutes the receiver after spaced flickers regardless. Not Release 1
> blocking. Any redesign must pass the falsifier in the review.

Work plan M2, "ladder behaviour that A2 scores".

## Problem

`rx_consecutive_nack` (`openpulse-core/src/ota_rate.rs`, `on_rx_frame`) is reset only by a decode or by
the step-down it triggers. It has no notion of time. Every failed burst that passes the engine's
evidence guards (`ota_decode_and_ack_inner`: not a cap flush #1255; total-power run or spectral span
at least one recognition window #1452/#1454) adds one. Three steps down both `rx_confirmed` and
`rx_recommended`:

- **#1456, idle flicker.** Behind a 250 Hz filter, idle trips the squelch often enough that bursts
  longer than a recognition window occur several times an hour (estimated in #1456 from measured trip
  rates). Across an idle hour they accumulate, and every third one demotes both candidates. The
  peer's next real frame then arrives at a rung that is no longer a candidate.
- **#1460, foreign overs.** Another station's PSK31, RTTY or CW over in the passband is a failed burst
  long enough to count. Three of them demote, and up to three NACKs are keyed at a QSO that is not
  ours.

## Decision (proposed)

**Decay by spacing.** A peer whose frame failed retransmits it one exchange later: its frame airtime
at the rung it is sending, plus the ACK window. Failures spaced much further apart are not one fade
on one link, so they do not belong to one streak.

- The engine (which has the clock) records the receive-time position of each counted failure in
  **captured samples**: the daemon feeds every capture read through `accumulate_capture`, so sample
  count is received-audio time. It is deterministic in tests and pauses during this station's own
  transmissions, which are not listening time.
- On a counted failure, if the gap since the previous counted failure exceeds
  `T_gap = 2 × (max_frame_samples(rx_confirmed rung, its FEC) + ota_ack_timeout)`, the controller
  first restarts the streak: a new `OtaRateController::restart_nack_streak()`, called by the engine
  before `on_rx_frame(Failed)`.
- The controller stays clock-free, as it is today; the engine owns the time base.
- `rx_confirmed`'s rung is used, not the slowest candidate. That is what the peer was last confirmed
  sending, so a slow fallback candidate does not stretch the gap at a fast rung.

**What it fixes.**
- Idle flicker spaced minutes apart never reaches three within `T_gap` at SL5–SL6 (`T_gap` about
  30–40 s).
- Foreign overs spaced further apart than `T_gap` no longer accumulate.
- A real fade keeps its behaviour: the peer retransmits every period, so the failures stay inside
  `T_gap` and the step-down proceeds rung by rung, as today.

**What it does not fix (residual, stated).**
- Foreign overs closer together than `T_gap` still count. A contest-style exchange near SL2 is the
  case, where `T_gap` is about 2.5 min.
- The fix for that is evidence that a failed burst is OURS: a preamble correlation at a candidate
  rung. A template exists for BPSK250 only. Per-mode templates belong with the replacement preamble
  (#1062, after v0.17). #1460 stays open for that part and is narrowed.
- A liveness rule ("count failures only after a recent decode") was considered and **rejected**.
  `fast` enters at SL2 with SL1 below it, so a weak opening contact whose first frames all fail
  would never step down to SL1. And the sender (`server.rs`, OTA send loop) has no rate fallback of
  its own on ACK timeout, so NACKs are its only way down.

## Risks and how each is checked

- **A slow fade at a fast rung, with retransmission spacing above `T_gap`.** The ISS retransmits
  within one ACK window plus its frame, so spacing above `2 × (frame + ack window)` requires the ISS
  to skip a turn. The daemon's OTA send loop does not (`OtaAttempt::Timeout` retries at once).
  Checked by test: a fade at SL6 with frames every period still steps down.
- **A wrong `max_frame_samples` for the rung** (the #1384 coded-sizing pitfall). Use `frame_plan`
  with the rung's FEC, as `burst_cap` does. Checked by test at a coded rung.
- **A2 scoring:** fewer demotions during idle; climb behaviour is unchanged. `ota_channel_adaptation`
  (held out) and the OTA lockstep tests must still pass.

## Tests

- **Core:** `restart_nack_streak` resets the count, and two failures, a restart, then two failures do
  not step down.
- **Engine, via `accumulate_capture` over a recorded idle:**
  - long idle-flicker bursts spaced one minute apart at SL5 do not demote (sabotage: no restart →
    demotes);
  - three failures one frame period apart do demote (control).
- **Held out:** `ota_channel_adaptation` 3/0.

## Consumer

`ota_decode_and_ack_inner` (engine) → `OtaRateController::on_rx_frame(RxOutcome::Failed, ..)`, reached
from the daemon receive tick's OTA arm. Found by `grep -n "on_rx_frame(RxOutcome::Failed"
crates/openpulse-modem/src/engine.rs`.

## Prior art

#1452 and #1454 (recognition-window and spectral-span evidence rules), #1255 (cap flush is not
evidence), #1142 (abstention is not a low reading), `OTA_NACK_BUDGET` (daemon). Found by
`grep -n "#1452\|#1454\|#1255\|OTA_NACK_BUDGET" crates/openpulse-modem/src/engine.rs crates/openpulse-daemon/src/server.rs`.

## Twins

`openpulse-linksim` reimplements the policy layers and has its own failure feed. It is not on the
daemon path, and A2 is scored on air, but `goodput_gate` runs there. It should get the same rule or
an explicit note. UNCHECKED: whether linksim models idle flicker at all.
