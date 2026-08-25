#!/usr/bin/env bash
# Reachability ratchet — public items no PRODUCTION code references (coverage vouches for these).
# Wraps scripts/lib/reachability.py. See that file for why coverage cannot see this.
#
#   scripts/reachability.sh report        # two numbers + the orphan list
#   scripts/reachability.sh check         # fail on NEW orphans vs the baseline (the ratchet)
#   scripts/reachability.sh baseline      # (re)write the grandfathered baseline
#   scripts/reachability.sh self-check    # prove the comment/string stripper works AND is wired
#   scripts/reachability.sh --self-test   # prove `check` fails on a planted new orphan
set -u
REPO_ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$REPO_ROOT" || exit 2

case "${1:-report}" in
    report|check|baseline) python3 scripts/lib/reachability.py "$1"; exit $? ;;
    self-check) python3 scripts/lib/reachability.py self-check; exit $? ;;
    --self-test) ;;
    *) echo "usage: scripts/reachability.sh {report|check|baseline|self-check|--self-test}" >&2; exit 2 ;;
esac

# --self-test runs BOTH: the stripper self-check (lexer behaviour + proof it is wired into
# _analyze), then the planted-orphan test below. Without the first, a refactor that stops calling
# _strip_rust silently restores the false-PASS class #1192 closed, and the planted-orphan test
# still passes — it plants a symbol no comment mentions.
if [ "${1:-}" = "--self-test" ]; then
    python3 scripts/lib/reachability.py self-check || exit 1
fi

# self-test: plant a brand-new unreferenced public item and require `check` to fail on it.
[ -f docs/dev/project/reachability-baseline.txt ] || python3 scripts/lib/reachability.py baseline >/dev/null
scratch="crates/openpulse-core/src/reach_selftest_sabotage.rs"
cat > "$scratch" <<'RS'
// Temporary sabotage fixture written by scripts/reachability.sh --self-test. Safe to delete.
pub fn reach_self_test_orphan_that_nothing_calls() -> u8 { 42 }
RS
out="$(mktemp)"
python3 scripts/lib/reachability.py check > "$out" 2>&1
rc=$?
rm -f "$scratch"
if [ "$rc" -ne 0 ] && grep -q "reach_self_test_orphan_that_nothing_calls" "$out"; then
    echo "SELF-TEST: PASS — check detected the planted new orphan (exit $rc)"
    rm -f "$out"; exit 0
fi
echo "SELF-TEST: FAIL — planted orphan was NOT caught (exit $rc)."
cat "$out"; rm -f "$out"; exit 1
