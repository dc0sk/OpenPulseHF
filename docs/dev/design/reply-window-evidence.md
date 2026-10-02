---
project: openpulsehf
doc: docs/dev/design/reply-window-evidence.md
status: resolved
last_updated: 2026-10-02
---

# Idle flicker and ACKs are not ladder evidence; the NACK budget leaks (#1456, #1460, NACK-budget mute)

Work plan M2, ladder behaviour. Decision 21 keeps #1456 and #1460 in M2. This is **v3**, as implemented.
v1 (reply window, opening NACK) and v2 (reply window, half-frame floor) were reviewed and not implemented
([`reviews/review-reply-window-evidence.md`](../reviews/review-reply-window-evidence.md), rounds 1 and 2).
The file name is kept from v1, so the review links stay valid. This supersedes the parked
[`nack-streak-decay.md`](nack-streak-decay.md).

**Falsifiers:**
- From the decay review: idle through `accumulate_capture` gives zero demotions, **and** a failed frame
  still keys a NACK.
- From review round 1: idle keys at most 3 NACKs per 10 min.

## Problem

1. **#1456:** idle flicker that passes the #1452/#1454 guards demotes the ladder every third burst.
2. **#1460:** a foreign over counts the same way.
3. **NACK-budget mute** (decay review, finding 3): the daemon's count reset only on a decode, so three
   counted bursts muted the receiver until the next decode, and the ISS abandoned the peer's next
   failed frame after two silent windows.

## Measurements

**Idle flicker.** The recorded IC-9700 idle, cycled for 30 min through `accumulate_capture` with a `fast`
session pinned at the rung. "Evidence" means a flushed burst that passes today's guards; on air that is a
keyed NACK. Lengths are post-lead, i.e. after the pre-trigger ring (probe:
`tests/idle_flicker_evidence_rate.rs`).

| Rung | Capture | Flushed | Evidence | Per hour | Post-lead lengths |
|---|---|---|---|---|---|
| SL2 | 250 Hz | 1610 | 0 | 0 | — |
| SL5 | 250 Hz | 1610 | 1 | 2 | 1200 |
| SL6 | 250 Hz | 1634 | 107 | 214 | 800 (106), 1200 (1) |
| SL7 | 250 Hz | 1802 | 121 | 242 | not re-measured post-lead |
| SL5, SL6, SL7 | wide control | 28 | 0 | 0 | — |

Every flicker is two or three 400-sample daemon reads. It passes at SL6 because QPSK250-D's
recognition window is 544 samples. The `robust` profile tops out at SL6 and is meant for narrow
filters, so this case is reachable.

**Failing frames.** A frame was placed over the same 250 Hz idle at the given level against the
idle's RMS, then fed through `accumulate_capture`. "Counted today" lists the pieces that pass today's
guards (probe: `tests/marginal_frame_pieces.rs`).

| Frame | Level | Decoded | Pieces counted today (post-lead samples) |
|---|---|---|---|
| BPSK31 + Rs (66.6 s) | +6, +3, 0 dB | yes | — |
| BPSK31 + Rs | −3 dB | no | one piece, 49 600 (6.2 s), among dozens of 400–25 200 |
| BPSK31 + Rs | −6 dB | no | none: every piece ≤ 1200 |
| QPSK250-D + Rs (4.2 s) | +6 to −6 dB | no | one whole piece of 33 600–37 200, plus one 800 at −3 dB |

The shortest coded frame is fixed per RS block (measured with 1- and 16-byte payloads): SL1 17.0 s,
SL2 66.6 s, SL3 33.3 s, SL4 20.8 s, SL5 8.3 s, SL6 4.2 s, SL7 0.79 s.

## Decision

1. **Evidence floor** (engine, `ota_decode_and_ack_inner`). A failed burst with fewer post-lead
   samples than the floor is not ladder evidence: no count and no NACK.
   - The floor is `EVIDENCE_FLOOR_SAMPLES` = 4000 (0.5 s), or half the shortest candidate frame
     where that is less. The shortest frame is measured with `tx_airtime_seconds` on a 1-byte
     payload and cached.
   - 0.5 s is about 3× the longest flicker (1200) and an eighth of the shortest counted piece of a
     failing frame (4.2 s).
   - **Not half a frame**, because review round 2, finding 2 showed it would silence the 6.2 s piece
     above. The half-frame cap is only for OFDM rungs, whose whole frame is 0.79 s.
2. **An ACK is not evidence** (engine). A failed burst no longer than two FSK4 ACKs is scanned, lead
   included, for a ShortFEC ACK codeword. The scan is keyless and session-blind, so a foreign station's ACK on a keyed
   session is recognised too. An ACK (0.52 s) clears the floor at every rung, and without this rule two
   idle OTA stations NACK each other.
3. **Leaky budget** (daemon, `nack_budget.rs`).
   - A NACK is keyed only while fewer than `OTA_NACK_BUDGET` = 3 have been keyed since the last decode.
   - One comes back per `LEAK_SAMPLES` = 10 min of listening time, measured in samples fed through
     `accumulate_capture`.
   - A failure heard while muted adds nothing, so steady QRM cannot dig a debt that outlasts the leak.
   - Idle keying is therefore at most 3 + 6 per hour.

## What it does not do (stated)

- **#1460, foreign overs.** A foreign over longer than the floor still counts. With the leak it no
  longer mutes the receiver for good, but three such overs still demote. That needs a preamble
  correlation (#1062, after v0.17); it is HF only and not Release 1 (review round 2).
- **Uncoded rungs.** A 1-byte uncoded frame is short, so the half-frame cap brings the floor near the
  recognition window: no regression, but little protection.
- **HARQ** retains LLRs before the guards (decay review, finding 7). That is a separate work-plan row.
- **The sub-floor ACK.** When the recommendation is SL1, `transmit_ota_ack` sends an FSK4 copy
  plus three K=3 MFSK16 copies (about 5.4 s). That burst is longer than two FSK4 ACKs, so it is not
  scanned and counts like a failed frame at another idle station. The budget bounds the keying
  (review round 3, finding 3).
- **A weak ACK.** An ACK too weak to decode (below about +9 dB over the 250 Hz idle) counts like a
  failed frame.
- **One long read.** A daemon read after a blocking decode can hold seconds, so a single tripped read
  that long passes the floor.

## Tests

`tests/idle_flicker_is_not_evidence.rs` (default run), through `accumulate_capture` and
`ota_decode_burst`:
- `idle_behind_a_250hz_filter_is_not_evidence_at_sl6`: 5 min of the 250 Hz idle at SL6 gives no ACK
  frame and keeps the rung. About 18 are expected without the floor.
- `a_frame_failing_at_the_decode_edge_is_still_evidence`: QPSK250-D at 0 dB over the idle fails and
  is still answered.
- `an_ack_on_air_is_not_evidence`: another station's NACK at +6 dB is not answered.

`nack_budget.rs` unit tests cover three keyed then muted, the decode reset, the leak, and the idle
bound.

## Consumer

`ota_decode_and_ack_inner` → `on_rx_frame(RxOutcome::Failed)`; the daemon OTA arm's NACK keying. Found by
`grep -n "on_rx_frame(RxOutcome::Failed" crates/openpulse-modem/src/engine.rs` and
`grep -n "on_ladder_burst" crates/openpulse-daemon/src/server.rs`.

## Prior art

v1, v2 and their reviews; `nack-streak-decay.md` and its review; the #1452/#1454 guards; #1255;
`tx_airtime_seconds`; `fsk4_ack_at` and `ack_scan_span`. Found by
`grep -n "fn tx_airtime_seconds\|fn fsk4_ack_at\|fn ack_scan_span" crates/openpulse-modem/src/engine.rs`.

## Twins

- **ARDOP** does not use the OTA controller.
- **linksim** drives `on_rx_frame` directly (`apps/openpulse-linksim/src/lib.rs`), with no idle, so
  the floor is unreachable there.

Both are from review round 2.
