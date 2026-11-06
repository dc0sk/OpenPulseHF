---
project: openpulsehf
doc: docs/dev/reviews/review-dcd-raised-floor-2026-11-04.md
status: resolved
last_updated: 2026-11-04
---

# Review: the DCD under a raised noise floor, and the NACK and step-down proposals (2026-11-04)

Adversarial review (Fable) of `338bbbd` and `docs/dev/design/dcd-raised-floor.md`.
- Decision 1 (implemented, new DSP constants): recommended.
- Decisions 2 and 3 (proposals that change when a station keys): the mandatory design review.

## Prompt

The prompt asked for falsification, not agreement. It asked about:
- the mechanism, against the recording;
- regressions against #1452's cases;
- the fitted ranks;
- the test's fidelity;
- the residual deafness;
- for decisions 2 and 3, the failure scenarios and the lockstep.

It also asked "Does this block Release 1?".

## Verdict

**Nothing blocks Release 1 on 2 m.** Decision 1 is strictly not worse than `commit`, and it fixes a2g.
It does not cover the HF regime (S1, S2). Decisions 2 and 3 should not land.

## Findings and what was done

| # | Finding | Done |
|---|---|---|
| — | The mechanism is confirmed: floor 0.0116–0.0122 under `commit`, exactly the live squelch over 1.25; but frames are 30–33 % of a slab, not 60 %, and the hang is 0.58 s (median), not 0.4 s | Corrected in the design note |
| S1 | Ranking by total power, the quantity an AGC regulates: near the AGC's knee frames do not rank above gaps, and the commit is a no-op | Recorded as an HF limit |
| S2 | The lower edge was fitted to a 0.6 s hang; a slow AGC (2–3 s) or SL2's ~87 % duty defeats it | **Measured:** fixed ranks 7/7, 1/7, 0/7 at 0.6, 1.0, 1.5 s. Replaced by a plateau rule (the 20 %-wide run that spreads least, starting at or below the median): 7/7, 6/7, 0/7. From 1.5 s the steady-state learner itself slides the squelch under the gaps, so no commit rule helps. **AGC FAST on the receiving rig is a Release 1 operating requirement** (task 22); a gap-plateau learner is parked |
| S3 | A 6–8 % low bias: the squelch is ~1.16× the hiss after a band commit | Recorded |
| T | The test modelled no hang and ran at 69 % duty; the lower edge was untested | The test now uses the on-air cadence with a hang and a 1 s recovery; sabotage with the quietest fifth: 0/7, fails; an `#[ignore]`d measurement covers 0.6–2.5 s |
| R | One cap of deafness: 37 s at SL5/SL6, 266–320 s with SL2 | Recorded |
| K | A decoded cap flush keys an ACK for a frame up to 38 s old; the ACK has no sequence number | Recorded as a proposal for the maintainer's go (keying) |
| D2 | The busy mute never acted (0 lines); the proposal reopens task 15 for weak frames | Parked |
| D3 | Silence is not rung evidence; a lockstep break walks the sender to SL1 with no way back | Not implemented; parked |
