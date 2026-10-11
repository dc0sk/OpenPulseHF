---
project: openpulsehf
doc: docs/dev/design/failed-burst-ownership.md
status: draft
last_updated: 2026-10-11
---

# A failed burst is ladder evidence only if it looks like our frame (#1460)

Work plan M2, *Next, no maintainer input needed*, item 2. It follows
[`reply-window-evidence.md`](reply-window-evidence.md) v3, which left this case open (*What it does not
do*, first bullet): a foreign over longer than the 0.5 s evidence floor still counts as a failed
frame, keys a NACK and moves the ladder down. Decision 22 keeps #1460 open until this lands. **It
changes when a NACK is keyed, so it is reviewed before it is built (decision 8).**

> **Round 1 (2026-10-11): not implemented as written**
> ([`reviews/review-failed-burst-ownership.md`](../reviews/review-failed-burst-ownership.md)). Four
> blocking findings:
> - the false-match rate is minimised over 10³ wires per burst, so D = 16 refuses almost nothing;
> - a steady tone at baud/2 descrambles to 17 bits from the header;
> - SL1 loses every acquisition failure;
> - a refused frame of ours turns a NACK into ISS abandonment, on 2 m as well.
>
> **Not a Release 1 blocker.** The decision below stands as the round-1 text.

## Problem

During an OTA session, a burst that fails every candidate decode is ladder evidence once it clears the
#1452 recognition window, the #1456 evidence floor (0.5 s) and the ACK check. Evidence means
`on_rx_frame(Failed)` runs and the daemon keys a NACK, within its leaky budget of 3. Three in a row
are one demotion step.

The burst can be anything that opened the carrier detect. Since #1454's spectral detect, that
includes foreign narrowband overs in the passband: CW, PSK31, RTTY. Each over longer than 0.5 s is one
failed frame. A busy band therefore demotes a working session and keys NACKs into other stations'
QSOs.

## What can tell our frame from someone else's

Two signals, and only the second covers every rung where the problem lives:

1. **The preamble template (#1062).** Since the flag day BPSK250 publishes a PN-63 template that
   refuses every lone tone across fc ± 200 Hz (worst ρ 0.248 against 0.315). It covers one rung, and
   `rx_candidates()` has at most two rungs (the recommended one and the confirmed one), so a
   template-only rule refuses a foreign burst only while the session sits at SL5. #1062 F3 also
   measured the template matching whitened same-baud payload at up to about 0.5, so it does not refuse
   a foreign BPSK250 data stream either.
2. **The frame header in the clear.** SL1–SL6 carry `Rs` (`docs/mode-fec-ladder.md`). RS here is
   systematic, and the hard wire handed to `decode_variants` is already descrambled
   (`stage_demodulate_variants`). So a frame of ours, even one RS cannot correct, carries in its first
   nine wire bytes the 4-byte big-endian length prefix (the top two bytes always 0 for any frame the
   ladder sends), `OPLS`, and the version byte (1): **7 fixed bytes, 56 bits**. A foreign over
   demodulates to bits uncorrelated with that header.

SL7–SL14 (OFDM, `SC` and `LHR`) have no systematic header in the hard wire. They are wideband, so the
spectral detect is not what gathers them, and #1460's narrowband overs are not their case.

## Decision (proposed)

**Header evidence.** For each failed burst, the engine records the smallest Hamming distance, in
bits, between the 56 known header bits and the corresponding bits of every wire it tried at an RS
candidate. That covers every onset, arm and timing lock `decode_variants` saw for this burst, but only
the first wire block's header, never the parity.

A failed burst is ladder evidence, in addition to today's guards, only if one of these holds:
- **(a)** some candidate is not an RS rung, in which case today's rules alone decide; or
- **(b)** the burst's best header distance is at most **D** bits.

**D** is set by measurement M1 below, before M2 is run.

- **Where:** the distance is updated in `decode_variants`' failure arm, the one seam every hard RS
  chain goes through. It is reset per burst in `ota_decode_and_ack_inner` and read at the existing
  not-evidence guards.
- **Not the template:** it covers one rung, and it is redundant where the header applies. It stays a
  candidate refinement for later.
- **Not a session ID:** a frame from another OpenPulseHF station carries the same header and still
  counts. That is stated, not solved; the header has no session field.
- **HARQ:** retention follows the evidence decision, as it already does (#1502), so a refused burst
  retains no LLRs.

## Measurements, pre-registered (rules fixed here, before any number exists)

Every measurement runs through the production entry: `accumulate_capture`, the flush, then
`ota_decode_burst`, as in `idle_flicker_is_not_evidence.rs`. Each runs on SL2–SL6 under the `fast`
profile, locked at the rung.

- **M1, own frames.** Our frames, failing:
  - **Channels:** `moderate_f1` at the rung floor − 2 dB and − 4 dB, and AWGN 3 dB below the cliff.
  - **Sample:** 200 seeds each, payload 64 B `Rs`, in the 250 Hz and wide idle recordings.
  - **Recorded:** every failed burst that today's rules count, with its best header distance.
  - **Rule:** **D** is the smallest distance that keeps at least **99 %** of those counted bursts, per
    rung, taken as the maximum over rungs.
  - **Fails if** that D exceeds **16** bits: at 16, a random 56-bit word lands within D with
    probability about 2 × 10⁻⁴, the limit at which M2 can pass. The design then falls back to
    template-only at SL5 and parks the rest.
- **M2, foreign overs**, at the D from M1:
  - **Signals:**
    - CW, keyed at 20 wpm;
    - real BPSK31 varicode text, a different modulation for SL3–SL6 and the same one for SL2;
    - RTTY, 45.45 Bd 170 Hz;
    - a carrier pair at fc ± 31.25 Hz.
  - **Conditions:** each at +6, +12 and +20 dB over the idle, 60 s per cell, both idle recordings.
  - **Rule:** **at most 1 %** of the failed bursts today's rules count may still count. A miss is
    reported per signal and not averaged away.
- **M3, cost.**
  - **What:** the Hamming check is per wire, on 7 bytes; `receive_cost_scaling` re-measures decode
    per frame on x86.
  - **Rule:** no rung slower by more than 2 %.

## What it does not do (stated)

- Another OpenPulseHF station's frames, ours or not, still count.
- On SL7+ (OFDM), a foreign wideband over still counts; that was never #1460's narrowband case.
- A frame of ours whose first block's header is lost entirely, for example a fade null over the first
  2 s of BPSK31, is refused, where today it counts. M1 measures how often, and the 99 % rule bounds it.

## Tests (to write with the implementation)

- `a_foreign_narrowband_over_is_not_evidence`: a CW and a PSK31 over at +12 dB, at SL3, through
  `accumulate_capture`. It keys no ACK frame, and the rung holds. Sabotage: drop rule (b), and
  the over counts.
- `a_failing_frame_of_ours_is_still_evidence`: our SL3 frame at the floor − 4 dB fails and is still
  answered. Sabotage: D = 0, and it is dropped.
- The existing `idle_flicker_is_not_evidence` and `cap_flush_is_not_ladder_evidence` stay green.

## Consumer

The not-evidence guards in `ota_decode_and_ack_inner` → `on_rx_frame(RxOutcome::Failed)` → the daemon
OTA arm's NACK keying (`on_ladder_burst`); the HARQ retention gate (#1502). Found by
`grep -n "not ladder evidence" crates/openpulse-modem/src/engine.rs`.

## Prior art

`reply-window-evidence.md` v3 and its three review rounds; the #1062 veto and F3's same-baud
measurement; `holds_an_ack_codeword` (the ACK check, same shape: recognise a known codeword in a
failed burst). Found by `grep -n "fn holds_an_ack_codeword\|fn decode_variants" crates/openpulse-modem/src/engine.rs`
and `grep -n "^const MAGIC\|^const VERSION" crates/openpulse-core/src/frame.rs`.

## Twins

- **ARDOP** (`receive_with_ack_hint`) keys its NACK on any failed one-shot receive and does not use
  the OTA evidence rules. It is unchanged by this, and stated here so a reader does not assume
  otherwise.
- **linksim** drives `on_rx_frame` directly, without bursts.

linksim was found by `grep -rn "RxOutcome::Failed" crates apps --include=*.rs`. ARDOP was found by
`grep -rn "receive_with_ack_hint" crates/openpulse-ardop/src`, because that grep cannot see it.
