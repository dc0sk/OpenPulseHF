#!/usr/bin/env bash
# Surface every branch that is in none of the three good states (branch-lifecycle backstop).
#
# The three stale branches this was written for shared one signature: pushed to origin, **no PR ever
# opened**, findings posted as issue comments, branch left behind. `delete_branch_on_merge` is true
# on the repo and did nothing for them, because they never merged. So the check is not "was it
# merged" but "is it tracked by anything at all".
#
# Deliberately does NOT delete. Deleting work needs a human; this only makes the pile visible.
#
# Usage:  scripts/branch-audit.sh [--days N] [--strict]
#   --days N   age in days past which an untracked branch is reported (default 7)
#   --strict   exit 1 when any branch is untracked past --days, so a caller can gate on it
set -uo pipefail

DAYS=7
STRICT=0
while [ $# -gt 0 ]; do
  case "$1" in
    --days) DAYS="${2:?--days needs a number}"; shift 2 ;;
    --strict) STRICT=1; shift ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

cd "$(git rev-parse --show-toplevel)" || exit 2

# Derive the default branch; never assume "main".
DEFAULT_REF=$(git symbolic-ref --short refs/remotes/origin/HEAD 2>/dev/null)
if [ -z "$DEFAULT_REF" ]; then
  git remote set-head origin --auto >/dev/null 2>&1
  DEFAULT_REF=$(git symbolic-ref --short refs/remotes/origin/HEAD 2>/dev/null)
fi
[ -n "$DEFAULT_REF" ] || { echo "cannot derive origin's default branch" >&2; exit 2; }
DEFAULT_BRANCH=${DEFAULT_REF#origin/}

HAVE_GH=0
if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then HAVE_GH=1; fi

# Without `gh` every branch classifies as "unknown", the untracked count stays 0, and --strict would
# exit 0 HAVING CHECKED NOTHING — a green result meaning "I could not look", indistinguishable from
# "nothing is wrong". Refuse instead. (Hole found downstream, in the skills repo's port of this.)
if [ "$STRICT" = 1 ] && [ "$HAVE_GH" = 0 ]; then
  echo "--strict needs gh (authenticated): PR state is what this classifies on, and without it" >&2
  echo "the audit would report a clean bill of health for a check it never performed." >&2
  exit 2
fi

now=$(date +%s)
cutoff=$((DAYS * 86400))
untracked=0
printf '%-46s %-8s %-9s %s\n' BRANCH AGE PR STATE
printf '%-46s %-8s %-9s %s\n' "---" "---" "---" "---"

for b in $(git for-each-ref --format='%(refname:short)' refs/heads/); do
  [ "$b" = "$DEFAULT_BRANCH" ] && continue
  [ "$b" = "gh-pages" ] && continue

  ts=$(git log -1 --format=%ct "$b")
  age=$(( (now - ts) / 86400 ))
  upstream=$(git for-each-ref --format='%(upstream:short)' "refs/heads/$b")

  pr="-"
  if [ "$HAVE_GH" = 1 ]; then
    pr=$(gh pr list --head "$b" --state all --limit 1 --json state --jq '.[0].state' 2>/dev/null)
    [ -z "$pr" ] && pr="none"
  else
    pr="?"
  fi

  # Two-dot, not three: three-dot diffs the merge-base and is non-empty for every branch with
  # commits, merged or not. PR state still wins when the two disagree.
  if git diff --quiet "$DEFAULT_REF..$b" 2>/dev/null; then absorbed=1; else absorbed=0; fi

  case "$pr" in
    MERGED) state="merged - delete it" ;;
    OPEN)   state="active - open PR" ;;
    CLOSED) state="DEAD END - PR closed unmerged, work not in $DEFAULT_BRANCH" ;;
    none)
      if [ "$absorbed" = 1 ]; then
        state="content already in $DEFAULT_BRANCH - delete it"
      elif [ -z "$upstream" ]; then
        state="local only, never pushed - live work, leave alone"
      else
        state="UNTRACKED - pushed, no PR ever opened"
      fi
      ;;
    *) state="unknown (no gh); absorbed=$absorbed upstream='${upstream:-none}'" ;;
  esac

  case "$state" in
    UNTRACKED*|DEAD\ END*)
      if [ "$age" -ge "$DAYS" ]; then untracked=$((untracked + 1)); fi
      ;;
  esac

  printf '%-46s %-8s %-9s %s\n' "$b" "${age}d" "$pr" "$state"
done

echo
if [ "$untracked" -gt 0 ]; then
  cat <<EOF
$untracked branch(es) older than ${DAYS}d are tracked by nothing. Each needs a terminal state, not
another sweep: land it (a research harness belongs in the tree #[ignore]d, with a runner), open a PR,
or record the dead end on its issue AND delete the branch. "Left alone" is how this list grows.
EOF
  [ "$STRICT" = 1 ] && exit 1
fi
exit 0
