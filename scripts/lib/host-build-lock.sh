# Take the HOST-WIDE heavy-build lock, held until the calling script exits.
# Sourced, not executed: `source scripts/lib/host-build-lock.sh "<who>"`.
#
# Why host-wide and not per repo. Nothing coordinates two heavy cargo runs on one
# machine. On 2026-09-25 another project's gate (fnec-rust) was OOM-killed three
# times while this repo's `cargo test --workspace` ran alongside it, and each kill
# read as a failure of the code under test — the same misattribution
# `GATE: INVALID` exists to prevent, arriving by a channel the gate cannot see.
# The lock file is shared across projects: fnec-rust's `scripts/host-build-lock.sh`
# takes the SAME path, so the two gates queue instead of racing for RAM.
#
# Two rules that keep it from deadlocking:
#   - a script takes it ONCE, near the top; nothing it calls takes it again
#     (the pre-push hook does not call gate.sh, so each may take it);
#   - do not wrap a script that takes it in `flock` yourself — the inner take
#     would wait forever on the lock the outer one holds.
#
# Override the path with HEAVY_BUILD_LOCK (every project must agree on it). If
# `flock` is not installed, the caller runs unlocked and says so.

_hbl_who="${1:-gate}"
_hbl_path="${HEAVY_BUILD_LOCK:-${XDG_RUNTIME_DIR:-/tmp}/heavy-build.lock}"

# Fail CLOSED once flock exists: a lock file that cannot be opened (its directory
# missing, or the file owned by another user) made both `flock` calls fail with
# EBADF, and the old code printed "waiting" and then "lock acquired" and ran
# unlocked — the one outcome this helper exists to prevent, reported as success.
# Re-entrant within one process tree. The lock lives on descriptor 9, which a
# child inherits, and a flock belongs to the open file description: when fd 9
# already refers to this lock file and re-locking it succeeds at once, an
# ancestor holds the lock for us, and taking it again would only wait on
# ourselves. It used to: on 2026-09-29 a fnec-rust shell that held the lock and
# then ran its gate (which takes it too) waited on its own parent for an hour,
# and this repo's pre-push queued behind it for 67 minutes. An unrelated process
# still opens its own description and still waits. (fnec-rust FND-184.)
_hbl_inherited=""
if [[ -e /proc/self/fd/9 ]] \
    && [[ "$(readlink -f /proc/self/fd/9 2>/dev/null)" == "$(readlink -f "$_hbl_path" 2>/dev/null)" ]] \
    && command -v flock >/dev/null 2>&1 && flock -n 9; then
    _hbl_inherited=1
fi

if [[ -n "$_hbl_inherited" ]]; then
    echo "$_hbl_who: this process tree already holds $_hbl_path — continuing" >&2
elif command -v flock >/dev/null 2>&1; then
    if ! exec 9>"$_hbl_path"; then
        echo "$_hbl_who: cannot open the host-wide build lock $_hbl_path — refusing to run unlocked" >&2
        exit 1
    fi
    if ! flock -n 9; then
        echo "$_hbl_who: another heavy build holds $_hbl_path — waiting for it to finish" >&2
        if ! flock 9; then
            echo "$_hbl_who: could not take $_hbl_path — refusing to run unlocked" >&2
            exit 1
        fi
        echo "$_hbl_who: lock acquired, continuing" >&2
    fi
else
    echo "$_hbl_who: flock not installed — running WITHOUT the host-wide build lock" >&2
fi
