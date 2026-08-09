#!/usr/bin/env bash
# Doc frontmatter validator + anti-rot constitution. Delegates to scripts/lib/docfront.py, which
# scans docs/ RECURSIVELY (the old check saw only docs/*.md) and enforces that `status: living` is
# legal only for docs listed in docs/.living-manifest.txt — a doc that claims to reflect the present
# state must be machine-maintained, or the label just suppresses suspicion while it rots.
# Grandfathered: `--baseline` records today's offenders; `check` fails only on NEW ones.
#
#   scripts/validate-doc-frontmatter.sh            # check (CI entrypoint)
#   scripts/validate-doc-frontmatter.sh --baseline # regenerate baseline + seed the living manifest
#   scripts/validate-doc-frontmatter.sh --self-test
set -u
REPO_ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$REPO_ROOT" || exit 2

case "${1:-check}" in
    --baseline) python3 scripts/lib/docfront.py baseline; exit $? ;;
    check|"")   python3 scripts/lib/docfront.py check;    exit $? ;;
    --self-test) ;;
    *) echo "usage: scripts/validate-doc-frontmatter.sh {check|--baseline|--self-test}" >&2; exit 2 ;;
esac

# self-test: a NEW doc with a bad status must be caught (not grandfathered).
scratch="docs/_docfront_selftest_sabotage.md"
cat > "$scratch" <<'MD'
---
project: openpulsehf
doc: docs/_docfront_selftest_sabotage.md
status: totally-not-a-valid-status
last_updated: 2026-08-09
---
sabotage fixture
MD
out="$(mktemp)"
python3 scripts/lib/docfront.py check > "$out" 2>&1
rc=$?
rm -f "$scratch"
if [ "$rc" -ne 0 ] && grep -q "_docfront_selftest_sabotage" "$out"; then
    echo "SELF-TEST: PASS — a new invalid-status doc was caught (exit $rc)"
    rm -f "$out"; exit 0
fi
echo "SELF-TEST: FAIL — planted bad doc was NOT caught (exit $rc)."
cat "$out"; rm -f "$out"; exit 1
