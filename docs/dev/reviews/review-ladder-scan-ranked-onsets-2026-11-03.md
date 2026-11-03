---
project: openpulsehf
doc: docs/dev/reviews/review-ladder-scan-ranked-onsets-2026-11-03.md
status: resolved
last_updated: 2026-11-03
---

# Review: rank the ladder scan's onsets before the walk (2026-11-03)

Adversarial review (Fable) of `87ce9b6` (then uncommitted) and its design note
`docs/dev/design/ladder-scan-ranked-onsets.md`. Recommended, not mandatory: it adds a constant to a DSP
path and changes the order of the OTA receive's decode attempts; it keys nothing.

## Prompt

The prompt asked for falsification, not agreement:
- **reordering:** whether the ranked entries can lose a frame the walk would find, or change which
  frame or rung decodes first;
- **candidate precedence** and **two frames per keying** (#1461);
- **cost:** the bound on extra attempts;
- **the constant:** what inventory `ENERGY_RISE_FRACTION` was fitted to, and what would falsify it;
- **the A/B replay's credibility** and **the tests' quality**.

It also asked "Does this block Release 1?".

## Verdict

**No blocker; does not block Release 1.** The energy-rise entries are a strict subset of the walk's grid,
go through the same loop body, and so cannot lose a frame the walk would find. The coded ladder path
never calls `decode_following_frames`. The sabotage logs and the A/B replay (one build, a switch per
arm) were checked from their files.

## Findings and what was done

| # | Finding | Done |
|---|---|---|
| S1 | `energy_rise_onsets` was inserted between `onsets_nearest`'s doc and its `fn` | Moved back |
| S2 | The constant's rationale ("noise below a third") contradicts its own burst: opener 0.43 of P80, frame 0.68–0.75; 0.4–0.9 work, 0.3 fails; useless near a rung's cliff | Doc comment and design note carry the numbers and the strong-signal regime |
| S3 | The test's opener was 25 dB below the frame, so it did not pin the constant | Second case at 0.4 of the frame's RMS: 2 attempts; with the fraction at 0.3, 107 and it fails |
| S4 | The replay harness and its switch are not in the tree | The note says so: scratch code at `87ce9b6`, recordings not in the repository |
| N1 | "At most five attempts" understates: 5 × 2 passes × 2 candidates, plus off-grid preamble peaks as additions | The note states the bound and that the BPSK250 case is unmeasured |
| N2 | 2.7 s vs 2.86 s quoted | Quoted as 2.4–2.9 s (three replays) |
| N3 | Ranked decodes land 1–2 steps later, shifting `decoded_span` | One line in the note; ACK decisions unchanged in the A/B |
| N4 | The BPSK250 preamble profile is computed twice per burst | Accepted, noted |
| N5 | No unit test for `energy_rise_onsets`' edges | `energy_rise_onsets_edges` |
| N6 | Ledger and acceptance rows missing | Added |
| U1 | Preamble peaks in correlation order, not time order (#1461 if ever two frames) | Sorted by time |
| U2 | Phase 2 and the non-OTA `decode_burst` still walk from 0 | Stays UNCHECKED in the note's Twins |
