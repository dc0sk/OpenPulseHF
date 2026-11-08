---
project: openpulsehf
doc: docs/dev/reviews/review-ota-climb-memory-2026-11-08.md
status: resolved
last_updated: 2026-11-08
---

# Review: the receiver's climb memory and airtime-weighted climb evidence (2026-11-08)

Mandatory design review by Fable before implementing, because both changes alter when and at what
level a station transmits. Design: `docs/dev/design/ota-climb-memory.md` (the committed draft,
`d3504787`).

## Prompt

The reviewer was given the draft, `crates/openpulse-core/src/ota_rate.rs`, the #1456 NACK budget
(`crates/openpulse-daemon/src/nack_budget.rs`), `ladder-climb-into-silence.md` and
`release-1.0-criteria.md` §A2, with the replay apparatus and its outputs, and was asked to falsify:
- **Change 1:** a failed probe is withdrawn on its first failure, and the probe rung is declined
  for a doubling count of would-be climbs (4, 8, 16), expiring after 120 s of clean airtime. Does it
  forgo HARQ or transient rescues that matter? Does it interact with the budget's leak? Does it stop
  a degrading anchor from demoting? Does it remove A2's clause-2 evidence?
- **Change 2:** one SL2 decode with ≥ 60 s of airtime proves the rung. Is the 2 m evidence for it
  sound, and can it ship alone?
- The withdraw-at-the-threshold form, the HF expiry, the consumers of the two new decisions, and
  whether either change is a wire change.
- **Does this block Release 1? If not, say so.**

## Verdict

**APPROVE WITH CHANGES, for both changes.**

- **R1 (required). A `Leaked` failure with a probe outstanding keeps today's `NackStepDown`.** After
  the budget is spent, `failure_verdict` returns `Leaked` once per `LEAK_SAMPLES` (10 min) and
  `on_ladder_burst` refills the count to 3: the receiver keys exactly one NACK per 10 min, and it is
  the leaked one. The unanswered sender has meanwhile fallen back to the anchor, so the muted
  failures were at the anchor. A withdraw there would spend the only announced move per 10 min on
  nothing, and the anchor would never step down again (the f2h class). Only a keyed first failure
  at a probe withdraws; `a_leaked_nack_steps_on_its_own` stays as written.
- **R2 (required; the maintainer chose option c). Cap the expiry at 240 s.** The expiry is clean
  airtime at the anchor; below SL7, 120 s is only 2–6 frames, so the 4 → 8 → 16 doubling never
  happens there, and a dead SL3 ceiling would be re-probed every ~2 SL2 frames, about 22 % of
  airtime. The expiry doubles with the backoff level, 120 → 240 s, capped at 240 s: about 14 % tax
  on a dead ceiling and a worst-case wait of about 350 s after it recovers. Options recorded: (a)
  480 s, (b) 120 s, (c) 240 s; (c) because a 2× rate is lost for at most ~350 s, and never silence.
  The design must say that the counts apply only where a frame is short against the expiry.
- **R3 (required). Instrument the falsifier for `RX_CLIMB_BACKOFF_FIRST`.** Make "re-probe after a
  withdraw: decoded or withdrawn again" derivable from what an evidence bundle carries, with the
  memory's n and the rung; add a one-line census; state that FIRST = 1 or 2 is the fallback if HF
  falsifies 4 (the sweep at (2, 16) costs ≤ 1 %, except f2j at −7 %).
- **R4 (required; the maintainer chose option a). Induce A2's demotion on air.** With change 1, a
  stable 2 m ceiling produces no natural `NackStepDown`; f2k's ten SL10 → SL8 step-downs had
  supplied clause 2. Show clause 2 by an induced demotion (power or attenuator), report the replay's
  `NackStepDown` count for change 1 in f2j as the existence proof, and add a work-plan row.
- **R5 (required). One 2 m twin-OTA run per direction on the build with change 1, before the HF
  trip.** Needs the maintainer's go to key.
- **R6 (required).** `RateDecision::ALL` and `all_index` grow to 9; `is_failure_path` includes
  `ProbeWithdrawn`; state that the panel's `apply_event` drops an unparseable line silently
  (`connection.rs:179`), so an older panel loses only those events; do not add `#[serde(other)]`.
- **Recommended:** write the sender/receiver relation as a coupling (the sender clears
  `tx_climb_backoff` on any lower NACK, so a `ProbeWithdrawn` NACK resets the sender's doubling);
  park a wall-clock bound `max(airtime, wall time since withdraw)` as a follow-up unless it is cheap.
- **Also:** the 2 m claim that HARQ rescued nothing rests on 0 decodes, not on a log line (the
  engine logs no combined-decode line); name the cost that a lost first NACK no longer leaves the
  probe rung a candidate for the second copy; extend `never_desyncs_under_arbitrary_ack_loss` to
  withdraws.

**Key measurements from the review:**
- **Rescues forgone:** SL10 probes, 40 starts, 0 rescued. SL9 probes from SL8, 12 starts, 10
  rescued within 2 frames, but those probes are created by the SL8 detour that change 1 removes.
  The working-rung transient failure rate is 5–9 %.
- **A time-correlated (Gilbert-Elliott) channel model:** today 741–765 B/min; change 1 (withdraw at
  the first failure) 991–1159 B/min; the gain grows with fade dwell; the threshold form is worse
  everywhere.
- **Anchor degradation** (mean 14 → 10 dB over 600 s): the first `NackStepDown` comes at a median
  8.7 s today against 8.5 s with change 1; 60/60 runs demote under both.
- **Change 2's evidence, over 43 bundles:** 34 sessions decoded SL2 first; 25 had a second SL2
  decode and the first SL3 then decoded 24/25; 9 had a failure next (builds 2026-10-31..11-02);
  first-decode SNR does not separate the groups. A wrong climb costs about 39 s plus one budget
  unit, and the change saves about 140 s.
- **Not a wire change** (verified: `ack.rs` byte 3 carries an absolute level).
- **Release 1:** does not block; safe for the HF confirmation after R1, R2 and R5.

All required changes are applied to the design note, each marked R1–R6; the implementation follows
on the same branch (`feat/climb-memory`).

## Consumer

`ModemEngine::ota_decode_and_ack_inner` (`crates/openpulse-modem/src/engine.rs`, the decision and
its `EngineEvent::OtaRateDecision`), the only production feed of the receiver's controller; the
link simulator (`apps/openpulse-linksim/src/lib.rs`) is the other caller. Found with
`grep -rn "on_rx_frame_with\|on_rx_frame(" --include=*.rs crates apps plugins`, which also matched the
known call sites in `crates/openpulse-core/tests/success_based_climb.rs`. Decision consumers:
`grep -Rln "OtaRateDecision\|ota_rate_decision" --include=*.rs --include=*.py --include=*.sh .`
(the panel, the TUI, `ladder_evidence_probe.rs`, the daemon twin tests, the on-air runner's
comment).

## Prior art

`docs/dev/design/ladder-climb-into-silence.md`, review S2 (the sender's `tx_climb_backoff`, of which
this is the receiver's mirror); xara's growing climb margin (`docs/dev/research/xara-comparison.md`
§4); `docs/dev/design/failed-burst-ownership.md` (#1460 round 4b: a muted failure moves no ladder);
the #1456 NACK budget. Found with `grep -Rli "climb" docs/dev/design docs/dev/reviews`.

## Twins

The sender's `tx_climb_backoff` (left unchanged; its missing time bound is noted in the design) and
the link simulator, which drives the same controller and so gets change 1 with no edit, and change 2
only if it passes an airtime (it does not). ARDOP and KISS do not run the OTA ladder. Found with
`grep -rn "OtaRateController::new" --include=*.rs crates apps plugins`: outside tests the only
constructors are in `engine.rs` and `apps/openpulse-linksim/src/lib.rs`.
