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
# ONE regex, two uses. `is_prod` is a PREDICATE (exits 0/1, prints nothing) and `prod_files` is a
# FILTER (prints the matching paths). Keeping them as two hand-written greps is how they drift, and
# reading the predicate as a filter is a live mistake: `files=$(... | is_prod)` yields the EMPTY
# STRING on every input, so a relevance check written that way never runs and reports PASS.
PROD_RE='^(crates|plugins|apps|tools|pki-tooling)/.*/src/.*\.rs$|^(crates|plugins)/[^/]+/src/'

is_prod()    { grep -Eq "$PROD_RE"; }   # exits 0 if ANY line is production source
prod_files() { grep -E  "$PROD_RE"; }   # prints the production-source lines

# RELEVANCE, not just existence (#1371). `valid_ids` asks whether an id is REAL; this asks whether
# it is the RIGHT one. A live-but-unrelated id passed every check before this: a mode-retirement
# commit was attributed to "Signed classical handshake", and three preamble-probe commits to the OTA
# rate controller while CAP-76 (Preamble-correlation veto) existed and the next commit in that area
# used it. That does not break the build, it breaks the JOIN — the commit is attributed to a
# capability it never touched, and the one it did touch shows no implementing commit. Worse than a
# missing trailer, which is at least visibly missing.
#
# The rule: a named capability must own at least one file the commit touched, per `requirements.yaml`'s
# `code:` paths. SCOPE, stated because the escape hatch is where this design can go wrong: the check
# applies only when the commit touches at least one file owned by SOME capability. Otherwise it is
# SKIPPED and says so — a docs-only or test-only commit must not be forced to name a capability it
# does not touch, and equally "no owned files" must not become a way to opt out by also touching a
# doc, which is why the skip is printed rather than silent.
irrelevant_caps() {  # $1=CAP list, $2=NL-separated touched files ; echo caps owning none of them
    python3 - "$1" "$2" <<'PY'
import sys, yaml
ids = [x for x in sys.argv[1].replace(",", " ").split() if x]
touched = [f for f in sys.argv[2].splitlines() if f.strip()]
d = yaml.safe_load(open("docs/dev/project/requirements.yaml")) or {}
caps = d.get("capabilities", {})
owned = {c: [p for p in (caps.get(c, {}).get("code") or [])] for c in ids}
# "Owns" = the capability lists a path that is a prefix of a touched file, or vice versa (a
# capability may name a directory or an exact file).
def owns(paths, f):
    return any(f == p or f.startswith(p.rstrip("/") + "/") or p.startswith(f) for p in paths)
any_owner = any(
    owns([p for p in (v.get("code") or [])], f) for v in caps.values() for f in touched
)
if not any_owner:
    print("SKIP")          # no capability owns anything here; not this check's business
else:
    print(" ".join(c for c in ids if not any(owns(owned[c], f) for f in touched)))
PY
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
            continue
        fi
        if [ -n "$refac" ]; then
            off=$(irrelevant_caps "$refac" "$files")
            if [ "$off" = "SKIP" ]; then
                echo "  skip $short — no capability owns any touched file; relevance not checked"
            elif [ -n "$off" ]; then
                echo "  FAIL $short"
                echo "       trailer names a capability that owns NONE of the files this commit"
                echo "       touched: $off"
                echo "       (the id exists, but the commit is attributed to a capability it did"
                echo "        not touch, which breaks the requirement->implementation join)"
                fail=1
            fi
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
    diff_base="${2:-}"
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
    # RELEVANCE ON THE PR BODY — the path that actually guards `main`.
    #
    # The commit-range lint above checks relevance, but this repo SQUASH-merges, so those commits are
    # discarded and the PR body becomes the permanent message. Every one of the seven mislabels this
    # rule was built for is a squash message. Linting ids for EXISTENCE here while checking relevance
    # only on discarded commits is a gate on the wrong artifact — so the body gets the same rule,
    # against the same diff the review lint already classifies from.
    #
    # Scoped exactly like the commit path: production files only, and SKIP when no capability owns
    # any of them, so a docs-only or test-only PR is never forced to name a capability (the
    # 2026-09-14 decision). Without a base ref we cannot see a diff, so relevance is SKIPPED and
    # says so rather than silently passing.
    if [ -n "$refac" ]; then
        if [ -z "$diff_base" ]; then
            echo "  note: no --diff-base given, so PR-body relevance was NOT checked"
        elif ! git rev-parse --verify --quiet "$diff_base" >/dev/null; then
            echo "trailer-lint: base '$diff_base' does not resolve to a commit in this checkout." >&2
            echo "              Refusing to lint: an unresolvable base yields an empty diff, which is" >&2
            echo "              indistinguishable from a compliant PR." >&2
            echo "TRAILER-LINT: FAIL"; return 2
        else
            files=$(git diff --name-only "$diff_base"...HEAD | prod_files)
            if [ -n "$files" ]; then
                off=$(irrelevant_caps "$refac" "$files")
                if [ "$off" = "SKIP" ]; then
                    echo "  skip PR body — no capability owns any touched file; relevance not checked"
                elif [ -n "$off" ]; then
                    echo "  FAIL: the PR body's Refactors: names a capability that owns NONE of the"
                    echo "        files this PR touched: $off"
                    echo "        The squash message is what lands on main, so this is the record that"
                    echo "        would attribute the change to a capability it never touched."
                    echo "TRAILER-LINT: FAIL"; return 1
                fi
            fi
        fi
    fi
    echo "TRAILER-LINT: PASS (PR body)"; return 0
}

if [ "${1:-}" = "--message-file" ]; then
    # optional 3rd/4th arg: --diff-base <ref>, so the body can be judged against the PR's own diff
    if [ "${3:-}" = "--diff-base" ]; then
        lint_message "${2:-}" "${4:-}"; exit $?
    fi
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
    # #1371: the relevance rule needs both directions, or it is a rule nobody has watched fire.
    # CAP-59 is the radio/PTT capability; a commit touching only core's fec.rs does not touch it.
    off=$(irrelevant_caps "CAP-59" "crates/openpulse-core/src/fec.rs")
    if [ "$off" != "CAP-59" ]; then
        echo "SELF-TEST: FAIL — an unrelated capability was accepted as relevant (got '$off')"; exit 1
    fi
    # And the known-PASS direction, or the rule could be rejecting everything.
    off=$(irrelevant_caps "CAP-59" "crates/openpulse-radio/src/shared_ptt.rs")
    if [ -n "$off" ]; then
        echo "SELF-TEST: FAIL — a capability that owns the touched file was called irrelevant (got '$off')"; exit 1
    fi
    # THE PREDICATE PROBE. Everything above passes identically whether the caller fails on ANY
    # irrelevant id or only when ALL are irrelevant, so none of it pins the choice — and the choice
    # was nearly made the wrong way. A mixed trailer is the discriminator: CAP-38 owns engine.rs and
    # CAP-59 does not, so a commit touching only engine.rs and naming both must still be refused.
    #
    # ANY is deliberate (#1371, 2026-09-18). ALL was proposed to absorb one apparent false positive,
    # `09048b84` — which on inspection was not a rule error at all but a MAP GAP: CAP-33 was
    # semantically right and simply did not own the engine's OTA arm. Under ALL the cheapest fix to
    # a failure is to APPEND the suggested id and keep the wrong one, and the blessing rates make
    # that a real bypass rather than a theoretical one: CAP-66 owns a touched file in 49% of
    # production commits, and six ids bless every engine.rs commit.
    off=$(irrelevant_caps "CAP-59 CAP-38" "crates/openpulse-modem/src/engine.rs")
    if [ "$off" != "CAP-59" ]; then
        echo "SELF-TEST: FAIL — a mixed trailer did not isolate the irrelevant id (got '$off')"; exit 1
    fi

    # And the SKIP direction: a docs-only commit must not be judged on relevance at all.
    off=$(irrelevant_caps "CAP-59" "docs/dev/project/roadmap.md")
    if [ "$off" != "SKIP" ]; then
        echo "SELF-TEST: FAIL — a docs-only change was judged on capability relevance (got '$off')"; exit 1
    fi
    echo "SELF-TEST: PASS — dangling id rejected, trailerless PR body rejected, unresolvable base rejected, irrelevant capability rejected, relevant one accepted, docs-only skipped"; exit 0
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
