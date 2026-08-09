#!/usr/bin/env bash
# Traceability checker — requirements as data, verified against the tree.
#
# The hand-maintained matrix rotted because nothing could diff prose against reality. This wraps
# scripts/lib/trace.py, which stores the trace as data (docs/dev/project/requirements.yaml) and
# CHECKS it: dangling code/tests, REQ<->CAP disagreement, requirements with no coverage, and NEW
# code orphans (files no capability claims, beyond the grandfathered baseline). `check` runs INSIDE
# scripts/gate.sh so it cannot be a separately-disableable job.
#
# Usage:
#   scripts/trace.sh check        # verify; exit 1 on enforced/new failures (the gate)
#   scripts/trace.sh import       # (re)generate requirements.yaml from the existing docs
#   scripts/trace.sh render       # regenerate the matrix from requirements.yaml
#   scripts/trace.sh --self-test  # plant a failure, require check to catch it, revert
#
# SABOTAGE-VERIFY after every edit (--self-test does it): a checker nobody has watched fail is the
# self-consistent checker it exists to prevent.
set -u
REPO_ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$REPO_ROOT" || exit 2
PY="python3 scripts/lib/trace.py"
YAML="docs/dev/project/requirements.yaml"

case "${1:-}" in
    check)  shift; $PY check "$@"; exit $? ;;   # e.g. `check --release` adds the draft-shipped gate
    import) $PY import; exit $? ;;
    render) $PY render; exit $? ;;
    --self-test) ;;
    *) echo "usage: scripts/trace.sh {check|import|render|--self-test}" >&2; exit 2 ;;
esac

# --- self-test: prove `check` fails on a planted enforced defect ---------------------------------
[ -f "$YAML" ] || { echo "SELF-TEST: need $YAML (run import) first" >&2; exit 2; }
backup="$(mktemp)"; cp "$YAML" "$backup"
restore() { cp "$backup" "$YAML"; rm -f "$backup"; }
trap restore EXIT

# Append an enforced capability citing a code path that cannot exist.
cat >> "$YAML" <<'YML'
  CAP-SELFTEST-SABOTAGE:
    name: deliberate self-test sabotage (safe to see only during --self-test)
    satisfies: []
    code:
    - crates/this/path/does/not/exist.rs
    tests: []
    traceability: enforced
YML

out="$(mktemp)"
python3 scripts/lib/trace.py check > "$out" 2>&1
rc=$?
restore; trap - EXIT

if [ "$rc" -ne 0 ] && grep -q "DANGLING-CODE" "$out"; then
    echo "SELF-TEST: PASS — check detected the planted enforced defect (exit $rc)"
    rm -f "$out"; exit 0
fi
echo "SELF-TEST: FAIL — planted defect was NOT caught (exit $rc). The checker is not a gate."
cat "$out"; rm -f "$out"; exit 1
