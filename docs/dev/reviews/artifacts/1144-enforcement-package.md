---
project: openpulsehf
doc: docs/dev/reviews/artifacts/1144-enforcement-package.md
status: review
last_updated: 2026-09-16
---

# #1144 Part 3 — the enforcement package

## Prompt

Sent to Fable 5.1 as an adversarial design review, with the packet stating up front that the code was
already written (branch `ci/1144-enforcement-package`, `7c0bec73`) and that this inverted the standing
rule. Seven sub-decisions were named and the reviewer was asked to falsify each, plus three
assumptions I had made without checking documentation. Command:
`git show 7c0bec73` plus read-only inspection of the workflows, the hook, `CLAUDE.md`, `gate.sh` and
the GitHub API.

## Verdict

**Accepted with five corrections, one of them a defect in the records rather than the code.** The
maintainer's decision (apply all four of #1144 Part 3) was not under review; the sub-decisions were.

**The headline finding — records written ahead of the configuration.** The commit message,
`CLAUDE.md`, the ledger entry and `release-1.0-criteria.md` all stated the ruleset change as applied
while `gh api repos/dc0sk/OpenPulseHF/rules/branches/main` still returned `[]`. This is the #1120
archetype committed inside the sweep whose purpose is to prevent it, and it voided the commit's own
claim to be "the first merge to run under them". Fixed by applying the configuration and quoting the
**readback** — not the PUT's exit status — beside every claim. A ruleset lives outside git and can be
silently undone; this one sat `active` with an empty `include` for five months.

**D1 — `cancel-in-progress` — REFUTED.** The dichotomy that justified `true` ("cancel, or queue every
merge") is false: a concurrency group holds at most one running and one pending run, and a newer push
replaces the pending one. Measured cadence (60 merges, 2026-09-05..09-16: median gap 156 min, 11/59
under an hour) against gate runtime (44 min on `ubuntu-latest` for the one real runner execution on
record; 76–79 min locally) means `true` would discard a near-complete run on roughly a fifth to a
third of merges. Changed to `false`.

**D2 — issue dedup — ACCEPTED with two fixes.** Eventual consistency of issue search yields at most a
duplicate, which is cheaper than engineering around. But the search had to be scoped with
`--author app/github-actions`, or any user could open an issue with that title and capture every
future failure report. More importantly, the body pointed at an Actions artifact for the failure
list; artifacts expire at 90 days and issues do not, so the `GATE:` line and the untruncated failure
block now go **into the issue body**.

**D3 — which checks to require — CONFIRMED, and the reasoning was incomplete in my favour.** GitHub
documents that a job skipped by an `if:` reports success and does not block, so requiring
`pr-hook-long-runner` would be vacuous on code PRs. But `ci.yml` also has a workflow-level
`paths-ignore`, and a workflow skipped by *path filtering* leaves its check **Pending and blocking** —
so requiring it would permanently block docs-only PRs. Two failure directions, both arguing for
exclusion. Also added: `strict_required_status_checks_policy: false`, since strict would impose a
rebase-and-rerun on every PR at this merge rate.

**D4 — dev-dependencies in the reverse-dependent list — CONFIRMED in principle, presentation
refuted.** Keeping dev-deps is right (the 3-crate delta for `openpulse-core` is exactly the crates
whose *tests* would break). But 31 of 40 crates is "the workspace", and a wall of names trains people
to ignore the hook. Now prints a count plus a copy-pasteable `cargo test -p …` line, and says "that
is effectively the whole workspace: run scripts/gate.sh" once the closure passes half the members.

**D5 — transitive closure — CONFIRMED, kept.** Direct-only would name 10 of the 23 crates
`openpulse-dsp` can reach; the two-hop case (dsp → plugin → modem → daemon) is the common one and the
computation is free.

**D6 — one message for several states — CONFIRMED as a real defect I introduced.** The first draft
printed "none found, or cargo metadata failed" for three distinct situations, and discarded the only
diagnostic with `2>/dev/null`. That is the construct this repo bans: silence is not a verdict, and a
zero from a filter is a claim about the filter. Each state now gets its own sentence from its own
exit status, with python exiting 2 on a parse failure so it can never be read as "no results".

**D7 — no `paths-ignore` on the post-merge gate — CONFIRMED, for a stronger reason than I gave.**
`ci.yml`'s justification ("no crate pulls a `.md` into the build") does not transfer, because
`gate.sh` is not only compile+test: doc frontmatter, ledger ordering, the review-trailer lint and the
re-homed-docs lint are all steps a docs-only merge can turn red.

**The three assumptions.** (a) REFUTED as stated — a `pull_request` workflow that does not run at all
leaves its check Pending and blocking; only an `if:`-skipped job reads as success. The conclusion
survives either way. (b) Partly REFUTED — three of the four ruleset rules were already enforced by
classic branch protection, so the blast radius is far smaller than claimed; what is genuinely new is
the required checks plus `require_extra_approval_for_unattributed_changes`, which affects
Copilot-opened PRs only. (c) UNPROVEN with the documented failure direction being the bad one —
`code_quality` covers seven languages, none of them Rust, blocks when analysis "fails for any
reason", and code scanning is `not-configured` here. **The rule was deleted rather than activated.**

**Sweep.** The mandated `git grep -ln 'gate.sh' -- ':!scripts' ':!target'` excluded every script's
own comments, so it was structurally unable to see `scripts/check-review.sh`'s header — which
justifies itself by when the gate runs and went stale under this change. The exclusion is narrowed to
`':!scripts/gate.sh'` in `CLAUDE.md`, and the stale comment is fixed. My reported count of 21 files
was also not reproducible (24 at `7c0bec73^`).

## What the review checked that did NOT fail

PyYAML is present on the runner image (the `trace check` step entered `gate.sh` before the successful
2026-08-13 runner execution). `__pycache__/` is gitignored, so a first python import cannot trip
`GATE: INVALID` on a fresh clone. And `ci.yml`'s header claim that the pinned `dtolnay` SHA ignores a
`toolchain:` input is itself false — that SHA's `action.yml` declares the input — which means
`REQUIRED_RUST` *is* honoured by the new job. A record error in a header, not a behaviour problem;
left for a separate change.

## Consumer

The post-merge gate's consumer is the maintainer, via an issue opened on failure
(`.github/workflows/post-merge-gate.yml`, the "Open or update an issue on failure" step). The
required status checks are consumed by GitHub's merge box on every PR against `main`. The hook's
reverse-dependent message is consumed by whoever is pushing, at
`.cargo-husky/hooks/pre-push` after the touched-crate test run.

Command: `gh api repos/dc0sk/OpenPulseHF/rules/branches/main --jq '.[] | .type'` →
`deletion`, `non_fast_forward`, `pull_request`, `required_status_checks`.

## Prior art

`.github/workflows/ci.yml`'s `pr-hook-long-runner` is the existing gate job and the reason a second
one was needed (it is `release/**`-scoped by #1120). `scripts/gate.sh` is the single verdict path and
was reused rather than re-implemented — the open-coded `cargo test` without `--no-fail-fast` that job
used to carry is the defect CLAUDE.md verification rule 2 names. Classic branch protection on `main`
already existed and already enforced three of the ruleset's four rules; that was discovered by the
review, not by the proposal.

Command: `git grep -ln 'gate\.sh' -- ':!scripts/gate.sh' ':!target'` → 29 files.

## Twins

`.github/workflows/traceability.yml` and `benchmark.yml` are the sibling per-PR checks, and both were
read to confirm neither carries a `paths-ignore` before being made required — the same skip-vacuity
that disqualifies `pr-hook-long-runner`. `scripts/check-review.sh`'s header is the twin record that
describes when the gate runs and that the old sweep could not see. The installed
`.git/hooks/pre-push` is the twin of the source hook and does **not** update on `cargo build` — only
a `cargo test` re-runs cargo-husky's build script, so the new block runs for nobody until then.
