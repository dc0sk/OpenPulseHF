---
project: openpulsehf
doc: docs/dev/adversarial-review.md
status: resolved
last_updated: 2026-09-30
---

# Adversarial review — the 2026-08-02 rule and its reasons

> Moved verbatim from `CLAUDE.md` on 2026-09-30. **The mandatory scope below is superseded** by work plan decision 8 (2026-09-30); the current rule is in `CLAUDE.md` → *Adversarial review*. The routing list, the three design fields and the reasons behind them still apply.

A second model (**Fable**) reviews work in this repo before it is trusted. This exists because the
expensive failures here are not bad code — they are **confident wrong beliefs** that survive long
enough to get built on. Every item below is a real occurrence, not a precaution.

**Mandatory scope (set 2026-08-02 by the maintainer; in force until the maintainer says otherwise).**
Two classes of work go to Fable *before* they land, with no judgement call about whether they are
"big enough":

- **Every design or architecture decision — reviewed BEFORE implementing.** Not after a prototype
  exists, not alongside the first commit: before the code is written. This includes wire-format and
  trait changes, new modules or crates, where a transform is seamed, what a state machine owns, and
  any choice between two viable approaches.
- **Every conclusion drawn from a test, feasibility check, prototype, or work result — reviewed
  BEFORE it becomes part of the project.** "Part of the project" means: written into `CLAUDE.md`,
  `docs/`, `traceability.md`, an issue or PR body, a commit message, or used as the premise of the
  next piece of work. Send the *apparatus* with the conclusion, and send it whether the result looks
  bad, good, or unsurprising.
- **The write-up itself, not only the conclusions behind it** (added 2026-08-02 after I posted a
  reviewed set of findings in unreviewed prose). Reviewing the finding and then writing it up
  unsupervised leaves the two failure modes review exists to catch: a hedge that quietly hardens
  into a claim, and an emphasis that makes a secondary result read as the headline. Send the actual
  text that will be posted or committed — not a summary of it — even when every underlying
  conclusion has already been cleared.

The costs of skipping are asymmetric and already paid here: a wrong elimination closes a door
silently (the 2026-07-30 settle-recovery case in item 5 below), an unreviewed constant ships fitted
to an inventory nobody widened (#1053), and a conclusion that reaches `CLAUDE.md` is quoted back as
fact for months.

**Route to Fable:**

1. **New hypotheses**, for plausibility — before building the fix a diagnosis implies.
2. **New insights**, for correctness — especially claims about what the code or a record *is*
   (a review found "#1020" was a merged PR, not an open issue, after it had been cited as one).
3. **Prototypes and their results**, for validation — the result *and* the apparatus that produced it.
4. **Areas of trouble, against the reference projects** — `Rhizomatica/mercury`, `RFnexus/modem73`
   and the rest of `docs/dev/research/references.md`. Findings update that doc (it has a *Recurring
   lesson* section for exactly this); do not start a parallel artifact.

**And four more, each earned:**

5. **Eliminations, not just hypotheses.** A negative result gets written into issues and into this
   file as "do not re-attempt", so a wrong one closes a door permanently and silently. 2026-07-30:
   "forcing the retry live refutes the recovery direction" was recorded as an elimination when it had
   only refuted *recovery through an unchanged gate* — and the fix that shipped was a recovery that
   changes the gate. A wrong elimination costs more than a wrong hypothesis.
6. **The harness, not just the conclusion — including when the result looks GOOD.** Three instruments
   lied in one session, all self-built: a squaring carrier estimator that locked to the wrong line of
   a 250 Hz comb, an SDR saturating at RFGR 12 into a smear that mimicked a modulation defect, and
   `route_with_capture_agc` *discarding* the idle it primed with (making an "AGC regime" measurement
   a buffer-is-the-frame fixture). Surprising results get the apparatus reviewed, not just the number.
7. **Any new constant in a DSP path**, with the question *what inventory was this fitted to, and what
   would falsify it?* `SATURATION_FLOOR_CEILING = 0.05` was fitted between the two fixture levels then
   known and falsified by the third. See the *artifact-calibrated constant* archetype.
8. **Prompt for falsification, never for agreement.** Ask it to *test* the instinct rather than
   confirm it, and to flag anything wrong or unproven in the framing. A prompt that presents a
   conclusion gets a conclusion agreed with.
9. **Three fields, before the prompt goes out — enforced by `scripts/check-review.sh`, not by
   care.** A design artifact must carry `## Consumer`, `## Prior art` and `## Twins`, each non-empty
   and each either an answer with the command that produced it or the literal word `UNCHECKED`.
   - **Consumer** — who CALLS this in production, by `file:line`. #1271 proposed answering a query
     from `GetConfig`, which runs on a task holding no engine; the design would have paid none of
     the debt it claimed, and the consumer was never read.
   - **Prior art** — the sweep for an existing mechanism, with its hits. #1268 proposed building a
     ratchet that already existed (`NOT-GRANDFATHERED` in `trace.py`). One grep.
   - **Twins** — the sibling paths sharing the shape. #1252 pinned the responder and left the
     initiator open; #1177 and #1249 each needed a second arm.

   `UNCHECKED` is legal on purpose. The goal is to turn an omission into a written claim, exactly as
   `Review: none` does at tier 1 — a field that may not be empty but may say `UNCHECKED` bans a
   construct, where "consider the consumer" would be an exhortation that cannot fail. **Added
   2026-09-06 after a lessons review found that every overturned proposal in a ten-PR session was
   missing exactly one of these, each one command away — the reviewer was doing the proposer's
   falsification.** The same review falsified the tempting alternative: the rules were already
   written in `measurement-integrity` and `second-opinion` (one added the same day it was violated),
   and the transcript showed 92 `GATE:` lines with zero loads of the skill holding them. Adding text
   was the one intervention the evidence ruled out.

**What this does NOT replace.** Review is not the workspace gate and cannot be treated as one. The
same session's review approved a design whose three regressions were caught only by
`cargo test --workspace` — a fixture gated out at a level no reviewer had reason to consider. Run the
full gate at the end regardless of how the review went; the evidence tiers are independent, exactly
as simulation and hardware are.

**What is still out of scope.** Mechanical work that decides nothing and concludes nothing: applying
a review's own verdict, renames and formatting, a fix whose shape the maintainer already specified,
running an existing gate and reporting its output verbatim. The line is *decision or conclusion*, not
size — a one-line change that picks between two designs is in scope; a 500-line mechanical refactor
is not. When it is unclear which side something falls on, send it.
