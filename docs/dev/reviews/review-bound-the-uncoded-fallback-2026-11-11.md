---
project: openpulsehf
doc: docs/dev/reviews/review-bound-the-uncoded-fallback-2026-11-11.md
status: resolved
last_updated: 2026-11-11
---

# Review: bound the uncoded fallback (2026-11-11)

- **Subject:** `docs/dev/design/bound-the-uncoded-fallback.md` (draft), on branch
  `design/bound-the-fallback`.
- **Reviewer:** Fable, read-only.
- **This review is recommended:** a receive-path change, with no change to keying.

## Prompt

The reviewer was asked to falsify six things:
1. the scan-order analysis (the four on-air rescues at about the 60th onset), by recovering each
   rescue's trigger from the logs;
2. candidate A, a wall-clock budget with ρ-first order (determinism, the ACK-deadline precedent, a slow
   Pi, 250 ms against the 0.5 s floor);
3. ρ-first ordering for the deferred scan and phase 2;
4. any better bound;
5. which non-ladder traffic is at stake in Release 1;
6. the test plan.

It was also asked: **"Does this block Release 1?"**

## Verdict

**APPROVE WITH CHANGES. A's budget is demoted to a backstop; the bound is a count over a re-ordered set.
It does not block Release 1:** the stall costs a floor-passing read, not a frame.

**Framing errors:**
1. **"Phase-2 fallback decodes are not logged apart" is false.** `ota fallback decoded … after
   acquisition` occurs 0 times on air.
2. **Two of the four rescues are reconstructible.** Their leads were 2248 and 2448, so their frames sat
   40 and 16 samples before the trigger. The other two are not reconstructible.
3. **"About 60th" is understated:** they were ranks 61 and 63 of 64.

**The answers:**
- **Q1, the scan order:** ordering the same set by distance to the trigger puts the rescues at ranks 1
  and 0, which is a pure reorder.
- **Q2, the wall-clock budget: reject it as the primary bound.**
  - `off_frequency_fallback_is_not_ladder` runs at +100 Hz, which only phase 2's fallback entry
    recovers, and in the debug-profile gate that takes seconds.
  - It would re-create #1066 in the receive path.
  - The ACK deadline is a protocol timer, not a performance bound, so it is no precedent.
  - 250 ms bounds only two of the five sites.
- **Q3, ρ-first ordering: unsound for exactly the population it serves.** The rescues were below the
  top-4 ρ peaks.
- **Q4, better bounds:**
  - gate phase 2's fallback entry on one settle at the trigger (cost ≈ one settle; the site's on-air
    yield is 0);
  - order the deferred scan by trigger distance and cap it at 16 (332 → about 85 ms);
  - not "skip when ρ is far below the threshold": the rescues are the ρ-miss population.
- **Q5, the traffic at stake:** every non-ladder frame passes this path.
  - The station ID is display only.
  - Send-message bodies and the handshake answer no ACK, so a miss is a silent loss.
  - The changes lose only a frame that is both off-frequency beyond the deadband and more than 8 symbols
    from the trigger. That has never been observed.
- **Q6, the tests:** the second test cannot be faithful without audio. Build it with K forced to 0, as an
  order test. Add the off-frequency and uncoded-dispatch tests.

**The changes:**
1. a deterministic count;
2. the phase-2 settle gate;
3. drop ρ-first ordering;
4. instrument the WARN;
5. the tests;
6. the doc corrections.

## Disposition

All six are applied (see the design's "As built"). The WARN logs `lead` and `from_trigger`. The ρ rank
is not logged, because the profile is not kept past the ranked site; that is stated. The measured cost
is cut by about 2.5× (on frequency 722 → 288 ms; at +100 Hz 1 258 → 507 ms, x86). On the Pi 5 an
on-frequency failed burst stays just above the 0.5 s floor, as stated, and the remaining sites are not
in this change.

## Consumer

`ModemEngine::ota_decode_and_ack_inner` (the deferred and phase-2 fallback sites), through
`nack_budget::decode_ota_burst`.

## Prior art

#1123, `fallback-onset-ranking.md`, the bounded rescue of 2026-11-01, and #1540.

## Twins

Both expensive sites take the shared `fallback_rescue_onsets`.
