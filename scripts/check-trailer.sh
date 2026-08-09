#!/usr/bin/env bash
# Requirements-trailer lint — where "no code without a requirement" binds at merge time.
#
# Every commit that touches PRODUCTION code must carry a trailer accounting for the change:
#   Implements: REQ-x[, REQ-y]      product behaviour serving a requirement
#   Refactors:  CAP-x               structural change to a capability (inherits its requirement)
#   Verification-objective: <text>  tooling / test / infra tree (the two-trees rule)
# Implements/Refactors IDs are validated against requirements.yaml — a dangling ID fails. A commit
# with no recognised trailer fails. Bright-line: commits before requirements.yaml's `bright_line`
# date are grandfathered (history is not rewritten); newer commits are enforced.
#
# Usage:  scripts/check-trailer.sh [BASE_REF]      # default base: origin/main (else main)
#         scripts/check-trailer.sh --self-test
set -u
REPO_ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$REPO_ROOT" || exit 2
YAML="docs/dev/project/requirements.yaml"

# Production code = source trees only. Tests, docs, scripts, CI, config are not product code.
is_prod() {  # reads a NUL-free file list on stdin, exits 0 if any is production source
    grep -Eq '^(crates|plugins|apps|tools|pki-tooling)/.*/src/.*\.rs$' && return 0
    grep -Eq '^(crates|plugins)/[^/]+/src/' 2>/dev/null
}

valid_ids() {  # $1=list of IDs, $2=Implements|Refactors ; echo bad IDs (empty = all good)
    python3 - "$1" "$2" <<'PY'
import sys, yaml
ids = [x for x in sys.argv[1].replace(",", " ").split() if x]
kind = sys.argv[2]
d = yaml.safe_load(open("docs/dev/project/requirements.yaml")) or {}
known = set(d.get("requirements", {})) if kind == "Implements" else set(d.get("capabilities", {}))
print(" ".join(i for i in ids if i not in known))
PY
}

lint_range() {
    base="$1"
    bright=$(python3 -c "import yaml;print(yaml.safe_load(open('$YAML'))['meta'].get('bright_line',''))" 2>/dev/null)
    range="$base..HEAD"
    commits=$(git rev-list --no-merges "$range" 2>/dev/null)
    [ -z "$commits" ] && { echo "trailer-lint: no commits in $range"; return 0; }
    fail=0
    for c in $commits; do
        cdate=$(git show -s --format=%cs "$c")              # commit date YYYY-MM-DD
        if [ -n "$bright" ] && [ "$cdate" \< "$bright" ]; then continue; fi   # grandfathered
        files=$(git show --no-commit-id --name-only --pretty="" "$c")
        if ! printf '%s\n' "$files" | is_prod; then continue; fi   # no production code touched
        msg=$(git show -s --format=%B "$c")
        impl=$(printf '%s\n' "$msg" | sed -n 's/^Implements:[[:space:]]*//p')
        refac=$(printf '%s\n' "$msg" | sed -n 's/^Refactors:[[:space:]]*//p')
        vobj=$(printf '%s\n' "$msg" | sed -n 's/^Verification-objective:[[:space:]]*//p')
        short=$(git show -s --format='%h %s' "$c")
        if [ -z "$impl$refac$vobj" ]; then
            echo "  FAIL $short"
            echo "       touches production code but has no Implements:/Refactors:/Verification-objective: trailer"
            fail=1; continue
        fi
        bad=""
        [ -n "$impl" ]  && bad="$bad $(valid_ids "$impl" Implements)"
        [ -n "$refac" ] && bad="$bad $(valid_ids "$refac" Refactors)"
        bad=$(echo $bad)
        if [ -n "$bad" ]; then
            echo "  FAIL $short"
            echo "       trailer names IDs not in requirements.yaml: $bad"
            fail=1
        fi
    done
    if [ "$fail" -eq 0 ]; then echo "TRAILER-LINT: PASS"; return 0; fi
    echo "TRAILER-LINT: FAIL"; return 1
}

if [ "${1:-}" = "--self-test" ]; then
    # A synthetic message with a dangling REQ id must be rejected by valid_ids.
    bad=$(valid_ids "REQ-DOES-NOT-EXIST-99" Implements)
    if [ "$bad" = "REQ-DOES-NOT-EXIST-99" ]; then
        echo "SELF-TEST: PASS — a dangling Implements: id is rejected"; exit 0
    fi
    echo "SELF-TEST: FAIL — dangling id was accepted (got '$bad')"; exit 1
fi

base="${1:-}"
if [ -z "$base" ]; then
    if git rev-parse --verify -q origin/main >/dev/null; then base="origin/main"; else base="main"; fi
fi
lint_range "$base"
