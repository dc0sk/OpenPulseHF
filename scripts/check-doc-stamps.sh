#!/usr/bin/env bash
# `last_updated` diff ratchet (#1349). A wrapper around scripts/lib/doc_stamps.py.
#
# A doc CHANGED in this branch must carry a `last_updated` at or after the merge-base date. Docs that
# carry no stamp are exempt, exactly as docfront.py grandfathers them — see the module docstring for
# why that scope is deliberate rather than lazy.
#
# FAIL CLOSED ON THE BASE, following check-trailer.sh (#1219) and check-rehomed-docs.sh: an
# unresolvable base must not read as an empty, and therefore clean, range.
#
# Usage:  scripts/check-doc-stamps.sh [--base REF]   # REF defaults to origin/main
#         scripts/check-doc-stamps.sh --self-test
# Exit:   0 clean, 1 a changed doc carries a stale stamp, 2 the check could not run.
set -uo pipefail
REPO_ROOT="$(git rev-parse --show-toplevel)" || exit 2
cd "$REPO_ROOT" || exit 2
PY="$REPO_ROOT/scripts/lib/doc_stamps.py"

if [ "${1:-}" = "--self-test" ]; then
    python3 "$PY" --self-test
    exit $?
fi

base_ref="origin/main"
if [ "${1:-}" = "--base" ]; then
    base_ref="${2:-}"
    if [ -z "$base_ref" ]; then
        echo "usage: scripts/check-doc-stamps.sh [--base REF] | --self-test" >&2; exit 2
    fi
elif [ -n "${1:-}" ]; then
    echo "usage: scripts/check-doc-stamps.sh [--base REF] | --self-test" >&2; exit 2
fi

if ! git rev-parse --verify --quiet "${base_ref}^{commit}" >/dev/null 2>&1; then
    echo "doc-stamps: base '$base_ref' does not resolve to a commit in this checkout." >&2
    echo "            Refusing to check: an unresolvable base yields an empty range, which is" >&2
    echo "            indistinguishable from a clean one." >&2
    echo "DOC-STAMPS: FAIL"
    exit 2
fi

python3 "$PY" "$base_ref"
