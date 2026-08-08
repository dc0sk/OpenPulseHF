#!/usr/bin/env bash
# Workspace coverage measurement. NOT a per-PR gate — see "Why not per-PR" below.
#
# Prints one authoritative verdict line, `COVERAGE:`, in the shape `scripts/gate.sh` uses for
# `GATE:`. Quote coverage numbers only from that line.
#
# Verification mechanics this follows (CLAUDE.md):
#   * no pipelines carry the verdict — the status comes from `cmd > log; rc=$?`
#   * `--no-fail-fast`, so one failing binary cannot truncate the measurement into a lower bound
#   * a sanity check on the harness itself, because coverage fails in the FLATTERING direction:
#     a run that silently skipped crates reports a *high* number and nothing looks wrong
#
# Why not per-PR: instrumented, this workspace runs far longer than the ~50 min uninstrumented
# suite — a 50-minute bound was hit mid-suite without producing a number. A per-PR coverage job
# would cost more than the entire rest of CI (see PR #1094, which removed ~99 min/PR of duplicated
# testing). This is a scheduled + pre-release measurement instead.
#
# Usage:  scripts/coverage.sh [--json-out FILE]
set -uo pipefail

cd "$(dirname "$0")/.."

ts=$(date -u +%Y%m%dT%H%M%SZ)
log="target/coverage-${ts}.log"
json_out=""
[ "${1:-}" = "--json-out" ] && json_out="${2:-}"

mkdir -p target

echo "coverage: commit $(git rev-parse HEAD) $(git diff --quiet && echo clean || echo dirty)  log ${log}"

# `rs_vs_conv_cpu_time` asserts a CPU-time RATIO and is skipped under instrumentation via
# CARGO_LLVM_COV — instrumentation does not slow the two codecs proportionally, so the measurement
# is invalid here rather than merely noisy.
cargo llvm-cov --workspace --no-default-features --no-fail-fast \
    --summary-only > "$log" 2>&1
rc=$?

if [ $rc -ne 0 ]; then
    echo "--- failures (untruncated) ---"
    grep -E "^(test result: FAILED|error|failures:)" "$log" || true
    echo "COVERAGE: FAIL $(git rev-parse --short HEAD) rc=$rc ${ts}"
    exit 1
fi

# TOTAL line: llvm-cov prints regions/functions/lines/branches; take the LINE coverage percentage.
total_line=$(grep -E "^TOTAL" "$log" | tail -1)
pct=$(echo "$total_line" | grep -oE "[0-9]+\.[0-9]+%" | sed -n '3p' | tr -d '%')
files=$(grep -cE "^[a-zA-Z].*\.rs" "$log")

# Harness sanity. Coverage fails flattering: a run that skipped crates reports a HIGH number with
# nothing visibly wrong. These are weak checks and deliberately labelled as such — the strong one
# is manual and documented below.
if [ -z "$pct" ]; then
    echo "COVERAGE: FAIL — no TOTAL line parsed; the report shape changed"
    exit 1
fi
if [ "$files" -lt 50 ]; then
    echo "COVERAGE: FAIL — only ${files} files in the report; crates were skipped"
    exit 1
fi

echo
echo "$total_line"
echo "files reported: ${files}"
echo
echo "STRONGER HARNESS CHECK, do this by hand when the number moves unexpectedly:"
echo "  pick a function known to have NO test (issue #1092 lists 16 with no caller at all)"
echo "  and confirm it reports 0%. If it reports covered, the measurement is wrong and the"
echo "  number means nothing yet."
echo
echo "NO THRESHOLD IS CONFIGURED. This records a number; it does not judge one. A bar invented"
echo "before a baseline exists is fitted to whatever the code happens to do today."
echo
[ -n "$json_out" ] && printf '{"commit":"%s","timestamp":"%s","line_pct":%s,"files":%s}\n' \
    "$(git rev-parse HEAD)" "$ts" "$pct" "$files" > "$json_out"

echo "COVERAGE: OK ${pct}% lines  $(git rev-parse --short HEAD)  ${ts}"
