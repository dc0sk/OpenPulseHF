---
project: openpulsehf
doc: docs/dev/reviews/review-p-rehead-budget-2026-11-07.md
status: resolved
last_updated: 2026-11-07
---

# Review: re-head a hiss-opened burst at the preamble detect; P-confirmed failures past the NACK budget (2026-11-07)

Mandatory design review by Fable before implementing, because both changes alter when an ACK/NACK is
keyed. Design: `docs/dev/design/ofdm-preamble-detect.md`, Implementation subsection for the re-head
and the budget exemption.

## Prompt

The on-air run f2h (task 28, A's IC-9700 on AGC SLOW) sat silent for about 10 minutes while B sent 55
SL8 frames, and 51 of them decode offline. The reviewer was asked to falsify:
- **The trap:** the noise floor learning the post-frame AGC dips; total power opening a burst on hiss;
  each frame landing at the tail of a 66–92k-sample burst.
- Whether the trap exists under AGC FAST.
- **(a), the re-head:** restart a burst at the preamble detect's onset when P opens inside a burst that
  total power or the spectral test opened. The falsifier set: the f2h stretch replayed decodes ≥ 45/55.
- **(b), the budget:** whether a P-confirmed failure (coherence ≥ 0.8) should be exempt from the
  #1456 NACK budget.
- A floor-tracker fix at the root, the "no sender-side step-down on silence" recommendation, and
  Release 1.

## Verdict

- **(a) re-head: APPROVE WITH CHANGES.** Prototyped: the trapped stretch on the daemon path goes from
  1/56 to 52/56. The required changes:
  - re-head only on P's first open in a burst P did not open, beyond the decoder's reach from the head
    (`lead + first_block + 4·SYM_LEN`);
  - drop the head, do not flush it (a flushed hiss head would key a NACK on noise, the #1178 class);
  - discard the head from the floor;
  - re-point the held-fragment test, and add a within-reach one;
  - add a `preamble_detect_reheads` instrument.
- **(b) budget exemption: APPROVE.** The budget caps keying on what is not our peer's frame, and P is
  that statistic for OFDM52. The one reopened case, another OpenPulse OFDM52 sender, is bounded at
  3·(rung − 6) keyed NACKs before the ladder leaves OFDM.
- **A root-cause floor fix (do not learn the post-flush hang): REJECT as prototyped.**
  - It failed an existing idle gate: the 250 Hz idle at 171-sample reads read 1.369 against a bound
    of 1.35.
  - It froze the floor in the trapped state.
  - The proper plateau learner is parked as post-release.
- **A sender-side step-down on silence: REJECT.**
- **The mechanism found:** the daemon drops capture from a decode to the end of its own keying, so the
  floor is pulled down only after *unkeyed* failures. The budget's mute feeds the trap.
- **AGC FAST:** f2i–f2k show no trapped frames on air. The trap needs post-frame dips of about 1 s or
  more per 8 s cycle, and FAST gives a median of 0.2 s.
- **Release 1:** does not block. The implementation measured 1/56 → 52/56 on the full stretch, and
  0/5 → 5/5 on the committed 892 KB cut.

## Consumer

`ModemEngine::accumulate_routed` (the re-head, `rehead_at_p`) and the daemon's
`nack_budget::decode_ota_burst` (the exemption); found with
`grep -n "rehead_at_p\|p_evidence\|set_ota_budget_verdict" crates/openpulse-modem/src/engine.rs crates/openpulse-daemon/src/nack_budget.rs`.

## Prior art

`docs/dev/design/ofdm-preamble-detect.md` (P, the evidence rule), the #1456 NACK budget, and
`docs/dev/design/dcd-raised-floor.md` (the floor under a receiver AGC).

## Twins

P-opened bursts already take their head from `p_ring`; the re-head reuses `take_p_head`. The busy
mute (`channel_busy`) is unchanged. UNCHECKED: the replay-only case of hiss-held bursts with P off
(receiver at SL6 or lower while the sender is on an OFDM rung), parked in the work plan.
