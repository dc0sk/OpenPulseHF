---
project: openpulsehf
doc: docs/dev/design/bound-the-uncoded-fallback.md
status: draft
last_updated: 2026-11-11
---

# Bound the uncoded fallback on every failed burst

Work plan M4, "before the HF trip" (the parked row on the uncoded-fallback stall). The maintainer,
2026-11-11: "Bound the fallback now". It changes the receive path that delivers non-ladder traffic
(station IDs, filexfer, handshake, QSY, relay, send-message bodies), so it is reviewed before it is
built. Review: pending.

## Problem — measured

The #1123 uncoded fallback runs on **every failed OTA burst** that no ladder candidate decodes. Its
three sites, on one 16 k-sample failed burst (x86, release; a probe through `accumulate_capture` and
`ota_decode_burst` with the fallback mode BPSK250, as the daemon passes it):

| site | what it does | time |
|---|---|---|
| ranked | the fallback mode's preamble ρ profile at the correction ± one grid step, then decodes at the top 4 peaks (`FALLBACK_RANKED_ONSETS`) | 58 ms |
| deferred exhaustive | `decode_fallback_near_trigger`: up to 64 uncoded decodes at the onsets nearest `trigger − 1024` (`OTA_FALLBACK_RESCUE_ONSETS`, `…_LEAD_BIAS`) | 332 ms |
| phase-2 fallback entry | `acquire_burst_correction_span` at up to 64 onsets, a settle and the veto at each, then a decode | 358 ms |

- **On the Pi 5 the whole failed-burst processing was about 1.5 s** (f2q). That is above the 0.5 s
  evidence floor: the next read is then one long read, itself a floor-passing burst
  (`reply-window-evidence.md`).
- **Union-ACK pieces now skip all three sites** (#1540).
- **Every other failed burst still pays about 750 ms**: a failed ladder frame, a fade-split fragment
  on HF, a foreign signal.

**What the expensive sites are worth, from the on-air logs** (every daemon log under
`docs/dev/test-reports/` on `m3/onair`, tasks 18–36):
- **About 20 fallback decodes in all**, almost all of them station IDs.
- **4 of them recovered only by the deferred exhaustive scan**
  (`WARN … ranked below the top 4 correlation peaks and was recovered only by the exhaustive scan`).
  All four were BPSK250 at onsets **2208, 2432, 2208, 2208**, about at the trigger.
- **Phase-2 fallback decodes are not logged apart** from the other fallback decodes: UNCHECKED how many
  there were.
- **So the deferred scan rescues about one fallback frame in five.** It cannot simply be skipped.
- **Its scan order puts those rescues late.** The 64 onsets are ordered nearest
  `trigger − 1024`, at BPSK250's 32-sample step, so they span `trigger − 2048 … trigger`. A frame at
  about the trigger is about 1 024 samples from the centre, which is about the 60th onset tried. A
  count cut keeps the onsets that never rescued and drops the ones that did.

## Candidates (for the review to choose or refute)

**A, a wall-clock budget for the two expensive sites, with a better order (proposed).**
- The deferred exhaustive scan and the phase-2 fallback entry share one budget per burst,
  `FALLBACK_BUDGET` (proposed 250 ms of wall time, from entry to the deferred site).
- On expiry, both stop. The burst is "not decoded by the fallback", a counter
  (`fallback_budget_spent`) rises, and the run logs it at info, as the ACK deadline's stop does.
- **The onsets inside the budget are tried best-first:**
  - first by the ρ the ranked site already computed at each onset (the profile it builds covers every
    offset up to `scan_end`, so this costs nothing);
  - then nearest the trigger.

  That puts the frames the on-air rescues found (high local ρ, near the trigger) inside any budget.
- **What it bounds:** the stall, which is what matters for the 0.5 s floor, on any CPU. A slower Pi
  covers fewer onsets in the same 250 ms, and the counter says how often.
- **The precedent:** the ladder decode already stops at its ACK deadline. The fallback is exempt from
  that deadline because non-ladder traffic answers no ACK, and this gives it its own bound.
- **The cost:** non-deterministic coverage. Tests inject the budget (a private field), so they stay
  deterministic.

**B, a ρ gate per onset, from the profile the ranked site already computed.**
- In the deferred scan, decode only at onsets whose ρ at the current correction clears the veto's
  effective threshold. The threshold is #1060's calibrated, per-mode one, with the stand-down: no new
  constant.
- **The catch:** the profile covers the correction ± one grid step (about 1 Hz at BPSK250). A frame
  off-frequency by more has a low ρ there. That frame is exactly phase 2's case, so phase 2 cannot be
  gated this way.
  - A profile wide enough for ±50 Hz is about 100 frequencies: about 2 s, worse than the stall.
- So B bounds the deferred scan, but not phase 2. It also needs the four rescued frames' ρ: UNCHECKED
  whether they clear the threshold.

**C, re-centre the rescue on the trigger and shrink it.**
- Centre at the trigger instead of `trigger − 1024`, and try the 16 nearest onsets.
- That covers where the four rescues were, at a quarter of the cost, but **it reverses the review of
  2026-11-01**, which centred the rescue earlier because a frame's head can precede the trigger by up
  to the 2 048-sample ring.
- It needs the rescue frames' positions relative to the trigger, which is not logged: UNCHECKED.
- It does not bound phase 2 either.

**The proposal is A**, possibly with B's ρ ordering, which A already uses. It is the only candidate that
bounds the stall itself, on any CPU, and keeps every onset reachable when time allows. B and C each
leave phase 2 unbounded.

## What it does not do

- **It does not change the ranked site** (58 ms), which always runs.
- **It does not change the ladder candidates, the evidence rules or the ACK**, so it changes no keying.
- **It does not log phase-2 fallback decodes apart.** It should: a separate counter
  (`fallback_phase2_decodes`) is added, so the next on-air run measures what the budget costs there.

## Test plan (each must fail on today's main where marked)

1. **The budget bounds the stall** (fails today). A failed burst built so that the deferred and phase-2
   sites would run long (a long burst of noise at the fallback mode's floor). With an injected budget,
   the two sites stop, `fallback_budget_spent` rises, and the attempts are bounded. Use counters, not
   wall time.
2. **The order:** a BPSK250 ID placed at the trigger, behind a burst lead as on air (onset about 2 200),
   at an SNR where today's ranked site misses it, as on air. It is decoded inside a small budget, which
   it was not under the old order with the same attempt cap.
3. **Nothing is lost when time allows:** with the budget unlimited, every frame today's fallback decodes
   (IDs and control frames at each fallback mode, at offsets 0 / ±10 / ±25 / ±50 Hz and SNRs down to the
   decode edge) is still decoded.
4. **The existing fallback tests stay green:** `ota_arm_uncoded_dispatch`, `idle_flicker_is_not_evidence`,
   `union_ack_guard_tests`, and the `fallback-onset-ranking` tests.
5. **Sabotage:** the budget removed turns test 1 red; the old order turns test 2 red.
6. **The cost, measured again** with the same probe: a failed burst against the 830 ms baseline, x86,
   then via the station on the Pi 5.

Before merge: `scripts/gate.sh`, and `scripts/slow-tests.sh ota` and `spectral`. Read
`dsp-acquisition-playbook.md` first.

## Questions for the review

1. **Is a wall-clock budget acceptable in a receive path,** given the ACK deadline's precedent? Or must
   the bound be a count, for determinism, with the cost of a count being CPU-dependent stall?
2. **Is ordering by the ranked site's own ρ profile sound for the deferred scan's onsets?** The profile
   covers only the correction ± one step. Is the "nearest the trigger" tie-break right for phase 2,
   which acquires off-frequency?
3. **250 ms:** the stall bound against the 0.5 s floor, on any CPU. Too tight or too loose?
4. **Can the four on-air rescues be reconstructed** to test (2) faithfully? No audio was kept for those
   runs.
5. **Does this block Release 1?**

## Consumer

`ModemEngine::ota_decode_and_ack_inner`'s deferred fallback and phase-2 fallback entry, reached from the
daemon receive loop through `nack_budget::decode_ota_burst`.

## Prior art

#1123, `fallback-onset-ranking.md` (ranked, then exhaustive), the bounded rescue of 2026-11-01
(`OTA_FALLBACK_RESCUE_ONSETS`, `…_LEAD_BIAS`, the task 14 stall), the ACK deadline (`decode_deadline`,
task 18), and `ack-piece-skips-fallback.md` (#1540).

## Twins

The two expensive sites share the budget. The ranked site is unchanged. ARDOP and KISS run no OTA
decode.
