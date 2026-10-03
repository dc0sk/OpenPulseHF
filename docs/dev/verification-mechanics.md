---
project: openpulsehf
doc: docs/dev/verification-mechanics.md
status: resolved
last_updated: 2026-09-30
---

# Verification mechanics — full text and history

> Moved verbatim from `CLAUDE.md` on 2026-09-30 (work plan decision 7). `CLAUDE.md` keeps rules 1–6 in short form; this file holds their full wording and the history of how the gate came to be enforced ("Known hole", #1074, #1120, #1144). Code comments citing "CLAUDE.md verification rule N" or "Known hole" mean this file.

Added 2026-08-03 after one session produced five wrong verdicts, none of them caused by bad code —
every one was a **corrupted verdict channel**. A rule like "check exit codes properly" cannot fail
and had already been written down; these ban specific greppable constructs instead, so a violation
is visible in the transcript rather than indistinguishable from compliance.

1. **Pipelines never carry verdicts.** A pass/fail claim, exit status or count may come only from
   `scripts/gate.sh`, or from the two-line form `cmd > log 2>&1; rc=$?`. Never `$?` after a
   pipeline; never `${PIPESTATUS[…]}` or `$pipestatus` (**dialect trap — the login shell here is
   zsh, where the bash form silently yields an empty string**); never from eyeballing piped output.
   Piping to `tail`/`head` to *read* is fine: **a pipe may shape what you read, never what you
   conclude.**
2. **The workspace gate is `scripts/gate.sh`.** It runs fmt + clippy + `cargo test --workspace
   --no-default-features --no-fail-fast`, captures real statuses without pipes, prints the failure
   list **untruncated**, and writes `target/gate-verdict.json`. Quote gate results only from its
   `GATE:` line. `--no-fail-fast` is not optional discipline — without it cargo stops at the first
   failing *binary* and the count is a lower bound (this repo has been bitten twice, #1052 latest).
   **`GATE: INVALID` (exit 3) is not a code failure — do not chase it.** It means the tree or HEAD
   moved while the gate ran, so the verdict is not attributable to any single state of the repo
   (#1151). Rerun on a quiet checkout; put concurrent work in a `git worktree`, never in the
   checkout being gated. Note what it does NOT cover: the guard samples at step boundaries, so a
   mutate-and-revert inside one step still hashes identically and passes unseen — **and since #1274
   it does not run the `#[ignore]`d acceptance suites** (`notch_rescues_interferer`,
   `ota_channel_adaptation`, ~83 min of a ~2 h gate between them; and since #1454 the spectral-busy
   decode counts in `spectral_busy_gathers_weak_frames`, whose counting gates DO run). `GATE: PASS`
   therefore does NOT mean REQ-QRM-01 was re-proven; `scripts/slow-tests.sh` is what proves it, and
   the gate prints a `held-out (runtime, #1274):` line so a green verdict cannot be read as covering
   them.
3. **After editing `gate.sh`, sabotage-verify it** — but know what each probe reaches, because
   this rule described behaviour the code does not have until 2026-09-01 (#1242). A gate nobody has
   watched fail is the self-consistent checker it exists to prevent, and so is a *rule* nobody has
   checked against the script.
   - `scripts/gate.sh --self-test` plants a deliberately failing test and requires a non-zero
     `cargo test` **with that test named in the output**. It calls `cargo test` directly and exits
     before any verdict is written, so it emits **no `GATE:` line and no `gate-verdict.json`** —
     the rule used to say it "requires `GATE: FAIL`", which it has never printed. It covers the
     failure-detection path ONLY.
   - `python3 scripts/lib/trace.py evidence-self-test` covers the **verdict** path — what a stored
     verdict is trusted for, and when it is refused (INVALID, truncated log, foreign toolchain).
     A change to the verdict schema is invisible to `--self-test` by construction; this is what
     catches it.
   - `python3 scripts/lib/trace.py graph-self-test` covers the dependency graph the dormancy join
     runs on (#1240).
   `scripts/trace.sh --self-test` runs the latter two plus the yaml probes — which since #1268
   include `EMPTY-CAP`: a capability that satisfies a requirement while citing no test. That one is
   worth knowing about, because `REQ-GAP` is cleared by a **non-empty `covered_by`** and nothing
   asked what the covering capability contained, so an untested capability laundered 16 requirements
   into looking covered — one of them `enforced` with a passing binding while its capability listed
   no code and no tests. The two layers could disagree about evidence with nothing noticing.
4. **A zero from a filter is a claim about the filter.** Before reporting any absence found through
   `grep`/`jq`/a log pattern — "0 occurrences", "never fires" — show the same filter matching a
   known-present instance, or write **"my filter found nothing"**, which is a different sentence
   from "there is nothing". A too-narrow trace filter nearly became a published finding.
5. **Reproduction harnesses share their inputs by reference — parameters AND sequences.** A harness
   claiming to reproduce gate X, or to stand in for something the product ships, takes it from the
   same `const`/generator/module the product uses, or **asserts equality against that generator in
   the DEFAULT test run** — not under `#[ignore]`, not in a comment. **A doc-comment fidelity claim
   is banned — a comment cannot fail**; one claiming to reproduce a QPSK500 gate while defaulting to
   QPSK1000 inverts the conclusion drawn from it.
   - **"Parameters" was too narrow, and the gap cost a wire-format argument (2026-09-07).** `f12`
     fed its correlator a hand-written alternating `+-+-` chip run under a comment calling that "the
     shipped sync word's structure". The wire carries alternating *bits*, which NRZI turns into
     `--++` — period **four**, not two; correlation between the measured template and the transmitted
     one was **0.035**, and its spectral lines sat at twice the right offset, so a *second*
     conclusion (about energy lost at a receive-filter edge) was also about a template that does not
     exist. Both reached a written argument for changing the wire format before a reviewer caught
     them. The fix is one assertion that runs by default:
     `f12_synthesised_template_matches_the_shipped_one` requires ρ > 0.999 and fails at 0.035
     against the original.
   - **Re-pointing the fixture is not enough if the REGIME is still wrong.** The corrected probe
     still ran at BPSK1000, where the now-correct template's lines fall exactly on the 1250–1750
     mask edges, so those cells measured leakage rather than the deployed mode. Ask what the fixture
     reproduces *and* what conditions it runs under — BPSK250 is the only mode publishing a template.
   - A probe that measures the wrong artifact is worse than no probe: it produces numbers that look
     like evidence, and they get quoted in issues and design decisions long before anyone re-derives
     them.
6. **Before `gh pr create`**, print `git log --oneline origin/main..HEAD` and `git diff --stat
   origin/main...HEAD`, and confirm both match the PR description. A PR labelled "docs-only" merged
   `engine.rs` because the branch was cut while standing on a code branch.

**Closed 2026-08-05 (was: "enforced by no machine").** `.git/hooks/pre-push` ran `cargo check`
only and deferred by comment to a PR CI job that was `disabled_manually` — so the comment was false
and nothing enforced the gate. It cost #1074: a constant bumped, four tests left red on `main`, plus
a fifth that had gone vacuous, unnoticed until someone read the file for an unrelated reason. What
closed it, in order:

1. **The root cause first.** Any enforcement wired up while the gate could not pass would be red on
   arrival, and a permanently-red gate teaches people to skip it. The five acquisition tests whose
   verdicts depended on machine speed (#1066 — same input, 5/5 idle, 0/5 on eight busy cores) now
   bound their search in **work** rather than elapsed time. First `GATE: PASS` on record:
   2324 passed, 0 failed, clean tree.
2. **`.cargo-husky/hooks/pre-push`** tests the crates a push touches (~1 min), not the whole gate
   (~15 min) — a hook too slow to tolerate is one people `--no-verify` past. Verified against
   #1074's actual breaking commit: it catches all four failures in 0.73 s of test time, where
   `cargo build` and `cargo clippy` both passed.
3. **`.github/workflows/ci.yml` re-enabled**, with its job calling `scripts/gate.sh` instead of
   open-coding `cargo test` **without `--no-fail-fast`** — which would have under-reported failures
   in exactly the way rule 2 warns about, while producing no `GATE:` line at all.

An **expected-failure baseline** was designed for the red tests and **rejected before implementation**:
the failure set is load-dependent, so "a listed test that starts passing is also a failure" would
flake in both directions and the file would not be portable between machines. Fixing the cause beat
cataloguing the symptom.

**Corrected 2026-08-15 — and the correction is the more useful lesson.** This section used to end
"`--no-verify` still bypasses the hook silently, so CI at the merge point — not the hook — is what
actually enforces this." That was **true when written** (#1076 put `scripts/gate.sh` on every PR)
and was falsified four days later by **#1120**, which scoped every `ci.yml` job to
`startsWith(github.head_ref, 'release/') || workflow_dispatch` — a deliberate, reasonable cost
decision that swept neither this sentence nor the hook's own success message. So the 2026-08-05
closure was not partial; a later narrowing re-opened the property without a blast-radius sweep.
That is the same archetype #1074 was: **a true statement invalidated by a later config change**,
which is why the sweep list matters more than the wording —
`git grep -ln 'gate.sh' -- ':!scripts/gate.sh' ':!target'` names every artifact that describes the
gate, and a change to *when* the gate runs must visit all of them. **Narrowed 2026-09-16 from
`':!scripts'`, which excluded every script's own COMMENTS** — `scripts/check-review.sh`'s header
justifies itself by when the gate runs, went stale under #1144's change, and the old sweep was
structurally unable to see it. Excluding `gate.sh` alone is the intent: the gate need not describe
itself. #1120 committed the shape twice in one
change: its PR body also justified the narrowing by citing **docs.yml**, which is
`disabled_manually` (#1129 caught that instance in the ci.yml comment; this one survived).

**Narrowed again 2026-09-16 by #1144's Part 3, which is a maintainer decision, not a silent drift.**
What is actually true now, and what it still costs:

- `scripts/gate.sh` runs in CI on a `release/**` head branch, on manual dispatch, and — since
  #1144 — **on a push to `main`** (`.github/workflows/post-merge-gate.yml`, `cancel-in-progress`, so
  a burst of merges gates only the tip). It still does **not** run before a merge.
- **That post-merge job is DETECTION, not prevention, and the distinction is the whole point.** Bad
  code still lands; what changed is that it is flagged within one merge instead of surviving to the
  next `release/**` PR. The job opens an issue on failure, because a default-branch failure email
  goes unread. Do not read a green `main` as "the gate passed before this merged".
- **Three status checks are now required** — traceability plus benchmark's two jobs — via the
  "protect main" ruleset, which until #1144 had `conditions.ref_name.include: []` and therefore
  targeted no refs and enforced nothing at all. Verified by readback, not by the PUT's exit code:
  `gh api repos/dc0sk/OpenPulseHF/rules/branches/main` now returns the four rules.
  `strict_required_status_checks_policy` is **false** — strict would demand an up-to-date branch
  before every merge, a rebase-and-rerun tax on every PR at this repo's merge rate.
- **`pr-hook-long-runner` is deliberately NOT required, for two reasons rather than one.** GitHub
  treats a job skipped by an `if:` as satisfying a required check, so it would be vacuously green on
  a code PR — a required check that cannot fail is the defect this file exists to ban. And `ci.yml`
  also carries a workflow-level `paths-ignore`; a workflow skipped by *path filtering* leaves its
  check **Pending and blocking**, so requiring it would permanently block every docs-only PR. The
  two cases fail in opposite directions and both argue for excluding it.
- **The ruleset's `code_quality` rule was DELETED rather than activated.** GitHub's rule-based code
  quality covers C#, Go, Java, JavaScript, Python, Ruby and TypeScript — not Rust — and the rule
  blocks when analysis "fails for any reason", with GitHub warning it "could block the merging of
  all pull requests". Code scanning is `not-configured` here, so pointing the ruleset at a real ref
  would have made a rule live that cannot pass and was never chosen as a gate.
- **Note what was already enforced, because this file previously said otherwise.** Classic branch
  protection on `main` has long carried `enforce_admins: true`, a required PR at 0 approvals, and no
  force-pushes or deletions. The claim that "no status check is required anywhere" was true; the
  claim that neither classic protection nor the ruleset existed was not.
- The residual is **not** hook-skipping, which is the exotic path. The mundane one needs no skipping
  at all: the hook tests **only the crates owning changed files**, so a behavioural change in
  `openpulse-core` that breaks an `openpulse-modem` or plugin test passes a fully compliant push
  (workspace clippy catches compile breakage, not behaviour). That is #1074's exact failure mode —
  "build and clippy passed, only a test run saw it". Since #1144 the hook **names** the reverse
  dependents it did not test, computed from `cargo metadata`; naming is all it does, because a hook
  slow enough to test them is one people `--no-verify` past.

So still run `scripts/gate.sh` yourself before merging — nothing blocks a merge on it. What is new is
that if you do not, `main` tells you within one merge rather than within one release. Tracked in #1144.
