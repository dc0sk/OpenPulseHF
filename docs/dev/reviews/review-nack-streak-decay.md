---
project: openpulsehf
doc: docs/dev/reviews/review-nack-streak-decay.md
status: resolved
last_updated: 2026-10-02
---

# Adversarial review — NACK streak decay (#1456, #1460; M2)

One round by Fable, held before implementation. **Mandatory** (decision 8): the design concerns when a
receiver keys NACKs. The reviewer was read-only and was prompted for falsification. Target: the draft
[`design/nack-streak-decay.md`](../design/nack-streak-decay.md).

## Consumer

`ota_decode_and_ack_inner` → `OtaRateController::on_rx_frame(RxOutcome::Failed, ..)`, from the daemon
receive tick's OTA arm. Found by `grep -n "on_rx_frame(RxOutcome::Failed" crates/openpulse-modem/src/engine.rs`.

## Prior art

#1452, #1454, #1255, #1142 and the daemon's `OTA_NACK_BUDGET`. Found by `grep -n "#1452\|#1454\|#1255\|OTA_NACK_BUDGET"
crates/openpulse-modem/src/engine.rs crates/openpulse-daemon/src/server.rs`.

## Twins

linksim drives its own controller in lockstep, with no idle and no foreign overs, so the rule is
unreachable there by construction. ARDOP does not use the controller, and its ISS steps down on a
missing ACK. Found by the reviewer (`apps/openpulse-linksim/src/lib.rs`, `crates/openpulse-ardop/src/bridge.rs`).

## Prompt

Falsify, do not agree, with file:line evidence:
1. Is the captured-sample clock sound, across own TX, blocking decodes and stream resets?
2. Is `T_gap` right per rung, against fade retransmission spacing, the #1456 flicker rates and
   foreign over spacing? Is keying it to `rx_confirmed` right?
3. How does it interact with NackHold and NackStepDown, the fast downshift, the daemon's
   `OTA_NACK_BUDGET`, HARQ retention and the climb streak?
4. Is rejecting the liveness rule correct, and is there a better discriminator today?
5. Does the #1460 residual matter for A2 on 2 m?
6. What about the twins (linksim, ARDOP)?

Does this block Release 1? If not, say so.

## Verdict

**Not implemented; parked.**
- **Release 1: no.** A2 (`release-1.0-criteria.md` §A2) is scored on clean-decode windows, a used
  climb and a logged demotion. Clean decodes reset the streak, and on 2 m the exchange cadence keeps
  real failures inside any `T_gap`. #1456 bites between sessions; the #1460 residual is HF only.
- **Decay alone does not fix #1456 where it matters** (finding 2). That is also the reason the
  design was not implemented.

| # | Finding | Class |
|---|---|---|
| 1 | The draft's `T_gap` numbers used raw geometry. By its own `frame_plan` rule: SL2 316 s, SL5 55 s, SL6 37 s, SL7 25 s | FIX |
| 2 | At SL2, the entry rung and where an idle HF session sits, decay barely helps. A Monte-Carlo at 12 flickers/h gives 4.0 demotions/h today and 2.45/h with the decay. From SL2 a demotion goes to SL1, which cannot decode the peer's SL2 frames | FIX (sinks the design for #1456) |
| 3 | The daemon's `consecutive_ota_nack` / `OTA_NACK_BUDGET` counts every keyed NACK and resets only on a decode. After three spaced idle flickers the receiver goes mute; a failed first frame after idle gets no NACK, and the ISS abandons after 2 silent windows. The draft's claim of "fewer NACKs keyed" was false, because it changes no keying | FIX — a present-day defect, recorded in the work plan |
| 4 | The sample clock is sound while receiving. It stops during own TX, which only shortens gaps (safe direction). The recorded position must be defined (burst end) | NOTE |
| 5 | Keying to `rx_confirmed` is right on air (`FastDownshift` is dormant, #1438) | NOTE |
| 6 | Rejecting liveness is correct. Duration-vs-frame-length was already rejected for fragments; the preamble veto covers BPSK250 only. The real discriminator is #1062 | NOTE |
| 7 | HARQ retains LLRs for every failed burst, BEFORE the evidence guards, so flickers' LLRs are combined with the next real frame at SL7+ | NOTE — recorded |
| 8 | A spaced failure still zeroes the climb streak | NOTE |
| 9 | Twins: linksim cannot reach the rule; ARDOP's ISS steps down on a missing ACK, so "no sender fallback" is true of `server.rs` only | NOTE |

**Falsifier for any redesign:** a recorded 250 Hz idle at SL2 through `accumulate_capture` for 90 min
must give zero demotions, **and** a following failed BPSK31 frame must still key a NACK.
