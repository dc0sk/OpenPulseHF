#!/usr/bin/env bash
# Requirement-scoped mutation — the vacuous-binding gate (tier 2 of "the test validates its req").
#
# Whole-repo mutation answers "are the changed lines tested?" This answers the traceability
# question: does the test bound to REQ-x actually VALIDATE REQ-x? It mutates ONLY that requirement's
# capability code and runs ONLY that requirement's bound tests. If zero mutants die, the // VERIFIES
# link is a green line over a test that proves nothing (the gate that discards its decode result;
# the acceptance file that never calls the function it names). That is a vacuous binding -> FAIL.
#
# Cadence (per the skill): diff-scoped per PR, full per-CAP scheduled and pre-release. NOT in the
# fast gate.sh — mutation is minutes per file. Run it in the scheduled/mutation CI job and at release.
#
#   scripts/req-mutation.sh REQ-FUN-05        # scope + mutate + verdict for one requirement
#   scripts/req-mutation.sh --all-enforced    # every enforced requirement
set -u
REPO_ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$REPO_ROOT" || exit 2

if ! command -v cargo-mutants >/dev/null 2>&1; then
    echo "req-mutation: cargo-mutants not installed — SKIPPED."
    echo "  install: cargo install cargo-mutants ; then re-run. (This is a scheduled/release gate,"
    echo "  not the per-push gate, so a missing tool skips rather than blocks — but a release must"
    echo "  not claim the vacuous-binding gate ran if it did not.)"
    exit 0
fi

targets=()
if [ "${1:-}" = "--all-enforced" ]; then
    mapfile -t targets < <(python3 - <<'PY'
import yaml
d = yaml.safe_load(open("docs/dev/project/requirements.yaml"))
for rid, r in d["requirements"].items():
    if r.get("traceability") == "enforced":
        print(rid)
PY
)
elif [ -n "${1:-}" ]; then
    targets=("$1")
else
    echo "usage: scripts/req-mutation.sh {REQ-ID|--all-enforced}" >&2; exit 2
fi

rc=0
for rid in "${targets[@]}"; do
    echo "=== $rid ==="
    files=(); tests=()
    while IFS=$'\t' read -r kind val; do
        [ "$kind" = "CODE" ] && files+=("$val")
        [ "$kind" = "TEST" ] && tests+=("$val")
    done < <(python3 scripts/lib/trace.py scope "$rid")

    if [ "${#files[@]}" -eq 0 ]; then
        echo "  $rid: no capability code to mutate — is it covered? (trace check owns that)"; rc=1; continue
    fi
    if [ "${#tests[@]}" -eq 0 ]; then
        echo "  $rid: no // VERIFIES bound test to run — MISSING-BINDING (trace check owns that)"; rc=1; continue
    fi

    fargs=(); for f in "${files[@]}"; do fargs+=(-f "$f"); done
    # Run ONLY this requirement's bound tests against mutants of ONLY its capability code.
    testfilter=""; for t in "${tests[@]}"; do testfilter="$testfilter $t"; done
    out="target/mutants-$rid.log"
    echo "  mutating: ${files[*]}"
    echo "  bound tests: ${tests[*]}"
    cargo mutants --no-shuffle "${fargs[@]}" -- $testfilter > "$out" 2>&1
    mrc=$?
    caught=$(awk '/caught/{for(i=1;i<=NF;i++) if($i ~ /caught/) print $(i-1)}' "$out" | tail -1)
    missed=$(grep -c '^MISSED' "$out" 2>/dev/null); missed=${missed:-0}
    total=$(grep -cE '^(MISSED|CAUGHT|UNVIABLE|TIMEOUT)' "$out" 2>/dev/null); total=${total:-0}
    killed=$(grep -c '^CAUGHT' "$out" 2>/dev/null); killed=${killed:-0}
    echo "  mutants=$total killed=$killed missed=$missed (log $out, exit $mrc)"
    if [ "$total" -gt 0 ] && [ "$killed" -eq 0 ]; then
        echo "  $rid: VACUOUS-BINDING — the bound tests killed ZERO mutants in the capability code."
        rc=1
    fi
done

[ "$rc" -eq 0 ] && echo "REQ-MUTATION: PASS" || echo "REQ-MUTATION: FAIL"
exit $rc
