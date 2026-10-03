#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 Simon Keimer (DC0SK)
#
# Known-pass / known-fail cases for `host-build-lock.sh`, on a scratch lock file
# (never the real one, so this can run inside a gate that holds it).
#
#   1. Nested: a script that takes the lock, then runs a script that takes it
#      too, must not wait on itself. It did, for an hour, on 2026-09-29.
#   2. Contention: an unrelated process must still wait while another holds it —
#      re-entrancy must not become "the lock never blocks".
#   3. Fail closed: an unopenable lock path refuses to run unlocked.
set -uo pipefail

HELPER="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib/host-build-lock.sh"
# A failed mktemp left SCRATCH empty and the cases ran against /lock, /inner.sh and /outer.sh —
# and as root still printed OK. Refuse instead.
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/test-host-build-lock.XXXXXX")" && [[ -d "$SCRATCH" ]] || {
    echo "FAIL: cannot create a scratch directory for the lock self-test" >&2; exit 1
}
trap 'rm -rf "$SCRATCH"' EXIT
export HEAVY_BUILD_LOCK="$SCRATCH/lock"
fail=0

# 1. Nested, three levels deep. `timeout` turns a deadlock into a failure.
cat > "$SCRATCH/inner.sh" <<EOF
source "$HELPER" inner
echo INNER_RAN
EOF
cat > "$SCRATCH/outer.sh" <<EOF
source "$HELPER" outer
bash "$SCRATCH/inner.sh"
EOF
out="$(timeout 10 bash -c "source '$HELPER' shell; bash '$SCRATCH/outer.sh'" 2>&1)"
rc=$?
if [[ $rc -ne 0 || "$out" != *INNER_RAN* ]]; then
    echo "FAIL nested take: exit $rc" >&2; echo "$out" >&2; fail=1
elif [[ "$out" == *waiting* ]]; then
    echo "FAIL nested take waited on itself:" >&2; echo "$out" >&2; fail=1
fi

# 2. An unrelated holder: the second process must wait (and time out here).
( source "$HELPER" holder; sleep 4 ) 2>/dev/null &
holder=$!
sleep 1
out="$(timeout 2 bash -c "source '$HELPER' stranger; echo STRANGER_RAN" 2>&1)"
rc=$?
if [[ $rc -ne 124 || "$out" == *STRANGER_RAN* || "$out" != *waiting* ]]; then
    echo "FAIL contention: an unrelated process did not wait (exit $rc):" >&2; echo "$out" >&2; fail=1
fi
wait "$holder" 2>/dev/null

# 3. Fail closed on a lock path that cannot be opened.
out="$(HEAVY_BUILD_LOCK="$SCRATCH/no/such/dir/lock" bash -c "source '$HELPER' closed; echo RAN_UNLOCKED" 2>&1)"
rc=$?
if [[ $rc -eq 0 || "$out" == *RAN_UNLOCKED* ]]; then
    echo "FAIL fail-closed: ran without the lock (exit $rc):" >&2; echo "$out" >&2; fail=1
fi

if [[ $fail -ne 0 ]]; then exit 1; fi
echo "host-build-lock self-test OK"
