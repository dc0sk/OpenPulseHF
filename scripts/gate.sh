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
#   * a machine-checkable last line (`GATE: PASS|FAIL ...`) and `target/gate-verdict.json`
#
# SABOTAGE-VERIFY THIS SCRIPT AFTER EVERY EDIT. A gate nobody has watched fail is exactly the
# self-consistent checker it exists to prevent — this repo's archetype #1 wearing a safety vest.
#   1. plant a failing assertion in any test
#   2. run this script
#   3. require `GATE: FAIL` with that test named in the failure list
#   4. revert
# `--self-test` runs steps 1-4 for you against a scratch test file.
#
# Usage:
#   scripts/gate.sh            # fmt + clippy + full workspace test
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
    "") ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
esac

COMMIT=$(git rev-parse HEAD 2>/dev/null || echo "unknown")
if [ -n "$(git status --porcelain 2>/dev/null)" ]; then DIRTY="dirty"; else DIRTY="clean"; fi

# Run one command, capture its REAL status. No pipes: `$?` after a pipeline is the last element's
# status, and ${PIPESTATUS}/$pipestatus differ between bash and zsh. This form works everywhere.
run_step() {
    step_name=$1; shift
    printf '  %-38s' "$step_name"
    echo "=== $step_name: $* ===" >> "$LOG"
    "$@" >> "$LOG" 2>&1
    rc=$?
    if [ "$rc" -eq 0 ]; then echo "ok"; else echo "FAILED (exit $rc)"; fi
    return $rc
}

self_test() {
    scratch="crates/openpulse-core/tests/gate_self_test_sabotage.rs"
    echo "SELF-TEST: planting a deliberately failing test at $scratch"
    cat > "$scratch" <<'RS'
// Temporary sabotage fixture written by scripts/gate.sh --self-test. Safe to delete.
#[test]
fn gate_self_test_must_fail() {
    assert_eq!(1, 2, "deliberate failure: if the gate reports PASS with this present, it is broken");
}
RS
    out="$LOG_DIR/gate-selftest-$STAMP.log"
    cargo test --workspace --no-default-features --no-fail-fast > "$out" 2>&1
    rc=$?
    rm -f "$scratch"
    if [ "$rc" -ne 0 ] && grep -q "gate_self_test_must_fail" "$out"; then
        echo "SELF-TEST: PASS — the gate detected the planted failure (exit $rc)"
        return 0
    fi
    echo "SELF-TEST: FAIL — planted failure was NOT detected (exit $rc). The gate is not a gate."
    echo "           full output: $out"
    return 1
}

if [ "$MODE" = "self-test" ]; then self_test; exit $?; fi

echo "gate: commit $COMMIT ($DIRTY)  log $LOG"
rc_total=0
run_step "cargo fmt --check" cargo fmt --all -- --check || rc_total=1
run_step "cargo clippy -D warnings" cargo clippy --workspace --no-default-features --all-targets -- -D warnings || rc_total=1

TEST_CMD="none"
if [ "$MODE" = "full" ]; then
    TEST_CMD="cargo test --workspace --no-default-features --no-fail-fast"
    run_step "cargo test (workspace)" cargo test --workspace --no-default-features --no-fail-fast || rc_total=1
    # Traceability is checked INSIDE the gate, not a separately-disableable job: enforced
    # requirements with no in-code binding, cited code/tests that don't exist, REQ<->CAP
    # disagreement, and NEW code orphans beyond the grandfathered baseline.
    run_step "trace check (requirements)" env GATE_LOG="$LOG" scripts/trace.sh check || rc_total=1
    # Reachability ratchet: a NEW public item no production code references (coverage would vouch
    # for it as "covered" if a test touches it). Grandfathered baseline; only growth fails.
    run_step "reachability ratchet" scripts/reachability.sh check || rc_total=1
    # Requirements-trailer lint on this branch's commits. Here so a local `GATE: PASS` predicts CI:
    # traceability.yml runs the same check on every PR, and a gate that green-lights a push CI then
    # rejects has stopped meaning anything. Inspects COMMITS only, so a dirty tree mid-work does not
    # trip it; `--quick` skips it with the rest. (The PR *body* half of this check — the squash
    # message that actually lands on main — can only run in CI, where the body exists.)
    run_step "requirements-trailer lint" scripts/check-trailer.sh || rc_total=1
fi

# Counts come from the LOG FILE, never from a pipe carrying the runner's output.
passed=$(awk '/^test result:/ {for(i=1;i<=NF;i++) if($i=="passed;") s+=$(i-1)} END {print s+0}' "$LOG")
failed=$(awk '/^test result:/ {for(i=1;i<=NF;i++) if($i=="failed;") s+=$(i-1)} END {print s+0}' "$LOG")
# `grep -c` prints 0 AND exits 1 when there are no matches, so `|| echo 0` appends a SECOND line
# and `suites` becomes "0\n0" — which produces invalid JSON in gate-verdict.json. Harmless only
# because --quick returns before the JSON write; fixed rather than left as a latent trap.
suites=$(grep -c '^test result:' "$LOG" 2>/dev/null | head -1)
suites=${suites:-0}

# The failure list is printed IN FULL. Truncating it is the defect this script exists to prevent.
if [ "$failed" -gt 0 ] || [ "$rc_total" -ne 0 ]; then
    echo ""
    echo "FAILURES (complete, untruncated):"
    awk '/^failures:$/{f=1;next} /^test result:/{f=0} f && /^    [a-zA-Z0-9_:]+$/{print "  " $1}' "$LOG" | sort -u
    grep -E '^error(\[|:)' "$LOG" | sort -u | sed 's/^/  /'
fi

echo ""
echo "suites=$suites tests_passed=$passed tests_failed=$failed"

if [ "$MODE" = "quick" ]; then
    echo "GATE: PARTIAL $COMMIT $DIRTY $STAMP (fmt+clippy only — NOT a gate, no token written)"
    exit $rc_total
fi

if [ "$rc_total" -eq 0 ] && [ "$failed" -eq 0 ]; then
    result="PASS"
else
    result="FAIL"
fi

cat > "$VERDICT" <<JSON
{
  "result": "$result",
  "commit": "$COMMIT",
  "tree": "$DIRTY",
  "timestamp": "$STAMP",
  "command": "$TEST_CMD",
  "suites": $suites,
  "tests_passed": $passed,
  "tests_failed": $failed,
  "log": "$LOG"
}
JSON

echo "GATE: $result $COMMIT $DIRTY $STAMP"
[ "$result" = "PASS" ] && exit 0 || exit 1
