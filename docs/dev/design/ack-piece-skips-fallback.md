---
project: openpulsehf
doc: docs/dev/design/ack-piece-skips-fallback.md
status: resolved
last_updated: 2026-11-10
---

# A union-ACK piece skips the uncoded fallback

Work plan M4, "before the HF trip" (the parked row on the uncoded-fallback stall). It changes the
receive path that delivers non-ladder traffic (station IDs, filexfer, handshake, QSY, relay, send-message
bodies), so it was reviewed before it was built.
- **The review: Fable, 2026-11-10, APPROVE WITH CHANGES**
  (`docs/dev/reviews/review-ack-piece-skips-fallback-2026-11-10.md`).
- Its four changes are applied: see "The design, as built". **The condition below under "Design" is
  superseded.**
- **Implemented 2026-11-10.**

## Problem — measured

- **On air** (task 36, f2q, the Pi 5), each piece of an SL1 K=3 union ACK heard in the sender's burst
  path held the receive loop for **about 1.5 s** (`capture read holds 1.3–1.5 s`, 30 times).
- **The cause, measured 2026-11-10.** A temporary probe sent a real MFSK16-ACK copy, cut from the union
  waveform, over noise into an SL1-clamped receiver through `accumulate_capture` and `ota_decode_burst`
  (x86, release):
  - with the uncoded fallback mode set (BPSK250, as the daemon passes it), the whole failed-burst
    processing took **830–1 160 ms**;
  - without it, **81–84 ms**.
- **Per phase, one piece (830 ms):**

| phase | time |
|---|---|
| fallback, ranked onsets (`ranked_fallback_onsets` + `decode_at_ranked_onsets`) | 58 ms |
| the ladder onset scan (MFSK16 is too short for the piece, so skipped) | ~0 |
| **fallback, the exhaustive scan the ranking deferred** (`decode_fallback_near_trigger`) | **332 ms** |
| **phase 2, the fallback mode's acquisition entry** (BPSK250 None) | **358 ms** |
| the rest: the evidence rules and the union-ACK guard | about 80 ms |

- **So about 750 ms of each piece is the fallback searching for an uncoded frame** in a burst that is an
  ACK piece. At SL1 every ACK is a union ACK, so its three MFSK16 pieces cost the sender about 4.5 s per
  ACK on the Pi 5.
- **The stall is above the 0.5 s evidence floor.** A long read after it is itself a floor-passing burst
  ("one long read", `reply-window-evidence.md`).

## Design

Before the fallback's first phase, in `ota_decode_and_ack_inner`, at the point where every rung
candidate has failed at offset 0: if the burst **holds an MFSK16-ACK codeword and holds no FSK4-ACK
codeword**, it is a union-ACK piece. Skip all three fallback sites for it:
- the ranked attempt;
- the deferred exhaustive scan;
- the phase-2 fallback entry.

The ladder candidates still run, so a ladder frame keeps its first claim. The burst then reaches the
evidence rules and the union-ACK guard as today, and is "not ladder evidence".

**Why "no FSK4 codeword" is the safety condition:**
- **A ride-along station ID always goes in front of the ACK, in the same keying** (`server.rs`,
  `kind = "with-ack"`, `transmit` then `transmit_ota_ack`). The union ACK begins with its FSK4 copy.
- **So a burst that holds an ID with a union ACK also holds the FSK4 codeword**, whether it is heard
  whole or split at the gaps. The split falls in the 0.5 s gaps *after* the FSK4 copy; the ID and the
  FSK4 copy are contiguous.
- **That burst still gets the fallback**, and the ID is decoded as today.
- **Only the MFSK16 copies that follow the FSK4 copy are skipped.** They carry nothing but the ACK.

**What decides "holds an MFSK16-ACK codeword":** the same `holds_an_mfsk16_ack_codeword` (Costas anchor,
slots, keyless ShortFEC 5-byte decode) and the same head windows (`ack_head_lens`) as the union-ACK
guard. One recogniser, two uses. The FSK4 test is `holds_an_ack_codeword` on the FSK4 head.

**The cost it adds:** the two scans run earlier, before the fallback.
- **On a piece**, they then save the ~750 ms.
- **On a failed burst that is not an ACK piece**, they cost what the guard would have cost later
  (19 + 60 ms x86, real piece; less on noise, where the anchor fails).
- **Duplication:** the result is cached for the burst, so the guard does not run the scans twice.
- **Below one MFSK16 copy** (10 240 samples, which includes every failed OFDM frame), the MFSK16 check
  returns at once.

## What it does not do

- **It does not bound the fallback for other failed bursts.** A failed burst that is not an ACK piece
  (a failed ladder frame, a foreign signal) still pays the 58 + 332 + 358 ms.
  - That is the larger item. Possible routes: a correlation floor on the fallback mode's preamble
    before the exhaustive scan and the acquisition. That is a new DSP constant, with the inventory
    question. Or a time budget. It stays parked, with this note's measurement as its baseline.
- **It does not change what is keyed.** A union-ACK piece was "not ladder evidence" before and after;
  it only stops costing the fallback.

## Test plan (each must fail on today's main where marked)

1. **A union-ACK piece skips the fallback** (fails today). Use a tripwire counter (`ack_pieces_skipped`,
   readable in a unit test) that rises, while the fallback's own counters (`fallback_onset_ranks`, the
   deferred attempts) do not. Run it through `accumulate_capture` and `ota_decode_burst` with the
   fallback mode set. A wall-time assertion is not used: it is flaky under load.
2. **The safety condition** (must pass today and after): a station ID **in front of** an FSK4 ACK, and in
   front of a whole union ACK, in one keying through the same path. The ID is still decoded by the
   fallback (`fallback_decoded`, payload "DE …").
3. **Non-ladder frames are not taken for ACK pieces** (negative control): a BPSK250 station ID alone, a
   handshake CONREQ, a filexfer fragment and a QSY frame, uncoded at the active mode, clean and under
   noise. Each is still delivered by the fallback. This extends `no_data_frame_or_station_id_is_taken_for_an_ack`
   to the non-ladder frame set (from the frame types the daemon sends uncoded: closed list, UNCHECKED
   where it lives).
4. **The existing suite** (`idle_flicker_is_not_evidence.rs`, the `union_ack_guard` unit test, the
   `ota_arm_uncoded_dispatch` tests) is unchanged and green.
5. **Sabotage:**
   - with the "no FSK4 codeword" condition removed, test 2 goes red (the ID behind an FSK4 copy is
     skipped);
   - with the skip removed, test 1 goes red.
6. **The cost, measured again** with the same probe: one piece, with the skip, against the 830 ms
   baseline (x86). Then on rpi51 via the station.

Before merge: `scripts/gate.sh`, and `scripts/slow-tests.sh ota` (the OTA receive), since this touches
acquisition-adjacent code. Read `dsp-acquisition-playbook.md` first.

## Questions for the review

1. **Is "MFSK16-ACK codeword and no FSK4 codeword" a safe definition of a piece?** Can any non-ladder
   frame the daemon must receive contain an MFSK16-ACK codeword? Can a ride-along ID end up in a burst
   without the union's FSK4 copy (an FSK4 copy too weak to decode, while the MFSK16 copies decode,
   which is SL1's whole point)?
   - **That last case is the real risk.** In a deep fade the FSK4 copy can fail while the MFSK16
     copies decode, and then a burst with ID + (failed FSK4) + an MFSK16 copy would be skipped and the
     ID lost.
   - Is the ID's loss acceptable there, given that the ID is uncoded, at the active mode, and at that
     SNR would not decode anyway? Or must the condition be stated differently, for example "the burst
     is no longer than one MFSK16 copy + tail"?
2. **Is caching the recognisers' result for the burst sound,** given that the guard runs after the
   evidence rules and the probe runs before the fallback?
3. **Should the larger item** (bounding the fallback for all failed bursts) come first instead?

## The design, as built (the review's changes applied)

**1. The safety condition is geometric** (change 1). The first form, "an MFSK16 codeword and no FSK4
codeword", inferred structure from a decode that can fail.
- It would skip an ID in front of a union ACK whose FSK4 copy did not decode, heard as one burst
  (possible when hiss holds the squelch through the gaps).
- It would also skip a piece followed by another keying within the DCD hold.

**`is_a_lone_ack_copy` holds only when all three hold:**
1. **The length.** The burst is no longer past its lead than **one MFSK16 copy + the tail slack**
   (10 240 + 9 600 = 19 840 samples).
2. **The margin.** At the active fallback mode, the shortest station ID (`DE` + a three-character
   call, uncoded) + the FSK4 copy + the gap + an MFSK16 copy is **longer than that bound**.
   - That is the minimum a burst holding both an ID and an MFSK16 copy can be: about 25 000 samples at
     BPSK250.
   - At a mode whose short ID fits inside the bound, **nothing is skipped**.
   - The ID length is cached per mode.
3. **The recognisers.** The burst holds an MFSK16-ACK codeword, and holds no FSK4 codeword. The FSK4
   test is now only a tie-break.

The length is the safety condition. A Costas-anchor position bound (the review's preferred form) was not
used: it needs a measured "how late the DCD opens after a copy starts", from an audio inventory f2q did
not record. The length form needs no new constant.

**2. Send-message bodies** were missing from the non-ladder list (change 2). The negative control is
taken over the **modes** an operator can configure, not over frame content: content is whitened, and only
the waveform matters.

**3. AFC is restored to `afc_before` before the recognisers** (change 3). The candidate loop leaves the
last failed try's drift. The result is kept in a function-local variable, and the union-ACK guard reuses
it.

**4. Test 1 uses an entry counter** (change 4). `fallback_entries` counts every entry to the three
fallback sites. The existing `fallback_onset_ranks` counts only hits, so it cannot show a skip.

**Tests** (`engine::union_ack_guard_tests`, through `accumulate_capture` and `ota_decode_burst` with the
fallback mode BPSK250):

| test | shows |
|---|---|
| `a_lone_union_ack_copy_skips_the_uncoded_fallback` | `ack_pieces_skipped` = 1 and `fallback_entries` = 0 for the copy, which is still not ladder evidence |
| `a_station_id_in_front_of_an_ack_is_still_decoded` | the with-ack keying (`transmit`, then `transmit_ota_ack`) in front of an FSK4 ACK and of a union ACK: the ID decodes |
| `an_id_in_front_of_a_union_ack_with_a_lost_fsk4_copy_is_still_decoded` | the case only the length bound protects: one burst (hiss holds the squelch), the FSK4 copy zeroed, and the ID decodes |
| `a_mode_whose_short_id_fits_the_bound_skips_nothing` | the margin: at every registered mode whose short ID fits the bound, a real copy is not taken; at BPSK250 it is |
| `no_uncoded_frame_at_any_mode_is_taken_for_a_lone_ack_copy` | an uncoded ID at every registered mode, clean and under noise, is never taken |

The existing `no_data_frame_or_station_id_is_taken_for_an_ack` is unchanged and green.

**Sabotage** (release, each restored from `HEAD`):

| sabotage | goes red |
|---|---|
| S1, the length bound removed | `an_id_in_front_…_lost_fsk4_copy` (the FSK4 tie-break alone does not save it) |
| S2, the skip removed | `a_lone_union_ack_copy_skips_…` (`fallback_entries` ≥ 1 on today's path) |
| S3, the margin removed | `a_mode_whose_short_id_fits_the_bound_skips_nothing` (the first version had no test for it, so S3 passed unnoticed, and this test was added) |

**The cost, measured again** with the same probe (x86, release, 3 seeds): one piece took **82–86 ms**
with the skip, against **830–1 160 ms** before, which is the same as with no fallback mode at all. The
probe was a temporary unit test, removed after the run; the figures are not reproducible from the tree.
The Pi 5 figure follows from the station.

**Gate and slow suites:** see the PR.

## Consumer

`ModemEngine::ota_decode_and_ack_inner`, the fallback sites (#1123), reached from the daemon receive
loop through `nack_budget::decode_ota_burst`. Found with
`grep -n "fallback_mode\|decode_fallback_near_trigger" crates/openpulse-modem/src/engine.rs`.

## Prior art

#1123 (the uncoded fallback), `fallback-onset-ranking.md` (the ranked and deferred split, which already
cut the fallback's cost from 77–91 % of decode time), `union-ack-recognition.md` (the recogniser), and
#1461 (decode the following frames).

## Twins

The phase-2 fallback entry and the deferred exhaustive scan are the same fallback, so all three sites
get the skip. The ARDOP and KISS front-ends do not run the OTA decode.
