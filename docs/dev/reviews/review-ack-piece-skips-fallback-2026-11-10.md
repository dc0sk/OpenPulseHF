---
project: openpulsehf
doc: docs/dev/reviews/review-ack-piece-skips-fallback-2026-11-10.md
status: resolved
last_updated: 2026-11-11
---

# Review: a union-ACK piece skips the uncoded fallback (2026-11-10)

- **Subject:** `docs/dev/design/ack-piece-skips-fallback.md` (draft), on branch
  `design/ack-piece-skips-fallback`.
- **Reviewer:** Fable, read-only.
- **This review is recommended, not mandatory:** a receive-path change, with no change to keying.

## Prompt

The reviewer was asked to falsify six things:
1. **The safety condition** "an MFSK16-ACK codeword and no FSK4-ACK codeword", especially an ID in
   front of an FSK4 copy too weak to decode;
2. the non-ladder frame set, as a closed list;
3. the ordering and caching (AFC state, flags, an SL1 data frame mistaken for a piece);
4. the measurement (830 ms = 58 + 332 + 358 + about 80);
5. whether the larger item (bounding the fallback for every failed burst) should come first;
6. the test plan.

It was also asked: **"Does this block Release 1?"**

## Verdict

**APPROVE WITH CHANGES. Not a Release 1 blocker:** in f2q every ACK was applied despite the stalls. It
is worth doing before HF.

1. **The condition is the wrong primitive**, because it infers structure from a decode that can fail.
   - **It misclassifies two bursts:** ID + (an FSK4 copy too weak) + gap + MFSK16 copy in one burst;
     and a piece followed by another keying within the DCD hold.
   - **State it geometrically instead:** a lone copy, by the Costas anchor's position and the burst's
     length. The fallback is `post ≤ copy_len + ACK_TAIL_SLACK_SAMPLES` (19 840), which excludes an
     ID at BPSK250 (25 024 minimum).
2. **The non-ladder set** comes from the daemon's `engine.transmit` sites: the station ID, filexfer
   fragments, handshake SAR fragments, QSY, relay envelopes, and **send-message bodies (missing)**.
   Take the control over the configurable modes, not over content.
3. **Ordering:**
   - `holds_an_ack_codeword` reads `afc_correction_hz`, which at the insertion point is the last failed
     candidate's drift, so restore `afc_before`;
   - cache the result as a function-local;
   - an SL1 data frame cannot be skipped;
   - the added cost, about 80 ms ahead of the ladder scan on every failed burst, is acceptable.
4. **The measurement holds.** Name where the probe lives, or label the figures unreproducible.
5. **Worth it alone on 2 m:** in f2q the 30 pieces were essentially the whole stall population. The
   larger item is the HF regime: keep it parked, and size it before the trip.
6. **Test 1 is vacuous as written:** `fallback_onset_ranks` counts hits only. Add an entry counter at
   all three sites. Test 2 must use the real with-ack keying.

**Framing:** "the FSK4 copy and the ID are contiguous" holds only under the 100 ms hold. The piece
population is a 2 m measurement.

## Disposition

All four changes are applied (see the design's "The design, as built"). The geometric condition is the
length form, plus a runtime margin check at the active mode. The anchor form needs an unmeasured DCD
opening delay. A margin test was added after sabotage S3 passed unnoticed. The probe figures are
labelled not reproducible from the tree.

## Consumer

`ModemEngine::ota_decode_and_ack_inner`'s fallback sites, through `nack_budget::decode_ota_burst`.

## Prior art

#1123, `fallback-onset-ranking.md`, `union-ack-recognition.md`.

## Twins

All three fallback sites (ranked, deferred, phase 2) take the skip. ARDOP and KISS run no OTA decode.
