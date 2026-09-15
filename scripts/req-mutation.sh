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

# SKIPPED is a distinct outcome from PASS, and refusable (#1279).
#
# This used to print SKIPPED and `exit 0`, so a CI runner without the tool produced a GREEN job —
# the header's own warning that "a release must not claim the vacuous-binding gate ran if it did
# not" was exactly what the code did. Exit 0 is kept as the default because the header's other
# intent is right (a missing tool on a dev box should not block), but the marker is now
# machine-greppable and `REQ_MUTATION_REQUIRED=1` turns absence into a failure, which is what a
# release or a scheduled job must set.
if ! command -v cargo-mutants >/dev/null 2>&1; then
    echo "req-mutation: cargo-mutants not installed."
    echo "  install: cargo install cargo-mutants ; then re-run."
    echo "REQ-MUTATION: SKIPPED — the vacuous-binding gate did NOT run."
    if [ "${REQ_MUTATION_REQUIRED:-0}" = "1" ]; then
        echo "  REQ_MUTATION_REQUIRED=1, so a skip is a failure here." >&2
        exit 2
    fi
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
    # `--output target/` keeps cargo-mutants' `mutants.out/` (megabytes, plus a rotated
    # `mutants.out.old/`) out of the repo ROOT, where it defaults. That directory is untracked, so
    # in the repo root it trips `gate.sh`'s drift guard — which fingerprints untracked files — and
    # is one `git add -A` away from being committed. `target/` is gitignored.
    cargo mutants --no-shuffle --output target "${fargs[@]}" -- $testfilter > "$out" 2>&1
    mrc=$?
    missed=$(grep -c '^MISSED' "$out" 2>/dev/null); missed=${missed:-0}
    total=$(grep -cE '^(MISSED|CAUGHT|UNVIABLE|TIMEOUT)' "$out" 2>/dev/null); total=${total:-0}
    killed=$(grep -c '^CAUGHT' "$out" 2>/dev/null); killed=${killed:-0}
    echo "  mutants=$total killed=$killed missed=$missed (log $out, exit $mrc)"

    # DID IT RUN? (#1279) `total` used to be a PRECONDITION for judging — `total > 0 && killed == 0`
    # — so a run that produced no mutant lines at all fell through every branch and the script
    # printed PASS. A crashed build, a bad filter or an OOM kill therefore read as a clean verdict.
    # `total == 0` is now a FAILURE TO RUN, which is a different thing from a clean run and must
    # never be reported as one. cargo-mutants exits 0 when every mutant was caught and non-zero when
    # some were missed, so `mrc` alone cannot carry this — it is checked only for the crash case,
    # where it is non-zero AND nothing was parsed.
    if [ "$total" -eq 0 ]; then
        echo "  $rid: DID-NOT-RUN — cargo-mutants produced no mutant results (exit $mrc). This is"
        echo "                     NOT a pass; see $out. Common causes: the baseline build failed,"
        echo "                     the test filter matched nothing, or the run was killed."
        rc=1
        continue
    fi
    if [ "$killed" -eq 0 ]; then
        echo "  $rid: VACUOUS-BINDING — the bound tests killed ZERO mutants in the capability code."
        rc=1
    fi
done

[ "$rc" -eq 0 ] && echo "REQ-MUTATION: PASS" || echo "REQ-MUTATION: FAIL"
exit $rc
