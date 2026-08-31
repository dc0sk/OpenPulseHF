#!/usr/bin/env bash
# Traceability checker — requirements as data, verified against the tree.
#
# The hand-maintained matrix rotted because nothing could diff prose against reality. This wraps
# scripts/lib/trace.py, which stores the trace as data (docs/dev/project/requirements.yaml) and
# CHECKS it. requirements.yaml is now the SOURCE OF TRUTH, not a generated artifact: the
# `import` migration was deleted in #1223 because its sources had gone stale by construction
# (four live entries appeared in neither of them) while it still advertised itself as safe to
# re-run, so re-running it would have silently deleted them. Checks performed: dangling code/tests, REQ<->CAP disagreement, requirements with no coverage, and NEW
# code orphans (files no capability claims, beyond the grandfathered baseline). `check` runs INSIDE
# scripts/gate.sh so it cannot be a separately-disableable job.
#
# Usage:
#   scripts/trace.sh check        # verify; exit 1 on enforced/new failures (the gate)
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
    --self-test) ;;
    *) echo "usage: scripts/trace.sh {check|--self-test}" >&2; exit 2 ;;
esac

# --- self-test: prove `check` fails on planted defects, at LINE level ----------------------------
#
# Assertions are on the CHECK NAME in the output, never on the exit code alone. An rc-only test
# passes whenever the checker is red for any unrelated reason — and on 2026-08-30 it was: running
# `check` while a gate is in flight reads that gate's partial log and reports a false
# CITED-BUT-DIDN'T-RUN. An rc-only assertion would have "passed" without any of the code below
# working.
[ -f "$YAML" ] || { echo "SELF-TEST: need $YAML — restore it from git" >&2; exit 2; }
backup="$(mktemp)"; cp "$YAML" "$backup"
# NOTE: restore must NOT delete the backup — it is called once per probe. Deleting it made every
# probe after the first restore nothing, so the plants accumulated in the working tree. The
# positive control at the end is what caught that; without it the three probes still printed `ok`
# while the file on disk was being corrupted.
restore() { cp "$backup" "$YAML"; }
cleanup() { restore; rm -f "$backup"; }
trap cleanup EXIT

rc_all=0
out="$(mktemp)"

# $1 = label, $2 = expected check name in the output, $3 = yaml fragment to splice before CAP-01
plant_and_expect() {
    python3 - "$3" <<'PYEOF'
import io, sys
p = "docs/dev/project/requirements.yaml"
s = io.open(p, encoding="utf-8").read()
s = s.replace("  CAP-01:", sys.argv[1] + "  CAP-01:", 1)
io.open(p, "w", encoding="utf-8").write(s)
PYEOF
    python3 scripts/lib/trace.py check > "$out" 2>&1
    rc=$?
    restore
    if [ "$rc" -ne 0 ] && grep -q "$2" "$out"; then
        echo "  ok: $1 -> $2"
    else
        echo "  SELF-TEST FAIL: $1 did not produce $2 (exit $rc)"
        rc_all=1
    fi
}

# The three ways an entry evaded enforcement before #1158. The third is the one that matters most:
# it is the copy-paste genesis, and it is legal under a plain "absent field defaults to enforced".
plant_and_expect "absent traceability field" "NO-TRACEABILITY" \
'  ZZ-SELFTEST-1:
    name: self-test probe
    satisfies: []
    code: []
    tests: []
'
plant_and_expect "misspelled traceability value" "BAD-TRACEABILITY" \
'  ZZ-SELFTEST-2:
    name: self-test probe
    satisfies: []
    code: []
    tests: []
    traceability: enforcd
'
plant_and_expect "baseline on an id that is not grandfathered" "NOT-GRANDFATHERED" \
'  ZZ-SELFTEST-3:
    name: self-test probe
    satisfies: []
    code: []
    tests: []
    traceability: baseline
'

# #1229 id-conformance probes. The first plants THE DEFECT ITSELF — the two-category-segment shape
# that went unregisterable for months because both of trace.py's id patterns were blind to it (the
# id is assembled from fragments below; writing it out here would trip the very check it tests). The second
# is the half a shape check cannot see — a well-formed id that is not in the source of truth, which
# is how REQ-FT-01..07 were found. Both plant in a doc, so no source file is touched.
probe_doc="docs/dev/project/backlog.md"
doc_backup="$(mktemp)"; cp "$probe_doc" "$doc_backup"
probe_id() {   # $1 = planted text, $2 = expected check name, $3 = label
    printf '\n<!-- trace self-test probe --> %s\n' "$1" >> "$probe_doc"
    python3 scripts/lib/trace.py check > "$out" 2>&1
    rc=$?
    cp "$doc_backup" "$probe_doc"
    if [ "$rc" -ne 0 ] && grep -q "$2" "$out"; then
        echo "  ok: $3 -> $2"
    else
        echo "  SELF-TEST FAIL: $3 did not produce $2 (exit $rc)"; rc_all=1
    fi
}
# Assembled from fragments on purpose: this file is itself scanned, so a literal probe id here
# would make the detector fail on its own fixture. Concatenation keeps the fixture honest without
# widening the checker's vocabulary with a real-looking id.
probe_id "REQ-SEC-""CTL-99 is a new off-convention id." "BAD-ID-SHAPE"    "an off-convention id in a doc"
probe_id "REQ-""ZZZ-01 is well-shaped but unregistered." "UNREGISTERED-ID" "a conforming id absent from the yaml"
rm -f "$doc_backup"

# The original sabotage: an enforced entry citing a path that cannot exist. Kept because it proves
# the ENFORCEMENT path still fails the build, which the three above do not (they stop at the
# vocabulary gate before any check runs).
cp "$backup" "$YAML"
cat >> "$YAML" <<'YML'
  CAP-SELFTEST-SABOTAGE:
    name: deliberate self-test sabotage (safe to see only during --self-test)
    satisfies: []
    code:
    - crates/this/path/does/not/exist.rs
    tests: []
    traceability: enforced
YML
python3 scripts/lib/trace.py check > "$out" 2>&1
rc=$?
restore
if [ "$rc" -ne 0 ] && grep -q "DANGLING-CODE" "$out"; then
    echo "  ok: enforced entry citing a missing path -> DANGLING-CODE"
else
    echo "  SELF-TEST FAIL: planted enforced defect was NOT caught (exit $rc)"; cat "$out"; rc_all=1
fi

# --- #1237 workspace-dormancy probes ------------------------------------------------------------
#
# These need a REAL `// VERIFIES:` marker, because the join resolves a requirement through its
# binding's FILE to that file's package. A yaml-only plant has no binding and is skipped by
# construction, so it would prove nothing. Each plants a scratch .rs (undeclared, so cargo ignores
# it) in a package of known dormancy, and removes it in the same function that reads the result.
# The probe ids are assembled from fragments for the same reason the #1229 probes are: the
# id-conformance layer scans THIS file, so writing them out would trip the check they sit beside.
plant_binding_and_expect() {
    # $1 = label, $2 = expected check name, $3 = scratch .rs path, $4 = req id, $5 = yaml fragment
    printf '// Temporary fixture written by scripts/trace.sh --self-test. Safe to delete.\n// VERIFIES: %s — self-test probe\n#[test]\nfn zz_trace_selftest_probe() {}\n' "$4" > "$3"
    python3 - "$5" <<'PYEOF'
import io, sys
p = "docs/dev/project/requirements.yaml"
s = io.open(p, encoding="utf-8").read()
key = "\nrequirements:\n"
i = s.index(key) + len(key)
io.open(p, "w", encoding="utf-8").write(s[:i] + sys.argv[1] + s[i:])
PYEOF
    python3 scripts/lib/trace.py check > "$out" 2>&1
    rc=$?
    rm -f "$3"; restore
    if [ "$rc" -ne 0 ] && grep -q "$2" "$out"; then
        echo "  ok: $1 -> $2"
    else
        echo "  SELF-TEST FAIL: $1 did not produce $2 (exit $rc)"
        rc_all=1
    fi
}

# An `enforced` requirement bound inside a package nothing links to a binary. This is REQ-CTL-04's
# exact shape — a passing test on a capability with no consumer, which satisfied `enforced`
# completely before #1237.
ZZA="RE""Q-ZZ""A-01"
plant_binding_and_expect "enforced binding in a workspace-dormant package" "DORMANT-ENFORCED" \
    "crates/openpulse-keystore/src/zz_trace_selftest.rs" "$ZZA" \
"  $ZZA:
    category: self-test
    covered_by: []
    statement: self-test probe
    status: draft
    traceability: enforced
"

# The reverse direction, which is what keeps `unwired` from being a resting place: a requirement
# marked `unwired` whose package DOES reach a binary must be reconciled deliberately.
ZZB="RE""Q-ZZ""B-01"
plant_binding_and_expect "unwired binding in a package that reaches a binary" "UNWIRED-BUT-REACHED" \
    "crates/openpulse-core/src/zz_trace_selftest.rs" "$ZZB" \
"  $ZZB:
    category: self-test
    covered_by: []
    statement: self-test probe (#1237)
    status: draft
    traceability: unwired
"


# POSITIVE CONTROL. Without this, every assertion above is satisfied by a checker that fails on
# everything — including the empty-yaml refusal, which also exits non-zero.
python3 scripts/lib/trace.py check > "$out" 2>&1
rc=$?
if [ "$rc" -eq 0 ]; then
    echo "  ok: the unmodified tree still PASSES (positive control)"
else
    echo "  SELF-TEST FAIL: the unmodified tree does not pass (exit $rc) — the checks above prove nothing"
    tail -20 "$out"; rc_all=1
fi

rm -f "$out"

# The gate-log evidence channel (#1224), probed in python because the fixtures are log files rather
# than yaml. Included here so one command covers both halves of what `check` trusts: the yaml it
# reads, and the run-evidence it reads.
python3 scripts/lib/trace.py evidence-self-test || rc_all=1

[ "$rc_all" -eq 0 ] && echo "SELF-TEST: PASS" || echo "SELF-TEST: FAIL"
exit "$rc_all"
