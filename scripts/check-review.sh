#!/usr/bin/env bash
# Adversarial-review trailer lint.
#
# Usage: scripts/check-review.sh --message-file FILE [--base SHA]   # lint a PR body (primary)
#        scripts/check-review.sh --base SHA                         # lint the local branch's diff
#        scripts/check-review.sh --self-test
#
# TWO TIERS, and the split is the whole design:
#
#   Tier 1 (every PR, one line). The body carries `Review: <path>` or `Review: none — <reason>`.
#     This converts a silent omission into an explicit recorded claim. `Review: none` is a legitimate
#     answer — most PRs are mechanical — but it is now a claim somebody wrote, greppable after the
#     fact (`git log --grep 'Review: none'`), rather than an absence nobody can see.
#
#   Tier 2 (design-class PRs). If the diff touches a decision site, `Review: none` FAILS and the
#     named artifact must exist and pass a structure check.
#
# WHY A PR-BODY LINT AND NOT A GATE STEP: the workspace gate does not run at merge (ci.yml scopes
# every job to release/** — issue #1144), takes 80-120 minutes, and is required on no ref. A check
# that fires weeks later fires after the author is gone. `traceability.yml` runs on EVERY PR
# including the `edited` event, which is where this belongs and where the sibling trailer lint
# demonstrably worked. gate.sh runs it too, for the local pre-push case only.
#
# WHAT THIS CANNOT DO: it cannot tell a real review from a fabricated one. Requiring artifact
# CONTENT converts forgetting into fabricating — a deliberate act rather than a lapse — and that is
# the ceiling for any checker that inspects only the repo. The provenance defence is that the review
# apparatus writes the artifact itself, so faking one means faking the apparatus.
#
# THE CLASSIFIER IS STRUCTURAL ON PURPOSE. An earlier draft hand-listed "wire-format files"; that
# would have re-committed the exact defect #1193 was about — a hand-maintained list, inside the
# checker written because a hand-maintained list rotted four times. Design-class is derived from
# where decisions are RECORDED (a design doc, a new module, the requirements registry), not from a
# list of files somebody must remember to extend.
set -u
REPO_ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$REPO_ROOT" || exit 2

REVIEW_DIR="docs/dev/reviews"
MIN_ARTIFACT_BYTES=1200   # a stub passes a "file exists" check; it must not pass this one

MSG_FILE=""
BASE=""
SELF_TEST=0
while [ $# -gt 0 ]; do
    case "$1" in
        --message-file) MSG_FILE="$2"; shift 2 ;;
        --base)         BASE="$2"; shift 2 ;;
        --self-test)    SELF_TEST=1; shift ;;
        *)              BASE="$1"; shift ;;
    esac
done

# Decision sites. Structural, not a file list:
#   docs/dev/design/**          — where a design decision is written down
#   */src/**       (NEW files)  — a new module is a structural decision
#   scripts/**     (NEW files)  — a new script is usually a new gate, i.e. a decision about how the
#                                 project verifies itself. This rule makes the classifier
#                                 SELF-CONSISTENT: it classifies its own introduction as
#                                 design-class, which is the least it should do.
#   .github/workflows/** (NEW)  — same reasoning, for CI
#   requirements.yaml           — the requirement/capability registry
is_design_class() {   # reads "STATUS<TAB>path" lines (git diff --name-status) on stdin
    awk -F'\t' '
        $2 ~ /^docs\/dev\/design\// { found = 1 }
        $2 ~ /^docs\/dev\/project\/requirements\.yaml$/ { found = 1 }
        $1 == "A" && $2 ~ /^(crates|plugins|apps|tools|pki-tooling)\/.*\/src\/.*\.rs$/ { found = 1 }
        $1 == "A" && $2 ~ /^scripts\// { found = 1 }
        $1 == "A" && $2 ~ /^\.github\/workflows\// { found = 1 }
        END { exit(found ? 0 : 1) }
    '
}

artifact_ok() {   # $1 = path ; echoes the reason it is bad, empty when good
    path="$1"
    [ -f "$path" ] || { echo "no such file"; return; }
    bytes=$(wc -c < "$path" | tr -d ' ')
    [ "$bytes" -lt "$MIN_ARTIFACT_BYTES" ] && { echo "only ${bytes}B; a review artifact under ${MIN_ARTIFACT_BYTES}B is a stub"; return; }
    grep -qiE '^##+[[:space:]]*prompt' "$path" || { echo "no '## Prompt' section — the artifact must record what was ASKED, or a reader cannot tell what was reviewed"; return; }
    grep -qiE '^##+[[:space:]]*verdict' "$path" || { echo "no '## Verdict' section — the artifact must record what came BACK"; return; }
    echo ""
}

lint_message() {   # $1 = message text, $2 = design-class (0/1)
    msg="$1"; design="$2"
    review=$(printf '%s\n' "$msg" | sed -n 's/^Review:[[:space:]]*//p' | head -1)

    if [ -z "$review" ]; then
        echo "  FAIL: no 'Review:' trailer."
        echo "        Every PR records whether it was adversarially reviewed. Add ONE of:"
        echo "          Review: $REVIEW_DIR/<file>.md"
        echo "          Review: none — <why> (e.g. mechanical; applies a verdict already given)"
        return 1
    fi

    case "$review" in
        none*|None*|NONE*)
            if [ "$design" -eq 1 ]; then
                echo "  FAIL: 'Review: none' on a design-class change."
                echo "        This diff touches a decision site (a design doc, a NEW src module, or"
                echo "        requirements.yaml). The standing rule reviews those BEFORE implementation."
                echo "        Name the artifact: Review: $REVIEW_DIR/<file>.md"
                return 1
            fi
            reason=$(printf '%s' "$review" | sed 's/^[Nn][Oo][Nn][Ee][[:space:]]*[-—:]*[[:space:]]*//')
            if [ -z "$reason" ]; then
                echo "  FAIL: 'Review: none' with no reason."
                echo "        The point is a recorded claim, not a formality. Say why."
                return 1
            fi
            echo "  ok: Review: none — $reason"
            return 0
            ;;
    esac

    why=$(artifact_ok "$review")
    if [ -n "$why" ]; then
        echo "  FAIL: review artifact '$review' — $why"
        return 1
    fi
    echo "  ok: $review"
    return 0
}

if [ "$SELF_TEST" -eq 1 ]; then
    # A gate nobody has watched fail is the self-consistent checker it exists to prevent.
    tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
    rc=0

    printf 'A PR with no trailer at all.\n' > "$tmp/m1"
    if lint_message "$(cat "$tmp/m1")" 0 >/dev/null 2>&1; then
        echo "SELF-TEST FAIL: a message with no Review: trailer was accepted"; rc=1
    else echo "  ok: missing trailer rejected"; fi

    printf 'body\n\nReview: none — mechanical\n' > "$tmp/m2"
    if lint_message "$(cat "$tmp/m2")" 1 >/dev/null 2>&1; then
        echo "SELF-TEST FAIL: 'Review: none' was accepted on a DESIGN-CLASS change"; rc=1
    else echo "  ok: 'none' rejected on design-class"; fi

    printf 'stub\n' > "$tmp/stub.md"
    printf 'body\n\nReview: %s\n' "$tmp/stub.md" > "$tmp/m3"
    if lint_message "$(cat "$tmp/m3")" 1 >/dev/null 2>&1; then
        echo "SELF-TEST FAIL: a STUB artifact was accepted"; rc=1
    else echo "  ok: stub artifact rejected"; fi

    # positive control: the checker must ACCEPT a good message, or it is vacuously strict
    printf 'body\n\nReview: none — applies a verdict already given\n' > "$tmp/m4"
    if lint_message "$(cat "$tmp/m4")" 0 >/dev/null 2>&1; then
        echo "  ok: a well-formed 'none' accepted (positive control)"
    else echo "SELF-TEST FAIL: a well-formed message was rejected"; rc=1; fi

    # positive control: a real artifact must pass
    { echo "# Review"; echo "## Prompt"; head -c 900 /dev/urandom | base64; echo "## Verdict"; echo ok; } > "$tmp/real.md"
    printf 'body\n\nReview: %s\n' "$tmp/real.md" > "$tmp/m5"
    if lint_message "$(cat "$tmp/m5")" 1 >/dev/null 2>&1; then
        echo "  ok: a structured artifact accepted on design-class (positive control)"
    else echo "SELF-TEST FAIL: a valid artifact was rejected"; rc=1; fi

    [ "$rc" -eq 0 ] && echo "REVIEW-LINT-SELF-TEST: PASS" || echo "REVIEW-LINT-SELF-TEST: FAIL"
    exit "$rc"
fi

# Classify the change
design=0
if [ -n "$BASE" ]; then
    if git rev-parse --verify --quiet "$BASE" >/dev/null 2>&1; then
        if git diff --name-status "$BASE...HEAD" 2>/dev/null | is_design_class; then design=1; fi
    else
        echo "review-lint: base '$BASE' is not a known commit; treating as non-design-class"
    fi
fi
[ "$design" -eq 1 ] && echo "review-lint: DESIGN-CLASS change (touches a decision site)" \
                    || echo "review-lint: ordinary change"

if [ -n "$MSG_FILE" ]; then
    [ -f "$MSG_FILE" ] || { echo "review-lint: no such message file: $MSG_FILE"; exit 2; }
    if lint_message "$(cat "$MSG_FILE")" "$design"; then
        echo "REVIEW-LINT: PASS"; exit 0
    fi
    echo "REVIEW-LINT: FAIL"; exit 1
fi

# No message to lint (local gate run): report the classification so the author sees it before push.
if [ "$design" -eq 1 ]; then
    echo "  note: this branch is design-class — its PR body will need a real Review: artifact."
fi
echo "REVIEW-LINT: PASS"
exit 0
