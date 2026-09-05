#!/usr/bin/env bash
# The two acceptance suites held out of the default workspace run because of RUNTIME, not staleness.
#
# They cost ~83 minutes of the ~2 h `scripts/gate.sh` between them, which is most of it. They are
# `#[ignore]`d rather than feature-gated on purpose: an ignored test still compiles and is still
# linted by `--all-targets`, so it cannot rot the way a default-off feature can.
#
# Run this before any change that touches the receiver notch, the OTA rate controller, or the
# acquisition chain they exercise — and before a release. `scripts/gate.sh` does NOT run them.
#
#   scripts/slow-tests.sh          # both suites
#   scripts/slow-tests.sh notch    # just the notch acceptance suite (REQ-QRM-01)
#   scripts/slow-tests.sh ota      # just the OTA rate-adaptation suite (CAP-33)
#
# Named tests, never a blanket `-- --ignored`: the notch binary also holds `probe_band_sweep`, a
# manual env-driven research harness that asserts nothing, and `capture_replay_corpus` holds two
# tests ignored for a DIFFERENT reason (#1148, stale corpus) which would fail if swept in. An
# `--ignored` run mixes gates with non-gates and produces a verdict that means nothing.
set -u
REPO_ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$REPO_ROOT" || exit 2

want=${1:-all}
case "$want" in
    all|notch|ota) ;;
    *) echo "usage: scripts/slow-tests.sh {all|notch|ota}" >&2; exit 2 ;;
esac

rc_total=0
log_dir="$REPO_ROOT/target"
stamp=$(date -u +%Y%m%dT%H%M%SZ)

# One cargo invocation per SUITE, not per test: cargo runs the tests inside a binary in parallel,
# and these two suites are ~35 and ~48 minutes precisely BECAUSE their slowest test dominates while
# the others run alongside it. A per-test loop serialises them and roughly triples the wall clock,
# which was the first version of this script and is why the numbers here are per-suite.
#
# `--skip probe_band_sweep` is load-bearing: `--ignored` runs EVERY ignored test in the binary, and
# the notch binary also holds that manual env-driven research harness, which asserts nothing. It is
# ignored for a different reason and must not appear in a verdict about the notch gate.
run_suite() {  # $1=suite name, $2...=extra args passed after --
    local suite=$1; shift
    local log="$log_dir/slow-$suite-$stamp.log"
    echo "  $suite -> $log"
    # Two-line form, never a pipeline: the verdict must come from the command's own status.
    cargo test -p openpulse-modem --no-default-features --test "$suite" -- --ignored "$@" >"$log" 2>&1
    local rc=$?
    local result
    result=$(grep '^test result:' "$log" | tail -1)
    if [ "$rc" -ne 0 ]; then
        echo "    FAIL (exit $rc)  ${result:-<no test result line — the binary did not run>}"
        rc_total=1
        return
    fi
    # A pass whose result line says `0 passed` ran nothing: the ignore reason changed, a name moved,
    # or --skip swallowed the lot. That is a false PASS and must fail here.
    case "$result" in
        *"0 passed"*)
            echo "    FAIL ran no tests: $result"
            echo "         (an --ignored run that executes nothing exits 0 — the suite is not proven)"
            rc_total=1
            ;;
        *) echo "    ok   $result" ;;
    esac
}

[ "$want" = "ota" ]   || run_suite notch_rescues_interferer --skip probe_band_sweep
[ "$want" = "notch" ] || run_suite ota_channel_adaptation

if [ "$rc_total" -eq 0 ]; then
    echo "SLOW-TESTS: PASS"
else
    echo "SLOW-TESTS: FAIL"
fi
exit "$rc_total"
