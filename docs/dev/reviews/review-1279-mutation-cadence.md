---
project: openpulsehf
doc: docs/dev/reviews/review-1279-mutation-cadence.md
status: resolved
last_updated: 2026-09-18
---

# Design review — a scheduled requirement-scoped mutation job that fits (#1279)

Maintainer decision 2026-09-18: **early-exit + `--in-place`, nightly, all enforced requirements.**
This is the design that follows from it, for review BEFORE implementation. Nothing is built.

## Consumer

- `.github/workflows/mutation.yml` — does **not exist on `main`**. It lives only on the unmerged
  branch `ci/1279-scheduled-mutation` (`648dfdb8`), where adversarial review judged it not fit to
  merge. This design replaces it; nothing today runs `scripts/req-mutation.sh` on a schedule
  (`grep -r req-mutation .github/workflows/` → no hits; positive control: `gate.sh` matches 3).
- `scripts/req-mutation.sh` — repaired in #1398; it can now emit a true verdict. Its outcome counts
  come from cargo-mutants' own files, and it reports DID-NOT-RUN / INCOMPLETE / NO-VIABLE-MUTANTS /
  FILTER-MATCHED-NOTHING / VACUOUS-BINDING distinctly.
- `scripts/lib/trace.py scope` — supplies each requirement's files and `TESTPKG`.

## Prior art

- `review-1279-mutation-verdict-parser.md` (merged) — the script's repair, and the three cadence
  options it recorded, **all of which were refuted**: per-requirement sharding leaves a shard that
  does not fit a runner; rotation fails on the same requirements; `--in-diff` makes every quiet
  night a DID-NOT-RUN failure.
- cargo-mutants' own `ci.md` prescribes `--in-place` for CI, and `shards.md` documents
  `--shard k/n` + `--baseline=skip`.
- `coverage.yml` is the nearest scheduled-job precedent in this repo for cost framing.

## Twins

- **`gate.sh`** is the other long CI job; it runs post-merge on `main` and files an issue on failure
  (#1144). Its issue-filing step is the pattern to copy, including that the failure list lives in the
  issue BODY because artifacts expire.
- **`coverage.yml`** — the other scheduled job. Checked: its `Measure` step ran 94 min under a
  300-min cap and failed with exit 1; it is **not** an instance of a timeout-cancellation, which
  matters below.

## Prompt

Test these rather than confirm them. My sizing for this issue has already been wrong twice.

### A. The cost, re-measured after this week's curation — and my earlier figure was wrong

`cargo mutants --list` over the union of all enforced requirements' files: **3 167 mutants across 34
files.** My earlier "6 914" was a per-requirement **sum**, which double-counts: `engine.rs` (1 335
mutants) belongs to REQ-DCD-01, REQ-RX-02 and REQ-RX-03, so the current per-requirement loop mutates
it three times. The union is what a de-duplicated run would cost; the sum is what the script costs
today.

Dedup is not straightforward, and I want this attacked: the gate asks whether *this requirement's
bound tests* kill mutants in *its* files, so the same mutant must be tested against each
requirement's own test set. Is there a formulation that shares the BUILD across requirements while
keeping the test sets separate, or is the 2× overhead irreducible?

### B. Early exit — the mechanism, and the one measurement I have

The gate is **existential**: it asks `killed >= 1`. So a healthy binding does not need a full sweep.

Proposed: **sample-then-confirm.** Run a bounded random sample (n ≈ 20) of the requirement's mutants
first; if any dies, the binding is not vacuous — PASS, stop. Only if the whole sample survives do we
escalate to the full sweep, which is the case we actually want to pay for.

Why a sample rather than "stop at the first kill in listed order": `--no-shuffle` lists in file
order, so a plain first-kill scan is at the mercy of whether the bound tests happen to exercise the
top of the file. A sample is order-independent.

**The evidence I have is one requirement.** REQ-SEC-13: 26 mutants, first kill at **position 1**,
kills at positions 1,2,3,4,6,7,10,14,16,19 — i.e. p(kill) ≈ 0.43, and any sample finds one instantly.
I am generalising from n=1 and I know it. What I want tested: (1) is n=20 defensible, or should the
design measure first-kill position for 3–4 more requirements before picking n? (2) Does a surviving
sample of 20 justify the escalation, or does it mostly mean "this requirement's tests are weak but
not vacuous", in which case the full sweep buys a verdict nobody acts on? (3) Does sampling break
the DID-NOT-RUN / INCOMPLETE distinctions the repaired script depends on?

### C. `--in-place`, and the guard it needs

`--in-place` removes the per-invocation tree copy (measured previously at 787 MB / 1 895 files, ×16 a
night) and makes `rust-cache` non-inert — it is the single biggest lever and none of my three earlier
options named it.

It also **mutates the source tree directly**, which on a developer box is precisely the thing
`work-preparation` now bans: editing a checkout that a verification is running against. Proposed:
`req-mutation.sh` refuses `--in-place` unless `CI=true`, with the refusal naming the reason.

What I want tested: is `CI=true` the right gate (it is set by GitHub Actions and by most CI systems,
and is trivially spoofable locally — which may be fine, since the failure mode is self-inflicted)?
And does `--in-place` interact badly with the `TMPDIR` guard added in #1398?

### D. The workflow, and a claim I must not repeat

The branch's `mutation.yml` carries three defects beyond sizing: an inert `rust-cache` step (fixed by
C), a `REQ_MUTATION_REQUIRED` guard accepting only the literal `1`, and a comment asserting that a
timed-out job is cancelled so `failure()` is false and no issue is filed — **citing coverage.yml as
precedent**. That citation is wrong (see Twins), and I could not confirm the underlying behaviour.
The design therefore uses `if: always() && steps.mutation.outcome != 'success'`, which is correct
under either behaviour, and the prose makes no claim about cancellation. Flag it if that is still
overstated.

Also open: whether the job should shard across requirements as a matrix at all, given B may make the
total small enough for a single job; and what timeout is honest once B is measured.

### E. Should this be built now?

The honest alternative: `req-mutation.sh` works, and running it by hand before a release and when a
`// VERIFIES` binding changes costs nothing. A scheduled job earns its place only if the nightly cost
is bounded and the verdict is one someone acts on. If B's measurement says the cost is not bounded,
say so and the answer is "not yet".

## Verdict

Reviewed 2026-09-18. **E: do NOT build the scheduled job yet. B: sample-then-confirm REJECTED as
specified.** A and C and D adopted with changes, none of which matter until E flips.

**My n=1 was the top of the distribution, not its centre.** Three requirements now measured
end-to-end through the production script, outcome vectors preserved under
`~/.cache/openpulse-evidence/mutants/`:

| requirement | viable | killed | p(kill\|viable) | first kill at |
|---|---|---|---|---|
| REQ-SEC-13 | 23 | 6 *(bound kills; 10 counting a sibling's)* | 0.26 | position 1 |
| REQ-PQ-05  | 134 | 23 | 0.172 | **position 93** |
| REQ-FUN-12 | 39 | **1** | **0.026** | position 28 |

**The order-dependence I worried about is real, and it defeats the tool's default sampling.**
PQ-05's kills are all in `sar.rs` (23/52) while `pq_handshake.rs` is 0/90, and file order puts the
latter first — so `--shard`'s default *slice* mode takes 21 mutants and finds **zero**, on a healthy
binding. Round-robin finds 3. A uniformly random 20-sample survives 2.2 % of the time. There is also
no random-sample option in the tool at all: `--shuffle` happens *after* sharding and carries no seed.

**REQ-FUN-12 is the case that kills the design.** Its non-vacuity rests on a **single** mutant
(`trust.rs:255 replace == with != in evaluate_handshake`). A random 20-sample survives **57 %** of the
time, so sample-then-confirm would escalate it most nights to a full sweep ending "killed 1 of 39" —
a PASS by this gate's own rule that nobody would act on, and one refactor away from flipping.

**Interrupt-based early exit is incompatible with #1398.** A SIGTERM leaves `end_time: null`, which
the repaired script correctly reports as INCOMPLETE. Sampling must be *complete bounded runs*, never
a run stopped on first kill.

**The free finding that matters more than the workflow.** A mutant in a package outside the bound
test's dependency closure cannot be killed — the binary cannot link it. From `cargo metadata`, with
no mutants run: REQ-FUN-11 **7/408 reachable (0.02)**, REQ-CTL-05 **8/303 (0.03)**, REQ-FUN-10
205/674, REQ-CTL-01/02 246/303. For six of sixteen enforced requirements a PASS would be bounded by
SCOPE rather than by test quality. That is diagnosable for free and belongs in `trace.py` as a scope
finding (`UNREACHABLE-FROM-BINDING`), not in a nightly job.

**Other costs that foreclose the job today.** REQ-RX-02's bound test binary takes **395 s**, so each
of its mutants pays ~400 s on top of the build: a 20-mutant sample is ~2.4 h locally and the
escalation ~6.8 days. The per-requirement sum is **7 846** (not the 6 914 I cited, which predated
this week's curation) against a 3 167-mutant union — 2.48× overlap, because `engine.rs` is mutated
three times. Dedup is possible (attribute kills from module-qualified test names in the per-mutant
logs, with `--no-fail-fast`) but only pays on full sweeps, which is the thing we are not running.

**What would change the answer:** round-robin 20-shard timing and first-kill position on DCD-01,
RX-03 and CTL-01, *after* the scope is intersected with the bound test's package closure. If each
heavy requirement's sample kills within ~30 min at 4-vCPU cost, a nightly of 16 closure-filtered
samples fits — with a reportable `SAMPLE-SURVIVED n/N, no verdict` in place of escalation.

**The hand-run cadence in the issue is also wrong.** `// VERIFIES` lines changed in 9 commits over
60 days, but `requirements.yaml` — which sets the mutation *scope* — changed in **26**, against 129
merges in 30 days. So "run it when a binding changes" must mean "when `trace.py scope` output
changes", roughly 3×/week, scoped to the changed requirements and never `--all-enforced`.
