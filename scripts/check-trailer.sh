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
# THE SQUASH BLIND SPOT. This repo squash-merges, so a PR's per-commit messages are DISCARDED and
# what lands on `main` is the squash message, which GitHub composes from the PR title + body.
# Linting only `base..HEAD` therefore enforces trailers on commits that never survive and enforces
# nothing on the permanent record — it has worked so far only because the default squash body
# happens to concatenate the branch commits, which an edited message silently drops. `--message-file`
# lints a single message (CI passes the PR body), and that is the check guarding what actually lands.
#
# Usage:  scripts/check-trailer.sh [BASE_REF]           # lint commits in BASE_REF..HEAD
#         scripts/check-trailer.sh --message-file FILE  # lint ONE message (PR body / squash message)
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

# Lint ONE message (the PR body, i.e. the squash message that actually lands on main). Unlike
# lint_range this cannot inspect a diff, so it applies whenever the PR touches production code —
# the caller decides that; here we simply require a valid trailer in the text.
lint_message() {
    file="$1"
    [ -f "$file" ] || { echo "trailer-lint: no such message file: $file" >&2; return 2; }
    msg=$(cat "$file")
    impl=$(printf '%s\n' "$msg" | sed -n 's/^Implements:[[:space:]]*//p')
    refac=$(printf '%s\n' "$msg" | sed -n 's/^Refactors:[[:space:]]*//p')
    vobj=$(printf '%s\n' "$msg" | sed -n 's/^Verification-objective:[[:space:]]*//p')
    if [ -z "$impl$refac$vobj" ]; then
        echo "  FAIL: the PR body carries no Implements:/Refactors:/Verification-objective: trailer."
        echo "        This repo squash-merges, so the PR body becomes the commit message on main —"
        echo "        without a trailer there, the permanent record does not say what this serves."
        echo "        Add a line to the PR description, e.g.  Implements: REQ-FUN-12"
        echo "TRAILER-LINT: FAIL"; return 1
    fi
    bad=""
    [ -n "$impl" ]  && bad="$bad $(valid_ids "$impl" Implements)"
    [ -n "$refac" ] && bad="$bad $(valid_ids "$refac" Refactors)"
    bad=$(echo $bad)
    if [ -n "$bad" ]; then
        echo "  FAIL: PR-body trailer names IDs not in requirements.yaml: $bad"
        echo "TRAILER-LINT: FAIL"; return 1
    fi
    echo "TRAILER-LINT: PASS (PR body)"; return 0
}

if [ "${1:-}" = "--message-file" ]; then
    lint_message "${2:-}"; exit $?
fi

if [ "${1:-}" = "--self-test" ]; then
    # Two probes: a dangling id must be rejected, and a message with no trailer must be rejected.
    bad=$(valid_ids "REQ-DOES-NOT-EXIST-99" Implements)
    if [ "$bad" != "REQ-DOES-NOT-EXIST-99" ]; then
        echo "SELF-TEST: FAIL — dangling id was accepted (got '$bad')"; exit 1
    fi
    tmp=$(mktemp); printf 'fix: something\n\nno trailer here\n' > "$tmp"
    if lint_message "$tmp" >/dev/null 2>&1; then
        rm -f "$tmp"; echo "SELF-TEST: FAIL — a trailerless PR body was accepted"; exit 1
    fi
    rm -f "$tmp"
    # #1219: an unresolvable base must FAIL, never read as an empty (therefore clean) range.
    # A well-formed 40-hex object NAME satisfies `rev-parse --verify`, so this needs `^{commit}`.
    if "$REPO_ROOT/scripts/check-trailer.sh" "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef" >/dev/null 2>&1; then
        echo "SELF-TEST: FAIL — an unresolvable base was read as a clean range"; exit 1
    fi
    if "$REPO_ROOT/scripts/check-trailer.sh" "origin/no-such-branch-for-self-test" >/dev/null 2>&1; then
        echo "SELF-TEST: FAIL — a nonexistent base ref was read as a clean range"; exit 1
    fi
    # positive control: a resolvable base must still be lintable
    if ! "$REPO_ROOT/scripts/check-trailer.sh" "HEAD" >/dev/null 2>&1; then
        echo "SELF-TEST: FAIL — a resolvable base was rejected"; exit 1
    fi
    echo "SELF-TEST: PASS — dangling id rejected, trailerless PR body rejected, unresolvable base rejected"; exit 0
fi

base="${1:-}"
if [ -z "$base" ]; then
    if git rev-parse --verify -q origin/main >/dev/null; then base="origin/main"; else base="main"; fi
fi
# FAIL CLOSED (#1219). `lint_range` reports "no commits in <range>" and returns 0 when the range is
# empty — which is also what an unresolvable base produces, since `git rev-list` fails into
# 2>/dev/null. So a bad base read exactly like a clean branch. Note `--verify` alone is not enough:
# it returns 0 for any well-formed 40-hex object NAME whether or not the object exists, which is the
# most likely bad input here (a stale base.sha). `^{commit}` is the check that actually fires.
if ! git rev-parse --verify --quiet "${base}^{commit}" >/dev/null 2>&1; then
    echo "trailer-lint: base '$base' does not resolve to a commit in this checkout." >&2
    echo "              Refusing to lint: an unresolvable base yields an empty range, which is" >&2
    echo "              indistinguishable from a compliant branch." >&2
    echo "TRAILER-LINT: FAIL"
    exit 2
fi
lint_range "$base"
