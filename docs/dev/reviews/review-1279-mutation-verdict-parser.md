---
project: openpulsehf
doc: docs/dev/reviews/review-1279-mutation-verdict-parser.md
status: review
last_updated: 2026-09-18
---

# Review — `req-mutation.sh` could not emit a true verdict, and #1279's workflow half needs re-sizing

## Consumer

`scripts/req-mutation.sh` has exactly two production callers, and one of them does not exist yet:

- `.github/workflows/mutation.yml:85` — `./scripts/req-mutation.sh --all-enforced`, on the unmerged
  branch `ci/1279-scheduled-mutation` (commit `648dfdb8`, no PR opened). This is the only scheduled
  consumer, and it is the artifact under review.
- No other caller. `git grep -n req-mutation -- ':!docs' ':!target'` returns the script itself, that
  workflow line, and the header of `scripts/gate.sh` naming it as deliberately NOT in the fast gate.

So the script's verdict is consumed by nothing today; every claim made about its behaviour to date
was made about an instrument that had never been run to completion against the real tool.

## Prior art

- `git grep -n 'mutants' .github/workflows/` -> only `mutation.yml` on the branch. Nothing else
  schedules mutation; the `coverage.yml` cadence comment is the nearest analogue and it covers
  line coverage, not mutation.
- `grep -n pyyaml .github/workflows/*.yml` -> `traceability.yml:38-39` installs it; `mutation.yml`
  does not. That asymmetry is finding 1 below.
- `cargo mutants --help | grep -n 'CARGO_TEST_ARGS'` -> "Pass remaining arguments to cargo test
  after all options and after `--`", which reads as though cargo-mutants inserts the `--` itself.
  It does not — see the apparatus. The prose is what the original code was written against.

## Twins

- `scripts/trace.sh`, `scripts/gate.sh`, `scripts/check-review.sh`, `scripts/check-rehomed-docs.sh`
  all produce a single machine-greppable verdict line. Checked: each derives its counts from its own
  computation or from a file it wrote, not by scraping a third-party tool's stdout. `req-mutation.sh`
  was the only one parsing another program's console output, which is the shape that broke.
- The sibling risk inside this script: `python3 scripts/lib/trace.py scope "$rid"` at line 107 is
  read through `while … done < <(…)`, which discards the producer's status exactly as the `mapfile`
  did. A `trace.py` traceback yields empty `files`/`tests` — but that path already fails closed
  (`no capability code to mutate` / `MISSING-BINDING`, both `rc=1`), so it is reported, not silent.
  Flagged rather than changed; confirm that reading is right.

## Prompt

My instinct is that `scripts/req-mutation.sh` was structurally incapable of printing a true
`REQ-MUTATION: PASS`, and that #1279's workflow cannot be merged as written. Test that instinct
rather than confirming it, and flag anything wrong or unproven in the framing — particularly the
sizing, where I have already been wrong once by a factor of six.

### Finding 1 — the only reachable PASS was the one that did no work

`mapfile -t targets < <(python3 …)` discards the producer's exit status. Measured:

```
$ bash -c 'set -u; mapfile -t t < <(python3 -c "import sys; sys.exit(1)"); echo "rc=$?  count=${#t[@]}"'
mapfile rc=0  count=0
```

An empty `targets` array means the `for` body never runs, `rc` stays 0, and the script prints
`REQ-MUTATION: PASS`. `mutation.yml` never installs PyYAML, so on the runner the enumerator would
have raised `ModuleNotFoundError` and the nightly job would have gone green having enumerated
nothing. Controlled before/after, environment stated (`cargo-mutants -> ~/.cargo/bin/cargo-mutants`,
`python3 -> a stub that exits 1`):

```
PRE-FIX  --all-enforced -> rc=0   ModuleNotFoundError: No module named yaml
                                  REQ-MUTATION: PASS
POST-FIX --all-enforced -> rc=2   req-mutation: enumerating enforced requirements FAILED (exit 1)
                                  REQ-MUTATION: FAIL
```

### Finding 2 — bound tests were passed as multiple cargo positionals (red on arrival)

`req-mutation.sh:79,87` built `-- $testfilter` from the requirement's bound test names.
cargo-mutants passes trailing args to `cargo test` **positionally**, and cargo takes exactly one.
Measured on a three-test throwaway crate (`add(a,b)`, tests `alpha_one`/`beta_two`/`gamma_three`),
reading the command cargo-mutants actually issued out of `mutants.out/log/`:

```
cargo mutants -- alpha_one beta_two
  issued: /usr/bin/cargo test --verbose --package=argprobe@0.1.0 alpha_one beta_two
  result: error: unexpected argument 'beta_two' found        -> baseline fails, rc=4

cargo mutants -- -- alpha_one beta_two
  issued: /usr/bin/cargo test --verbose --package=argprobe@0.1.0 -- alpha_one beta_two
  result: rc=0; per-mutant logs show "running 2 tests" (of the crate's 3)
```

Affected, because they bind more than one test: **REQ-RX-02** (3), **REQ-RX-03** (3),
**REQ-SEC-13** (2), **REQ-SEC-14** (3). Four DID-NOT-RUN failures on the first night — the #1074
shape (`red on arrival`) that `mutation.yml`'s own header says was avoided by landing the
preconditions first.

### Finding 3 — the outcome parser counted a line the tool never prints (mine, and the largest)

The script derived its verdict from cargo-mutants' stdout:

```
missed=$(grep -c '^MISSED' "$out"); total=$(grep -cE '^(MISSED|CAUGHT|UNVIABLE|TIMEOUT)' "$out")
killed=$(grep -c '^CAUGHT' "$out")
```

**Corrected by review — the conclusion held, the mechanism was wrong in three ways, and one of
them changes what CI would have reported.** I wrote this up as a TTY property. It is not:

1. `console.rs:80` returns early on `outcome.mutant_caught() && !options.print_caught` — there is no
   TTY test anywhere. `NEWS.md` records the default flipping in **0.2.0**, years before this script
   was written. So this is NOT version drift and pinning a version would not have fixed it: the code
   was wrong when written. Probed under a real TTY via `script -qec`: `CAUGHT=0`, identical.
2. Even `--caught` would not have matched. `console.rs:615` renders `SummaryOutcome::CaughtMutant`
   as `style("caught")` — **lowercase**, so `^CAUGHT` misses it either way.
3. On the CI runner `^MISSED` would have missed too. `mutation.yml` sets `CARGO_TERM_COLOR: always`,
   which cargo-mutants reads for `--colors`; the raw bytes are `033[31m033[1mMISSED`. So the
   dev-box behaviour below is not what the nightly would have done — **every** requirement would
   have read DID-NOT-RUN there, a fourth independent reason the job was red on arrival.

The dev-box measurement, which is what the two arms below describe:

```
filter: alpha_one beta_two   (all mutants caught)
  stdout: "Found 5 mutants to test" / "5 mutants tested in 1s: 5 caught"   <- no per-mutant lines
  script's parse:  total=0  killed=0  missed=0
  files:           caught.txt 5, missed.txt 0

filter: gamma_three          (3 mutants survive)
  stdout: three "MISSED   src/lib.rs:…" lines, then "5 mutants tested in 1s: 3 missed, 2 caught"
  script's parse:  total=3  killed=0  missed=3
  files:           caught.txt 2, missed.txt 3
```

`CAUGHT` is never printed, so `killed` was structurally 0. The two arms of the verdict therefore
covered every possible run:

- every mutant caught (the **best** outcome) -> `total=0` -> `DID-NOT-RUN`, reported as a crash;
- any mutant surviving -> `killed=0` -> `VACUOUS-BINDING`, however many were actually killed.

No real run could reach `REQ-MUTATION: PASS`. Combined with finding 1: **the script's only reachable
PASS was the one that ran nothing** — which is the exact property #1279 exists to eliminate,
reproduced one layer down, in the gate rather than in the tests it judges.

Why it survived this session's arm-testing: the arms were exercised through a scratch shim standing
in for cargo-mutants, and the shim printed `CAUGHT`/`MISSED` lines — the format the parser assumed.
The harness manufactured the output under test, so the parser's defect was invisible to every arm
that used it. I found the shim afterwards, on `PATH`, in a scratch directory I reused without
listing (it had also contaminated one control run, which I re-ran clean; the conclusion held).

Repair: counts now come from `mutants.out/{caught,missed,unviable,timeout}.txt`, which is
cargo-mutants' own record; `viable = caught + missed + timeout`; `total == 0` is DID-NOT-RUN,
`viable == 0` is a new NO-VIABLE-MUTANTS outcome (all mutants failed to compile — not a vacuous
binding, and not evidence either), `killed == 0` with `viable > 0` is VACUOUS-BINDING.

### Finding 4 — four smaller ones, each measured

- **The tool guard measures a different thing than the command it guards.** `command -v
  cargo-mutants` tests `PATH`; cargo resolves a subcommand from `$CARGO_HOME/bin` **first**. On this
  host `~/.cargo/bin` is not on `PATH`, so `command -v` reports absent (rc=1) while `cargo mutants
  --version` reports `cargo-mutants 27.1.0`. The script would print SKIPPED on a box where it works.
  Probe changed to `cargo mutants --version`.
- **`--output target` was shared across requirements**, rotating the previous run to
  `mutants.out.old`, so an `--all-enforced` run destroyed the evidence for all but its last two.
  Now `target/mutants/$rid`.
- **Scratch defaulted to the tmpfs.** Measured: `/tmp/cargo-mutants-…tmp`. `/tmp` here is a 15 GB
  tmpfs and cargo-mutants copies the workspace and builds in it. `TMPDIR` now defaults to
  `target/mutants-tmp` (gitignored, excluded from the copy). An orphaned 2.8 GB directory from an
  earlier run was found and removed.
- **Feature parity.** The script built with default features while every other gate in the repo uses
  `--no-default-features` (the daemon defaults to `gpu`). Now `--cargo-arg=--no-default-features`.

### The design question — #1279's cadence, which I have not built

My sizing in `mutation.yml:8-10` ("one requirement is ~4 minutes … 16 requirements — 1-2 hours")
is wrong. `cargo mutants --list` over the enforced set gives **6 914** mutants, not the ~1 100 the
240-minute timeout was sized against; REQ-DCD-01 alone is 1 422 and REQ-RX-02/RX-03 are 1 377 each.
Observed per-mutant cost on the REQ-SEC-13 run below is ~3 s build + ~1 s test with warm deps, and
an `engine.rs` mutant relinks far more test binaries than a `signing.rs` one does.

Three further workflow defects. Two are confirmed: the `rust-cache` step is **inert**, because
cargo-mutants builds in a fresh scratch copy and not in `target` (the tool's own `ci.md` prescribes
`--in-place` for exactly this); and the `REQ_MUTATION_REQUIRED` guard accepts only the literal `1`
(`true` silently fails open).

The third I asserted and **cannot support**. I wrote that on timeout the job is cancelled, so
`failure()` is false and no issue is filed, and cited coverage.yml as having hit this at 50 minutes.
Review checked the record: run 32615182798's `Measure` step ran 03:23→04:57 — **94 minutes under a
300-minute cap** — and ended `Process completed with exit code 1`, a real failure with job
conclusion `failure`. So the cited precedent is not an instance of the thing, and `mutation.yml`'s
comment claiming it is must go. GitHub's documentation does say a job exceeding `timeout-minutes` is
cancelled, so the concern is plausible and `if: always() && steps.mutation.outcome != 'success'` is
the right shape regardless — but it is unverified here and will be written as such. A second
cancellation path the issue step would also miss: `concurrency: cancel-in-progress` firing when a
manual dispatch lands during the nightly.

I do not think this is patchable into the existing single-job shape, and I have not written a
replacement. The options I can see, none of them measured:

1. **Shard per requirement** via `strategy.matrix` + `fail-fast: false`, with an aggregating job.
   Honest per-shard timeouts, but the longest shard still governs and REQ-DCD-01 may not fit a day.
2. **Rotate** — one requirement per night, cycling, so the whole set is covered weekly at a bounded
   nightly cost. Cheapest, but a vacuous binding can sit undetected for up to a week.
3. **Scope the mutation set** — `--in-diff` against the last run, or restrict mutation operators —
   so the nightly cost tracks churn rather than the whole capability surface.

Which of these is right is a decision, not a measurement, so it is not mine to make unreviewed.
Please test the framing, including whether the 6 914 figure is the right denominator at all
(it counts mutants, not mutants *reachable from the bound tests*, which may be far fewer and would
change the answer).

## Verdict

Reviewed 2026-09-18. **Findings 1, 2 and 4 CONFIRMED. Finding 3 CONFIRMED-WITH-CORRECTION** (see
the three mechanism corrections inline). The framing claim "the script's only reachable PASS was the
one that ran nothing" was confirmed, with the addition that on CI it would have been DID-NOT-RUN for
a fourth reason. One claim of mine was **refused**: the timeout/`failure()` precedent, corrected
above.

The review then audited the repair and found six defects in it. Four are fixed in this change:

- **A (major) — a cross-crate binding was a false VACUOUS-BINDING.** cargo-mutants runs each
  mutant's tests in the **mutated file's package alone** (`lab.rs` → `PackageSelection::Explicit`).
  REQ-CTL-01/02 have capability code in `openpulse-config`/`keystore`/`linksec` and their bound test
  in `openpulse-daemon`, so 303 mutants would have built, run **zero** tests, and all been recorded
  MISSED — roughly 20 minutes each to produce a fabricated finding. Measured directly:
  `cargo test -p openpulse-linksec -p openpulse-keystore -p openpulse-config -- noise_client_…`
  reports `running 0 tests` six times, rc=0. REQ-DCD-01, REQ-FUN-10, REQ-FUN-11 and REQ-SEC-14 are
  partially the same shape. Fixed: `trace.py scope` now emits `TESTPKG` (the join already existed at
  `_package_of`, it just was not printed) and the script passes `--test-package`. Verified by a
  bounded `--shard 1/150` probe on REQ-CTL-01, reading the package and test count out of each
  per-mutant log:

  ```
  CONTROL (no --test-package)   every mutant -> --package=openpulse-config   tests_run=0
  FIXED   (--test-package)      every mutant -> --package=openpulse-daemon   tests_run=1
  ```

  Both runs still report 3 MISSED, and that is the honest outcome: with the fix the bound Noise test
  really does execute and really does not cover `validate_owner_only`'s permission-bit check. Three
  of 303 mutants is not a verdict on REQ-CTL-01 and is not claimed as one.

- **A residual neither the review nor I had: the BASELINE ignores `--test-package`.** In both runs
  above the baseline tested `openpulse-config`, logged `running 0 tests` twice, and reported `ok` —
  so nothing in the tool validates that the filter selects a test at all. That makes
  "the tests prove nothing" and "no test ran" indistinguishable from the counts, which is the review's
  point (E) with a concrete mechanism. The script now discriminates on the PER-MUTANT logs rather
  than the baseline, and reports `FILTER-MATCHED-NOTHING` separately from `VACUOUS-BINDING`.
  Validated against real data from both regimes:

  ```
  ctl01-control  -> FILTER-MATCHED-NOTHING (no test executed)
  ctl01-probe    -> VACUOUS-BINDING (tests ran, killed nothing)
  ```
- **C — `mrc` was never consulted, so a killed run yielded a verdict from partial files.** The
  outcome files are opened at start and appended per mutant; a probe SIGTERMed 14 s in left
  `caught=5 missed=1` of 19 mutants with `end_time: null`, which the script would have scored as a
  complete PASS. Fixed: a new INCOMPLETE outcome gated on `outcomes.json`'s `end_time` plus the exit
  status (0/2/3 complete, 4 failed baseline, anything else an error).
- **D — `count_lines` could exit the script with no verdict line at all.** `grep -c .` on a
  blank-only file prints `0` **and** exits 1, so `[ -s f ] && grep -c . f || echo 0` emitted `0\n0`,
  making the arithmetic a syntax error that under `set -u` terminates bash before any
  `REQ-MUTATION:` line. Unreachable with today's tool, but silent-no-verdict is the wrong latent
  failure for a gate.
- **F — `$rid` reached `rm -rf "target/mutants/$rid"` unvalidated**, guarded only by trace.py's
  manners. Now checked against `REQ-<AREA>-<N>` first.

Plus two corrections to my own comments: the scratch copy is excluded by **top-level name**
(`copy_tree.rs`'s `is_top_level_target`), **not** because `target/` is gitignored — the debug log
records `git_ignore: false` — so my stated reason was wrong, and an override pointing anywhere else
inside the repo recurses until `File name too long (os error 36)`, measured. The script now refuses
such an override. And `--cargo-arg=--no-default-features` is replaced by the tool's native
`--no-default-features`.

**One defect is NOT fixed and is recorded as a known limitation in the script header.** (B) libtest
filters are substring matches, so REQ-SEC-13's two bound names actually selected **five** tests, and
4 of its 10 kills belong to `wire_query::tests::tampered_payload_fails_verification` rather than the
bound `signing::tests::tampered_payload_fails` — true bound kills are 6, not 10. It inflates a PASS
and cannot manufacture one from nothing, so the verdict below stands, but a broad-named sibling
could mask a vacuous binding elsewhere. `-- --exact` fixes it and needs module-qualified paths,
which `_scan_verifies` does not record. Tracked separately.

**Sizing, revised again — the review's numbers, not mine.** 6 914 is confirmed and IS the right cost
denominator (the tool has no reachability notion; every listed mutant pays a build and a test run
whether the bound test can reach it or not — CTL-01's 303 are the proof). What my figure missed is
that the **build dominates and every baseline is cold**: `touch engine.rs; cargo test --no-run -p
openpulse-modem` is **26 s** on this 16-core host (140 executables relinked) against a 2.8 s test
phase, so ≈29 s per `engine.rs` mutant. REQ-DCD-01 alone is **≈11.5 h**; DCD-01 + RX-02 + RX-03 are
**≈32 h here**, and `ubuntu-latest` is 4 vCPU. Each invocation also makes a fresh copy, so the
nightly is **16 cold builds**.

That kills all three of my options as stated: sharding per requirement leaves a shard that does not
fit a runner; rotation fails on the same three requirements; and `--in-diff` is incompatible with the
verdict logic, since a requirement whose files did not change yields `Found 0 mutants` → DID-NOT-RUN
→ FAIL every quiet night. What survives, and what I had not considered: **`--in-place`** (the single
biggest lever, and it makes `rust-cache` effective instead of inert); **`--shard k/n` within a
requirement** with an aggregating verdict, which is the tool's designed answer; **early exit on the
first kill**, because this gate is *existential* (`killed >= 1`), making the cost O(first kill) for a
real binding and full only for a vacuous one; and **`-F` function scoping**, since CAP-72 maps a
whole 1 335-mutant `engine.rs` to one DCD requirement.

The workflow is therefore still unbuilt and still a decision, not a measurement.
