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

if command -v flock >/dev/null 2>&1; then
    exec 9>"$_hbl_path"
    if ! flock -n 9; then
        echo "$_hbl_who: another heavy build holds $_hbl_path — waiting for it to finish" >&2
        flock 9
        echo "$_hbl_who: lock acquired, continuing" >&2
    fi
else
    echo "$_hbl_who: flock not installed — running WITHOUT the host-wide build lock" >&2
fi
