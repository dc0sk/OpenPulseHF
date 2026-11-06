---
project: openpulsehf
doc: docs/dev/reviews/review-ladder-climb-into-silence-2026-11-05.md
status: resolved
last_updated: 2026-11-05
---

# Review: a climb into silence (P3) and SL7's peak (P1) (2026-11-05)

Adversarial review (Fable) of `docs/dev/design/ladder-climb-into-silence.md`, as a design before the
implementation landed. It is mandatory: P3 changes when and at what level a station transmits, and P1
changes what it transmits.

## Prompt

The prompt asked for falsification, not agreement:
- P3's lockstep under lost ACKs and NACKs, the dead ACK path, the SL1 sub-floor, the riding ID and
  `transmit_arq_ota`;
- what clears the record;
- the retry budget and the blocked control loop;
- the re-probe cost;
- P1's level, CE-SSB and the audio backends;
- the P2(a) caveat.

It also asked "Does this block Release 1?".

## Verdict

**P3 is the right shape. The f2c stall blocks Release 1 (A2 stability), so P3 is needed.**
- As designed, P3 had two holes: a NACK cleared the record, and a fallback could land on the SL1
  sub-floor.
- P1 is right by contract, but its consequence is larger than the design said.

## Findings and what was done

| # | Finding | Done |
|---|---|---|
| M1 | A NACK cleared the record. One keyed NACK at a marginal rung, then silence, leaves the sender stranded: f2c again | **Taken:** only a decode-path ACK clears it (`adopt_recommendation_from(level, nack)`). Test `a_nack_after_the_climb_keeps_the_way_back`; sabotage fails it |
| M2 | A fallback onto SL1 (MFSK16) with a body over 209 B keys PTT with nothing radiated, and the receiver (never told SL1) hears nothing: decision 3's permanent stall. Measured: `MFSK16 frame carries 255 bytes; got 510` | **Taken:** never onto an MFSK16 rung. Test `never_falls_back_onto_the_mfsk16_sub_floor`; sabotage fails it |
| S1 | The design's lockstep claims fail once the receiver has decoded at the new rung (the old rung is then not a candidate). The dead-ACK-path claim was false | **Taken** into the design note: recovery by NACK, and the a2g shape settles one rung lower |
| S2 | The re-probe: with SL8 inaudible, ≈62 % of wall time in silent windows, ≈390 instead of ≈1030 messages/h | **Taken:** a sender-side backoff, 4 declined climbs doubling to 32. Test `a_climb_back_to_a_rung_that_went_silent_waits_and_the_wait_doubles`; sabotage fails it |
| S3 | The `MAX_SILENT_ACK_WINDOWS` comment became false; the edge where the budget ends at the fallback | **Taken:** comment; test `a_fall_back_on_the_last_attempt_moves_the_level_but_sends_nothing_more` |
| S4 | No event for a sender-side level change; A2 scoring cannot tell it from a step-down | **Taken:** `EngineEvent::OtaFallBack { from, to }` and an `info` line; the panel shows it, the TUI ignores it |
| S5 | P1's numbers: SL7 goes to −10.5 dBFS (not −9.6), so SL6 → SL7 is −6.2 dB. On the stations SL7 was not clipped (f32 through PipeWire's float volume), so its +9.9 dB was real RF. B→A will top at SL6 | **Taken** into the design note, with the expected reading of the next f2 run |
| S6 | P2(a)'s caveat: the estimators are on different scales (f2c SL6 9.6 dB, SL7 6.0 dB with SL7 3.7 dB louder) | **Taken:** P2(a) needs a per-mode calibration first; not implemented in this round |
| S7 | `transmit_arq_ota` has no Release 1 on-air caller | **Taken** into Twins |
| N1 | A lock or new bounds left a record set before them | **Taken:** both clear the record and the backoff |
| N2, N3, N4 | A raise by more than one rung after a fallback; an overwritten record is always receiver-confirmed; without the backoff the oscillation is unbounded in time | Recorded |

## The implementation's own test, and two sabotages that proved nothing

- **The first version of `climb_into_silence_falls_back` passed with the fallback disabled.** Its muted
  channel fed `generate_noise`, louder than the channel's idle output. So B saw a burst, failed it,
  NACKed and stepped down by itself. The muted channel now delivers exactly what the channel makes of
  silence.
- **The second sabotage, `o.fall_back_after_silence() && false`, still called the fallback** (`&&`
  evaluates its left side). The level moved while the send was reported abandoned, and the test passed.
  `false && …` is the real sabotage: timeout at 400 s, fails.
