---
project: openpulsehf
doc: docs/dev/reviews/artifacts/1349-frontmatter-enforcement.md
status: review
last_updated: 2026-09-13
---

# Review — where the doc-frontmatter check's enforcement should live (#1349)

Design review before implementing. My recommendation was **partly falsified**, and one premise of
mine was simply false on the record.

## Prompt

Sent to Fable as a falsification request. Given: the frontmatter checker is grandfathered against
`docs/.frontmatter-baseline.txt` and fails only on new offenders; it runs in **no** workflow, script
or hook; its only CI host `docs.yml` is `disabled_manually`; 40 new offenders accumulated in four
weeks and #1350/#1355 have just cleared them, leaving the ratchet clean **and** unenforced.

Three options as filed — re-enable `docs.yml`, add a step to `traceability.yml`, or add a `run_step`
to `gate.sh` — with the issue noting none was picked because "where enforcement lives is a decision".

**My recommendation, which I asked to have attacked: 2 and 3, not 1**, following the re-homed-docs
precedent (#1345/#1356), and against option 1 because "I do not know what else `docs.yml` runs" and
"the maintainer disabled it deliberately (#1120)".

Asked specifically: (a) read `docs.yml` and say whether my "restores whatever else it gates"
objection is real or lazy; (b) is `traceability.yml` the right home or am I making it a junk drawer,
and at what point is a separate workflow correct; (c) the auto-stamper is the other half and I have
no plan for it — would enforcing the validator without a stamper make the check hostile, and what
does `docfront.py` actually require of `last_updated`; (d) is enforcement even the right fix, or is
the 71-line baseline the problem; (e) is "40 in four weeks" a fair rate to project, given 37 of the
39 were review records.

## Verdict

**Take option 1 + option 3, not 2 + 3.** Both premises behind my objection to option 1 are wrong:

- **`docs.yml` runs exactly two jobs** plus an aggregate — the frontmatter check and
  `check-version-bump-docs`. The benchmark jobs #1129 complained about were split out in #1134
  (`40446f7b`). The second job is **REQ-DOC-01**, and its script is called from no other workflow,
  script or hook. So the "whatever else" is a ratified requirement that
  `release-1.0-criteria.md` calls "enforced" — the objection is real in fact and lazy in conclusion.
- **"The maintainer disabled it deliberately in #1120" is false.** Both docs workflows flipped to
  `disabled_manually` on **2026-06-24**; #1120 merged **2026-08-09** and its body *cites* `docs.yml`
  as live coverage. The decision has been escalated twice (#1129, #1134) and never made, which makes
  it the maintainer's rather than mine. Cost is not an argument either: the repo is public, so
  Actions minutes are free, and both jobs are shell over the tree (~30 s).

**`traceability.yml` is the wrong home for this check**, and the reason is structural rather than
aesthetic: it is a single job with sequential steps and **no `if: always()`**, so a fifth step would
sit behind two trailer lints and the review lint — all of which fail routinely on body edits — and a
PR failing the review trailer would get no frontmatter verdict at all until the trailer was fixed.
The clean dividing line is what each check reads: everything in that workflow reads the **PR event**
(body, title, or a diff against the base ref — the #1219 discipline the file spends twenty lines on),
while `docfront.py` reads the **tree** and needs neither.

**On the stamper (c):** `docfront.py` checks `last_updated` against `^\d{4}-\d{2}-\d{2}$` — format
only, never currency — so enforcing the validator without a stamper is **not** hostile; a stale
well-formed date passes. And the stamper is not merely disabled: `5c93ca29` (the maintainer) removed
its `pull_request` trigger and its commit-and-push body, retiring a bot-pushes-to-your-branch
pattern. Measured: of 143 docs with a well-formed stamp, **92** have their last commit more than a
week after it. The field was decorative.

**On the baseline (d):** 71 lines are 46 files, and 40 of the 71 are a `sed` (16 `project`, 12 `doc`
path, 12 `last_updated` format). A "touched doc must comply" rule would not drain it — since
2026-08-15 only 6 grandfathered files were touched at all, one of them (`traceability.md`) 86 times,
so the rule would block essentially every PR and then never fire again.

**On the rate (e):** a burst from one generator, not a trend. All 39 files fixed in #1350 were
created in 19 days; 37 are review artifacts, and `check-review.sh` verifies their `## Prompt`/
`## Verdict` structure while never looking at frontmatter. Since #1350 merged, 7 docs were added and
all comply.

## What was implemented, and what was escalated

Mine: the `gate.sh` step; the `origin/$BASE_REF` fix to `docs.yml`'s stale `base.sha`; the three
false "enforced"/"in CI" claims; and — after the maintainer's decision — the `last_updated` diff
ratchet. Escalated and then decided by the maintainer on the issue: **re-enable `docs.yml`** (done,
`active` again) and **make `last_updated` a diff ratchet** rather than format-only or dropped.

## Consumer

Who runs these checks, by path — `grep -rn "validate-doc-frontmatter\|check-doc-stamps" --include='*.yml' --include='*.sh' .`

- **`scripts/gate.sh`** — the frontmatter check, full-mode lint block, next to the trailer lint.
- **`.github/workflows/docs.yml`** — the frontmatter check and `check-version-bump-docs` (REQ-DOC-01),
  on every PR now that the workflow is enabled.
- **`.github/workflows/traceability.yml`** — the new `last_updated` ratchet, with `if: always()`.
- Nothing else calls either script; the pre-push hook is deliberately left alone, so a WIP push does
  not have to satisfy a docs-shape check.

## Prior art

- `docs/.frontmatter-baseline.txt` is the existing grandfathering ratchet; the stamp check reuses its
  exemption philosophy rather than inventing one — a doc with no stamp is exempt from the stamp rule.
- `scripts/check-rehomed-docs.sh` established the fail-closed base wrapper (exit 2 on an unresolvable
  base, no `|| echo HEAD` fallback); `check-doc-stamps.sh` copies it.
- The retired `docs-last-updated-pr.yml` is what the ratchet replaces: same effect, no bot.

## Twins

- **Same shape, different subject:** `check-version-bump-docs.sh` is the other script that runs only
  in `docs.yml`. Re-enabling the workflow restores it, which is the main argument for option 1.
- **Same trap, avoided:** `traceability.yml`'s sequential steps. The ratchet lands there because it is
  diff-based, and it carries `if: always()` so it does not inherit the failure-ordering problem that
  disqualified the workflow for the frontmatter check.
- **Not a twin:** the pre-push hook. It shares the base-ref discipline but is a latency budget, not a
  correctness surface (#1357 covers its stale-upstream ranges separately).
