#!/usr/bin/env bash
# The workspace gate. Run this instead of composing cargo invocations by hand.
#
# WHY THIS EXISTS. Five separate times, a verdict about this repo was wrong because the CHANNEL
# carrying it was corrupted, not because the code was:
#   - `cargo test ... | tail -3` then `$?`   -> captured tail's status; a failed build reported green
#   - `${PIPESTATUS[0]}`                     -> bash-only; empty under zsh, silently
#   - `| tail -20` on a results summary      -> failures earlier in the run invisible
#   - no `--no-fail-fast`                    -> cargo stops at the first failing BINARY; the count
#                                               is a lower bound, never a total (bitten twice)
#   - "main is clean"                        -> asserted with no run behind it at all
#
# So this script bans the constructs rather than asking anyone to use them carefully:
#   * no pipelines anywhere in the verdict path — every status comes from `cmd > log 2>&1; rc=$?`,
#     the only form that is shell-dialect-free
#   * full output to a log file; the terminal gets a summary, but the FAILURE LIST IS NEVER
#     TRUNCATED, because failures are the one thing that must not be cut
#   * a machine-checkable last line (`GATE: PASS|FAIL|PARTIAL|INVALID ...`) and
#     `target/gate-verdict.json`. INVALID (exit 3) means the tree or HEAD moved during the
#     run, so the verdict is not attributable to any single state of the repo — rerun it;
#     it is NOT a code failure and must not be chased as one.
#
# SABOTAGE-VERIFY THIS SCRIPT AFTER EVERY EDIT. A gate nobody has watched fail is exactly the
# self-consistent checker it exists to prevent — this repo's archetype #1 wearing a safety vest.
#   1. plant a failing assertion in any test
#   2. run this script
#   3. require `GATE: FAIL` with that test named in the failure list
#   4. revert
# `--self-test` automates a NARROWER version of that against a scratch test file: it plants the
# fixture, runs the gate's own nextest function (`run_nextest`) directly, and requires a non-zero
# status, the planted test named in the JUnit-derived failure list, and every selected test run
# (the `--no-fail-fast` property). It does not go through run_step and exits before any verdict is
# written, so it prints no `GATE:` line and writes no gate-verdict.json — the failure-detection
# path only. CLAUDE.md rule 3 claimed
# it "requires GATE: FAIL" until #1242; it has never printed one. A change to the VERDICT path is
# therefore invisible to it: `python3 scripts/lib/trace.py evidence-self-test` is what covers that.
#
# Usage:
#   scripts/gate.sh            # fmt + clippy + full workspace test (cargo-nextest + doctests)
#   scripts/gate.sh --quick    # fmt + clippy only (NOT a gate; prints GATE: PARTIAL, no token)
#   scripts/gate.sh --self-test

set -u

REPO_ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$REPO_ROOT" || exit 2

STAMP=$(date -u +%Y%m%dT%H%M%SZ)
LOG_DIR="$REPO_ROOT/target"
LOG="$LOG_DIR/gate-$STAMP.log"
VERDICT="$LOG_DIR/gate-verdict.json"
mkdir -p "$LOG_DIR"

MODE="full"
case "${1:-}" in
    --quick) MODE="quick" ;;
    --self-test) MODE="self-test" ;;
    # Prints the drift fingerprint and exits. Exists so the primitive can be tested in seconds
    # instead of only through a 2 h run — the properties everything else rests on (content edit
    # changes it, mtime alone does not, writes under target/ do not, an untracked file's CONTENT
    # does) are each one command with this.
    --fingerprint) MODE="fingerprint" ;;
    "") ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
esac

# Queue behind any other heavy build on this machine, in any project, before
# anything is timed or snapshotted — so time spent waiting is not part of the run,
# and START_HEAD below is taken after the wait. `--fingerprint` is exempt: it is
# the seconds-long primitive and builds nothing.
if [ "$MODE" != "fingerprint" ]; then
    # shellcheck source=scripts/lib/host-build-lock.sh
    source "$REPO_ROOT/scripts/lib/host-build-lock.sh" "gate"
fi

COMMIT=$(git rev-parse HEAD 2>/dev/null || echo "unknown")
if [ -n "$(git status --porcelain 2>/dev/null)" ]; then DIRTY="dirty"; else DIRTY="clean"; fi

# Run one command, capture its REAL status. No pipes: `$?` after a pipeline is the last element's
# status, and ${PIPESTATUS}/$pipestatus differ between bash and zsh. This form works everywhere.
# Wall time per step, mm:ss, so the slow steps are known rather than guessed (the gate takes ~2 h).
fmt_secs() { printf '%dm%02ds' $(( $1 / 60 )) $(( $1 % 60 )); }

run_step() {
    step_name=$1; shift
    printf '  %-38s' "$step_name"
    echo "=== $step_name: $* ===" >> "$LOG"
    step_t0=$(date +%s)
    "$@" >> "$LOG" 2>&1
    rc=$?
    step_secs=$(( $(date +%s) - step_t0 ))
    # Completion marker (#1224). Without it a truncated log is indistinguishable from a finished one
    # — a completed gate log simply ends with the last step's output — so `trace.py` read a
    # half-written log as evidence and reported CITED-BUT-DIDN'T-RUN for tests that had not been
    # reached yet. Written REGARDLESS of $rc: a test run with failures is still a completed run and
    # is valid evidence about which tests passed; only truncation invalidates it.
    echo "=== end $step_name: exit $rc ===" >> "$LOG"
    # A separate line: trace.py matches only `=== end <step>: exit N ===`, which stays verbatim.
    echo "=== time $step_name: ${step_secs}s ===" >> "$LOG"
    if [ "$rc" -eq 0 ]; then printf 'ok%-18s%s\n' "" "$(fmt_secs "$step_secs")"
    else printf 'FAILED (exit %s)%-5s%s\n' "$rc" "" "$(fmt_secs "$step_secs")"; fi
    return $rc
}

# The installed pre-push hook vs the versioned one (#1448). cargo-husky 1.5.0 copies
# `.cargo-husky/hooks/pre-push` into the hooks dir only when no hook from the same cargo-husky
# version is there yet, so once installed, a change to the versioned hook never reaches the hook git
# runs — and nothing noticed: a maintainer host ran a hook missing three later passes. cargo-husky
# inserts two header lines after the shebang (`#` and `# This hook was set by cargo-husky …`);
# `normalise_hook` strips exactly that pair, so a header-bearing install and a plain `cp` both
# compare equal to the source. A missing hook (fresh clone, CI before any build) is not drift.
normalise_hook() { sed -e '2{/^#$/{N;/set by cargo-husky/d;};}' "$1"; }
hook_drift() {
    versioned=".cargo-husky/hooks/pre-push"
    installed="$(git rev-parse --git-path hooks/pre-push)"
    if [ ! -f "$installed" ]; then
        echo "no installed pre-push hook at $installed — nothing to compare"
        return 0
    fi
    if cmp -s <(normalise_hook "$installed") <(normalise_hook "$versioned"); then
        echo "installed pre-push hook matches $versioned"
        return 0
    fi
    echo "installed pre-push hook at $installed differs from $versioned:"
    diff <(normalise_hook "$versioned") <(normalise_hook "$installed") | head -40
    return 1
}

# ------------------------------------------------------------------ the workspace test step
#
# cargo-nextest, not `cargo test` (2026-11-08). `cargo test --workspace` ran the ~400 test
# binaries one after another: their run times summed to 43.3 min of a 48 min gate. nextest runs
# binaries and tests in parallel, one process per test, bounded by `.config/nextest.toml` (read its
# header before touching the thread count or the test groups). What the switch must NOT change is
# the contract, so each part of it is carried explicitly:
#   * coverage — nextest does not run doctests, so `cargo test --doc` is its own step; and the
#     "nextest/cargo test parity" step asks every test binary what `cargo test` would run and
#     fails on any test nextest would not (scripts/lib/nextest_parity.py);
#   * `--no-fail-fast` — on the command line AND `fail-fast = false` in the gate profile;
#   * the counts — from nextest's JUnit report, re-emitted as libtest-shaped lines by
#     scripts/lib/nextest_junit.py, never scraped from nextest's terminal output (a failing test's
#     replayed stdout contains libtest's own `test result:` lines, which would be counted);
#   * trace.py's evidence channel — reads those same libtest-shaped `test <path> ... ok` lines.
NEXTEST_PROFILE="gate"
# The two steps whose `test result:` lines are counted. Named ONCE and used by reference by both
# run_step and the counter: a hand-typed copy would let a rename count nothing and pass on a zero
# (review 2026-11-08, finding 2 — the shape trace.py's marker matcher already refuses).
STEP_REPORT="nextest JUnit report"
STEP_DOCTEST="cargo test --doc (workspace)"
# The version the gate was validated with (and that CI installs). Recorded in gate-verdict.json.
NEXTEST_VERSION="0.9.143"
NEXTEST_CMD="cargo nextest run --workspace --no-default-features --no-fail-fast --profile $NEXTEST_PROFILE"
DOCTEST_CMD="cargo test --workspace --no-default-features --doc --no-fail-fast"

# Where nextest writes the JUnit report: `<store dir>/<profile>/junit.xml`. The store dir is pinned
# in .config/nextest.toml (`[store] dir`) and nextest resolves it against the WORKSPACE ROOT, not
# CARGO_TARGET_DIR — measured 2026-11-08: with the build in ~/cargo-targets, the report still
# landed in ./target/nextest/gate/. Kept a function so the self-test and the gate share it.
junit_path() {
    echo "$REPO_ROOT/target/nextest/$NEXTEST_PROFILE/junit.xml"
}

# The report step: the JUnit report rendered for the counters and trace.py, and required to hold
# exactly as many tests as nextest selects — the gate's form of the self-test's property 3.
report_step() {
    _want=$(python3 scripts/lib/nextest_parity.py nextest-count) || {
        echo "cannot count nextest's selection"; return 2; }
    python3 scripts/lib/nextest_junit.py report "$JUNIT" "$_want"
}

# Delete the previous report FIRST: a run that dies before writing one must not be judged from
# the last run's file. A missing report is then a hard failure of the report step.
run_nextest() {
    rm -f "$JUNIT"
    $NEXTEST_CMD
}

self_test() {
    scratch="crates/openpulse-core/tests/gate_self_test_sabotage.rs"
    # Remove the fixture even if the run is interrupted. An abandoned sabotage file was left in the
    # tree once by a Ctrl-C'd self-test, and a deliberately-failing test sitting in a source dir is
    # exactly the thing nobody expects to find there.
    trap 'rm -f "$scratch"' EXIT INT TERM
    echo "SELF-TEST: planting a deliberately failing test at $scratch"
    cat > "$scratch" <<'RS'
// Temporary sabotage fixture written by scripts/gate.sh --self-test. Safe to delete.
#[test]
fn gate_self_test_must_fail() {
    assert_eq!(1, 2, "deliberate failure: if the gate reports PASS with this present, it is broken");
}
RS
    out="$LOG_DIR/gate-selftest-$STAMP.log"
    JUNIT=$(junit_path)
    # The SAME function the gate's test step runs, so the probe cannot drift from the gate.
    run_nextest > "$out" 2>&1
    rc=$?
    want=$(python3 scripts/lib/nextest_parity.py nextest-count 2>>"$out")
    python3 scripts/lib/nextest_junit.py failures "$JUNIT" > "$out.failures" 2>&1
    frc=$?
    python3 scripts/lib/nextest_junit.py report "$JUNIT" > "$out.report" 2>&1
    rm -f "$scratch"
    got=$(awk '/^test result:/ {for(i=1;i<=NF;i++) if($i=="passed;"||$i=="failed;") s+=$(i-1)} END {print s+0}' "$out.report")
    named=0; grep -q "gate_self_test_must_fail" "$out.failures" && named=1
    nfail=$(grep -c . "$out.failures")
    # Three properties, each of which a broken step could lose independently:
    #   1. a non-zero status from the runner;
    #   2. the planted test named in the JUnit-derived failure list (the gate's failure channel);
    #   3. EVERY selected test ran — the report's total equals what `nextest list` selects. Without
    #      `--no-fail-fast` the run stops scheduling after the planted failure and this falls short;
    #      that is the "count is a lower bound" defect the gate exists to prevent.
    if [ "$rc" -ne 0 ] && [ "$frc" -eq 0 ] && [ "$named" -eq 1 ] && [ -n "$want" ] && [ "$got" -eq "$want" ]; then
        echo "SELF-TEST: PASS — the planted failure was detected (exit $rc), named in the JUnit"
        echo "           failure list ($nfail failed in all), and all $got selected tests ran"
        rm -f "$out" "$out.failures" "$out.report"
        return 0
    fi
    echo "SELF-TEST: FAIL — runner exit $rc, planted test named: $named, tests in report: $got of ${want:-?}"
    echo "           full output: $out  (failure list: $out.failures)"
    return 1
}

if [ "$MODE" = "self-test" ]; then self_test; exit $?; fi

# ---------------------------------------------------------------------------- drift guard (#1151)
#
# WHAT THIS DETECTS, AND WHAT IT DOES NOT. `COMMIT` and `DIRTY` above are read once, before step 1,
# while every step resolves HEAD or scans the tree at ITS OWN step time — a ~2 h run gives HEAD and
# the tree plenty of time to move, and one real verdict already named a commit made 39 minutes in on
# a different branch. This guard samples the tree and HEAD at every step boundary and voids the run
# on a change.
#
# It catches drift that PERSISTS to a sample point. It does NOT catch a mutate-and-revert inside a
# single step: two of the incidents that motivated it were sub-second edits that would hash
# identically at both boundaries. Detecting those needs a filesystem watcher whose own liveness is
# verified — #1231. Do not write "the gate detects mid-run mutation" anywhere; it detects
# PERSISTENT mid-run mutation.
#
# The fingerprint is a temp-index `write-tree`, NOT a hash of `git status` + `git diff`. Those are
# blind to the CONTENT of untracked files — status prints `?? path` and diff prints nothing — and an
# untracked file is the likeliest mutation (the self-test's own fixture is one). The real index is
# copied first so stat-clean files reuse their cached hashes instead of re-hashing the tree. HEAD is
# compared separately: `git commit` mid-run changes no working-tree content but moves HEAD, and the
# trailer and review lints read commits.
#
# `target/` is gitignored, so the gate's own log writing does not register — verify that stays true
# if .gitignore changes, because an unignored target/ would make this fire on every run.
fingerprint() {
    _idx=$(mktemp) || return 1
    cp .git/index "$_idx" 2>/dev/null || :
    GIT_INDEX_FILE="$_idx" git add -A >/dev/null 2>&1
    GIT_INDEX_FILE="$_idx" git write-tree 2>/dev/null
    rm -f "$_idx"
}

if [ "$MODE" = "fingerprint" ]; then fingerprint; exit 0; fi

START_TREE=$(fingerprint)
START_HEAD=$(git rev-parse HEAD 2>/dev/null || echo "unknown")
GATE_T0=$(date +%s)

# Called BEFORE each step and once after the last. Aborting at the next boundary rather than at the
# end keeps a 2 h test step from being spent on a run that is already void — and it is what makes
# the full-path sabotage cheap: mutate during the seconds-long fmt step and the gate aborts before
# clippy starts. It lives here rather than inside run_step, whose contract is running ONE command;
# drift policy is sequencing, not step execution.
drift_check() {
    _tree=$(fingerprint)
    _head=$(git rev-parse HEAD 2>/dev/null || echo "unknown")
    echo "=== tree $_tree head $_head ===" >> "$LOG"
    [ "$_tree" = "$START_TREE" ] && [ "$_head" = "$START_HEAD" ] && return 0
    _what=""
    [ "$_tree" != "$START_TREE" ] && _what="working tree"
    [ "$_head" != "$START_HEAD" ] && _what="${_what:+$_what and }HEAD"
    _steps=$([ "${rc_total:-0}" -eq 0 ] && echo PASS || echo FAIL)
    cat > "$VERDICT" <<JSON
{
  "result": "INVALID",
  "reason": "$_what changed during the run",
  "steps_result": "$_steps",
  "commit": "$START_HEAD",
  "commit_end": "$_head",
  "tree_start": "$START_TREE",
  "tree_end": "$_tree",
  "timestamp": "$STAMP",
  "log": "$LOG"
}
JSON
    echo
    echo "  The $_what changed while the gate was running, so this run proves nothing about any"
    echo "  single state of the repo. Steps so far reported $_steps — not discarded, but not"
    echo "  attributable either. Rerun on a quiet checkout; put concurrent work in a git worktree."
    echo "GATE: INVALID $START_HEAD $_what $STAMP (steps reported $_steps)"
    exit 3
}

echo "gate: commit $COMMIT ($DIRTY)  log $LOG"
rc_total=0
drift_check
# The build lock itself (fnec-rust FND-184): a nested take must not wait on its own
# parent, and an unrelated process must still wait. On a scratch lock file, so it
# runs here while this gate holds the real one.
run_step "host-build-lock self-test" bash scripts/test-host-build-lock.sh || rc_total=1
# The on-air runners may lower a rig's power, never raise it (the station owner's rule, 2026-10-14).
run_step "rig power: lower only" bash scripts/test-rig-power.sh || rc_total=1
run_step "cargo fmt --check" cargo fmt --all -- --check || rc_total=1
drift_check
run_step "cargo clippy -D warnings" cargo clippy --workspace --no-default-features --all-targets -- -D warnings || rc_total=1
drift_check
# The SHIPPED configuration, which the step above structurally cannot see (#1418). `--all-targets`
# puts dev units in scope, and resolver 2 then unifies dev-dependency features into the normal build
# — three dev-deps enable `openpulse-modem`'s `instruments` (its own Cargo.toml:41,
# openpulse-daemon:92, openpulse-kiss:59), and under `--workspace` any one suffices. So the step
# above always lints the lib with `instruments` ON, while `cargo build --release
# --no-default-features` (release.yml) links it OFF. Measured: an ungated production caller of an
# instruments-only item returned rc=0 from the step above AND from the pre-push hook, and rc=101
# (E0599) from `cargo clippy -p openpulse-daemon --no-default-features`.
#
# This is an ADDED pass, not a changed flag: dropping `--all-targets` above would stop linting test
# code, which is how an unused binding sat in session_key.rs. It must stay `--workspace` — a
# DOWNSTREAM crate's production code calling an instruments item also escapes, because that crate's
# lib builds against the ON modem. ~1 s warm; 9.1 s from a cold modem lib.
run_step "cargo clippy (shipped cfg) -D warns" cargo clippy --workspace --no-default-features -- -D warnings || rc_total=1
drift_check
# FEATURE-GATED code, which neither pass above compiles at all (#1380). `#[cfg(feature = "x")]` code
# must still PARSE when the feature is off, so a syntax error is caught — but nothing after parsing
# is: type errors, borrow errors, wrong arity, a renamed method. That is exactly the code most likely
# to drift, because nobody compiles it while editing something else. Precedent is not hypothetical:
# PR #424 found a build break, a clippy finding and a flaky test reachable only through the `gpu`
# feature, and this pass found two more the day it was written — a `serve`-gated test left behind
# when `LinkParams` gained two fields (uncompilable, therefore never run, for ~3 months) and a
# `float-literal-f32-fallback` in the `gui` binary that rustc says becomes a hard error.
#
# `--all-features` rather than a list of crate+feature pairs: a hand-maintained mirror of the feature
# set is the same rotting artifact this pass exists to catch. Safe because every feature here is
# ADDITIVE (`generic-serial = ["serial"]`); there is no mutually exclusive pair to break.
#
# TWO STATED LIMITS, neither closed by this pass:
#   * Not closed over TARGETS. A feature whose optional dependency is target-filtered (gpio's
#     `gpiocdev` is `[target.'cfg(target_os = "linux")']`) compiles here but not elsewhere; the code
#     must carry `all(target_os = ..., feature = ...)`, as gpio.rs now does.
#   * Not closed over the SHIPPED recipe. `--all-features` turns `instruments` on, so an
#     instruments-only item called from a `cpal-backend`-gated production path passes all three
#     passes and fails only `cargo build --release -p openpulse-cli --features cpal-backend`. Zero
#     instances today; it is a residual, not a claim of completeness.
#
# The preflight is NOT ceremony: --all-features pulls alsa-sys, libudev-sys and libdbus-sys, whose
# build scripts call pkg_config and PANIC when a .pc file is absent. Without this, a missing distro
# package reads as an unintelligible build-script backtrace in the middle of clippy output.
missing_pc=""
for pc in alsa libudev dbus-1; do
    pkg-config --exists "$pc" 2>/dev/null || missing_pc="$missing_pc $pc"
done
if [ -n "$missing_pc" ]; then
    printf '  %-38s%s\n' "cargo clippy (all features)" "SKIPPED (missing pkg-config:$missing_pc)"
    echo "  install: libasound2-dev libudev-dev libdbus-1-dev  (Debian/Ubuntu names)"
    echo "=== cargo clippy (all features): SKIPPED — missing pkg-config:$missing_pc ===" >> "$LOG"
    rc_total=1
else
    run_step "cargo clippy (all features) -D warns" cargo clippy --workspace --all-features --all-targets -- -D warnings || rc_total=1
fi

TEST_CMD="none"
if [ "$MODE" = "full" ]; then
    TEST_CMD="$NEXTEST_CMD && $DOCTEST_CMD"
    JUNIT=$(junit_path)
    # Not a skip: the step below fails without it. This only makes the reason readable.
    if ! cargo nextest --version >/dev/null 2>&1; then
        echo "  cargo-nextest is not installed: cargo install --locked cargo-nextest --version $NEXTEST_VERSION"
    fi
    drift_check
    run_step "cargo nextest (workspace)" run_nextest || rc_total=1
    # The report step's output is what the counts below and trace.py read; its status is 2 when the
    # report is missing (the run died before writing it), 1 when a test failed.
    run_step "$STEP_REPORT" report_step || rc_total=1
    drift_check
    run_step "$STEP_DOCTEST" $DOCTEST_CMD || rc_total=1
    drift_check
    run_step "nextest/cargo test parity" python3 scripts/lib/nextest_parity.py check || rc_total=1
    # Traceability is checked INSIDE the gate, not a separately-disableable job: enforced
    # requirements with no in-code binding, cited code/tests that don't exist, REQ<->CAP
    # disagreement, a capability that satisfies a requirement while citing no test (EMPTY-CAP,
    # #1268), and NEW code orphans beyond the grandfathered baseline.
    drift_check
    run_step "trace check (requirements)" env GATE_LOG="$LOG" scripts/trace.sh check || rc_total=1
    # Reachability ratchet: a NEW public item no production code references (coverage would vouch
    # for it as "covered" if a test touches it). Grandfathered baseline; only growth fails.
    drift_check
    run_step "reachability ratchet" scripts/reachability.sh check || rc_total=1
    # Requirements-trailer lint on this branch's commits. Here so a local `GATE: PASS` predicts CI:
    # traceability.yml runs the same check on every PR, and a gate that green-lights a push CI then
    # rejects has stopped meaning anything. Inspects COMMITS only, so a dirty tree mid-work does not
    # trip it; `--quick` skips it with the rest. (The PR *body* half of this check — the squash
    # message that actually lands on main — can only run in CI, where the body exists.)
    drift_check
    run_step "requirements-trailer lint" scripts/check-trailer.sh || rc_total=1
    # Doc frontmatter. The checker is grandfathered against `docs/.frontmatter-baseline.txt` and
    # fails only on NEW offenders, which is sound — but until 2026-09-13 it ran in NO workflow at
    # all: `docs.yml` is its only CI host and has been `disabled_manually` since 2026-06-24, and
    # `CLAUDE.md` said to run it by hand, which nobody did. It accumulated 40 new offenders in the
    # 19 days before anyone looked (#1349). Cheap (no build, ~0.2 s over the tree).
    #
    # This is the LOCAL half only, on the same reasoning as the review-trailer lint below: a green
    # gate should predict CI. Re-enabling `docs.yml` is the other half and is the maintainer's call
    # — the workflow also carries REQ-DOC-01's version-bump gate, and why it was switched off is
    # recorded nowhere (#1129 and #1134 both escalated it and neither got an answer).
    drift_check
    run_step "doc frontmatter" scripts/validate-doc-frontmatter.sh || rc_total=1
    # `unsafe` must arrive WITH a UB check (#1380 follow-up; maintainer decision 2026-09-15).
    #
    # Miri itself is NOT run here, deliberately: the workspace has zero `unsafe`, so a Miri step
    # would be a gate that cannot fail — and `rustup` is absent on this host, so `cargo miri` does
    # not exist. The `code-quality-gates` skill's rule is that a workspace with no unsafe "skips the
    # step honestly"; this is that skip, made SELF-ARMING so the obligation cannot be lost by
    # whoever first writes `unsafe`. It passes while there is none, and fails the moment any appears
    # without a Miri step wired alongside it.
    #
    # Cheap (no build, one grep over the tree). Placed AFTER `cargo fmt --check`, which it depends
    # on: the scan is line-based, and rustfmt is what guarantees `unsafe {` is not split across two
    # lines. `scripts/check-unsafe.sh --self-test` is the committed sabotage.
    drift_check
    run_step "unsafe/UB tripwire" scripts/check-unsafe.sh || rc_total=1
    # Ledger ordering. The file declares "Newest first" and had drifted into two regimes — 12 breaks
    # across 359 entries — because the convention lived only in prose and nothing measured it. Cheap
    # (no build, no I/O beyond one file), so it costs nothing to keep honest.
    drift_check
    run_step "ledger ordering" scripts/check-ledger-order.sh || rc_total=1
    # Secondary host only. The PR-body lint in traceability.yml is what actually enforces the
    # review trailer; this reports the classification locally so a design-class branch is known
    # to need an artifact BEFORE the PR is opened.
    drift_check
    run_step "review-trailer lint" scripts/check-review.sh --base "$(git merge-base HEAD origin/main 2>/dev/null || echo HEAD)" || rc_total=1
    # Re-homed doc / attribute lint (#1345). A diff can move a `///` doc or an outer `#[...]` onto the
    # wrong item without touching either, and clippy only sees the blank-line form; 29 live insertion
    # steals sat under a green gate. The script resolves the merge-base itself and FAILS on an
    # unresolvable base; there is no `|| echo HEAD` fallback, because that diffs HEAD against itself
    # and passes vacuously.
    drift_check
    run_step "re-homed docs lint" scripts/check-rehomed-docs.sh || rc_total=1
    # The hook git actually runs must be the hook in the tree (#1448); see hook_drift above.
    drift_check
    run_step "installed pre-push hook" hook_drift || {
        rc_total=1
        echo "    refresh it: cp .cargo-husky/hooks/pre-push \"$(git rev-parse --git-path hooks/pre-push)\""
    }
fi

drift_check   # final boundary: nothing moved between the last step and the verdict

# Counts come from the LOG FILE, never from a pipe carrying the runner's output — and only from the
# two steps whose `test result:` lines are the gate's own: the JUnit report (libtest-shaped, one per
# nextest binary) and the doctest run. nextest's own step is EXCLUDED: when a test fails nextest
# replays its stdout, which holds libtest's `test result:` line for that one-test process, and
# counting it would double the failure. `step` tracks which run_step section a line is in.
COUNTED_STEPS="$STEP_REPORT|$STEP_DOCTEST"
count_field() {  # $1 = passed; | failed; | suites
    awk -v want="$1" -v steps="$COUNTED_STEPS" '
        BEGIN { n = split(steps, a, "|"); for (i = 1; i <= n; i++) ok[a[i]] = 1 }
        /^=== end / { step = ""; next }
        /^=== (time|tree) / { next }
        /^=== / { s = substr($0, 5); sub(/: .*/, "", s); step = s; next }
        !(step in ok) || !/^test result:/ { next }
        want == "suites" { c++; next }
        { for (i = 1; i <= NF; i++) if ($i == want) c += $(i-1) }
        END { print c + 0 }' "$LOG"
}
passed=$(count_field "passed;")
failed=$(count_field "failed;")
suites=$(count_field "suites")

# The failure list is printed IN FULL. Truncating it is the defect this script exists to prevent.
if [ "$failed" -gt 0 ] || [ "$rc_total" -ne 0 ]; then
    echo ""
    echo "FAILURES (complete, untruncated):"
    if [ "$MODE" = "full" ]; then
        # nextest failures by binary and test, straight from the JUnit report.
        python3 scripts/lib/nextest_junit.py failures "$JUNIT" 2>&1 | sed 's/^/  /'
        # Doctest failures, from the doctest step's own libtest output only.
        # A doctest's name is `<file> - <item> (line N)`, so it is matched by that shape.
        awk -v h="=== $STEP_DOCTEST: " -v e="=== end $STEP_DOCTEST:" '
             index($0, h) == 1 {d=1} index($0, e) == 1 {d=0}
             d && /^failures:$/{f=1;next} d && /^test result:/{f=0}
             d && f && /^    [^ ].* \(line [0-9]+\)$/{sub(/^    /, ""); print "  doctest " $0}' "$LOG" | sort -u
    fi
    grep -E '^error(\[|:)' "$LOG" | sort -u | sed 's/^/  /'
fi

echo ""
echo "suites=$suites tests_passed=$passed tests_failed=$failed"
# A green gate must not read as "everything passed". Acceptance suites are held out for runtime
# (#1274) — REQ-QRM-01's notch gate and the OTA rate-adaptation suite, ~83 min between them, and the
# spectral-busy decode counts (#1454) — and nothing else in this output would say so.
echo "held-out (runtime, #1274): notch_rescues_interferer, ota_channel_adaptation, spectral_busy_gathers_weak_frames (decode counts, #1454) — run scripts/slow-tests.sh"

if [ "$MODE" = "quick" ]; then
    echo "GATE: PARTIAL $COMMIT $DIRTY $STAMP (fmt+clippy only — NOT a gate, no token written)"
    drift_check   # --quick shares the guard by reference; a drifted PARTIAL is quoted too
    exit $rc_total
fi

# A full run that counted no tests is not a pass, whatever the statuses say: it means the counter
# read nothing (a renamed step, a changed header format), not that nothing failed.
if [ "$MODE" = "full" ] && [ "$passed" -eq 0 ]; then
    echo "  no tests were counted — the counting channel is broken, so this cannot be a PASS"
    rc_total=1
fi

if [ "$rc_total" -eq 0 ] && [ "$failed" -eq 0 ]; then
    result="PASS"
else
    result="FAIL"
fi

# The toolchain is part of what a verdict is attributable to, and it is the member that drifts
# WITHOUT anyone performing an act: a distro package upgrade re-derives every cached lint verdict.
# The pin in rust-toolchain is unenforceable on this host (no rustup), so this record is the only
# thing that makes that drift visible to anything reading the verdict later.
TOOLCHAIN=$(rustc -V 2>/dev/null || echo "unknown")
# The test runner is part of what a verdict is attributable to, for the same reason.
NEXTEST=$(cargo nextest --version 2>/dev/null | head -1)
NEXTEST=${NEXTEST:-none}

cat > "$VERDICT" <<JSON
{
  "result": "$result",
  "commit": "$COMMIT",
  "tree": "$DIRTY",
  "toolchain": "$TOOLCHAIN",
  "nextest": "$NEXTEST",
  "timestamp": "$STAMP",
  "command": "$TEST_CMD",
  "suites": $suites,
  "tests_passed": $passed,
  "tests_failed": $failed,
  "log": "$LOG"
}
JSON

echo "gate wall time: $(fmt_secs $(( $(date +%s) - GATE_T0 )))"
echo "GATE: $result $COMMIT $DIRTY $STAMP"
[ "$result" = "PASS" ] && exit 0 || exit 1
