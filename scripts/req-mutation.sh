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
# fast gate.sh — mutation is minutes per file.
#
#   scripts/req-mutation.sh REQ-FUN-05        # scope + mutate + verdict for one requirement
#   scripts/req-mutation.sh --all-enforced    # every enforced requirement
#
# NO VERDICT THIS SCRIPT COULD PRODUCE WAS TRUE UNTIL 2026-09-18, and the shape is the one the gate
# exists to catch, one layer down. Outcome counts were scraped from cargo-mutants' STDOUT with
# `grep -c '^CAUGHT'` / `grep -c '^MISSED'`. Three independent reasons that could not work, each
# measured against cargo-mutants 27.1.0:
#   1. CAUGHT is not printed unless you ask. `console.rs:80` returns early on
#      `outcome.mutant_caught() && !options.print_caught`; NEWS.md records the default flipping in
#      **0.2.0**, years before this script was written. So `killed` was structurally 0 — and this is
#      NOT version drift, so pinning a version would not have fixed it.
#   2. Even `--caught` would not have matched. The label is lowercase — `console.rs:615`,
#      `SummaryOutcome::CaughtMutant => style("caught")` — so `^CAUGHT` misses it either way.
#   3. On the CI runner `^MISSED` would have missed too. `mutation.yml` sets `CARGO_TERM_COLOR:
#      always`, which cargo-mutants reads for `--colors`, so the line begins `\033[31m\033[1mMISSED`.
# The consequences differ by environment, which is why a dev-box check could not have revealed it:
# on a laptop every mutant caught read as DID-NOT-RUN and any survivor read as VACUOUS-BINDING; on
# the nightly EVERY requirement would have read DID-NOT-RUN. Either way no real run could reach
# `REQ-MUTATION: PASS`, and the only PASS the script could emit was the enumerator fail-open below,
# which ran nothing. Counts now come from the files cargo-mutants WRITES, which are its own record
# and carry no formatting.
#
# KNOWN LIMITATION, not yet closed: libtest filters are SUBSTRING matches, so a bound test name that
# is a prefix of a sibling's runs the sibling too, and a kill attributed to this requirement may be
# the sibling's. Measured on REQ-SEC-13: two bound names selected FIVE tests, and 4 of 10 kills came
# from `wire_query::tests::tampered_payload_fails_verification` rather than the bound
# `signing::tests::tampered_payload_fails`. It inflates a PASS; it cannot manufacture one out of
# nothing. `-- --exact` fixes it but needs MODULE-QUALIFIED paths, which `_scan_verifies` does not
# record — tracked separately.
set -u
REPO_ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$REPO_ROOT" || exit 2

# SKIPPED is a distinct outcome from PASS, and refusable (#1279).
#
# Exit 0 is the default (a missing tool on a dev box should not block), but the marker is
# machine-greppable and `REQ_MUTATION_REQUIRED=1` turns absence into a failure, which is what a
# release or a scheduled job must set.
#
# The probe is `cargo mutants --version`, NOT `command -v cargo-mutants`: cargo resolves a subcommand
# from $CARGO_HOME/bin BEFORE $PATH, so on a box where ~/.cargo/bin is not on PATH — this one —
# `command -v` reports the tool absent while the very invocation below works. A guard that measures a
# different thing than the command it guards is not a guard.
if ! cargo mutants --version >/dev/null 2>&1; then
    echo "req-mutation: cargo-mutants not installed."
    echo "  install: cargo install cargo-mutants ; then re-run."
    echo "REQ-MUTATION: SKIPPED — the vacuous-binding gate did NOT run."
    if [ "${REQ_MUTATION_REQUIRED:-0}" = "1" ]; then
        echo "  REQ_MUTATION_REQUIRED=1, so a skip is a failure here." >&2
        exit 2
    fi
    exit 0
fi

# Keep cargo-mutants' scratch copy of the source tree OFF the tmpfs: it copies the whole workspace
# (measured: 787 MB / 1895 files) to $TMPDIR and builds there, and /tmp here is a 15 GB tmpfs.
#
# It must ALSO stay out of the repo unless it is under `target/`, and the reason is narrower than it
# looks. cargo-mutants skips the copy's source tree by TOP-LEVEL NAME — `copy_tree.rs` tests
# `is_top_level_target` — NOT because target/ is gitignored (its debug log records
# `git_ignore: false`). So a scratch dir anywhere else inside the repo is copied into itself:
# measured, `TMPDIR=$REPO_ROOT/tmp` dies with `File name too long (os error 36)` after recursing.
# An override pointing inside the repo but outside target/ is therefore refused rather than obeyed.
TMPDIR="${REQ_MUTATION_TMPDIR:-$REPO_ROOT/target/mutants-tmp}"
case "$(cd "$(dirname "$TMPDIR")" 2>/dev/null && pwd)/$(basename "$TMPDIR")" in
    "$REPO_ROOT"/target/*) ;;
    "$REPO_ROOT"/*)
        echo "req-mutation: REQ_MUTATION_TMPDIR='$TMPDIR' is inside the repo but not under target/." >&2
        echo "  cargo-mutants excludes only a top-level 'target', so it would copy this into itself." >&2
        exit 2 ;;
esac
export TMPDIR
mkdir -p "$TMPDIR"

targets=()
if [ "${1:-}" = "--all-enforced" ]; then
    # FAIL CLOSED ON THE ENUMERATOR. `mapfile -t targets < <(python3 …)` discards the producer's
    # exit status — measured: `mapfile rc=0 count=0` when the producer exits 1 — so a traceback
    # (missing PyYAML on a runner that never installed it) yielded an EMPTY target list, the loop
    # body never ran, and the script printed `REQ-MUTATION: PASS` having done no work. Capture the
    # status, then require a non-empty list — which also covers the honest case of a YAML with no
    # enforced requirements, where doing nothing is still not a pass.
    enum_log="$(mktemp "$TMPDIR/req-mutation-enum.XXXXXX")"
    python3 - > "$enum_log" 2>&1 <<'PY'
import yaml
d = yaml.safe_load(open("docs/dev/project/requirements.yaml"))
for rid, r in d["requirements"].items():
    if r.get("traceability") == "enforced":
        print(rid)
PY
    erc=$?
    if [ "$erc" -ne 0 ]; then
        echo "req-mutation: enumerating enforced requirements FAILED (exit $erc):" >&2
        cat "$enum_log" >&2
        rm -f "$enum_log"
        echo "REQ-MUTATION: FAIL"
        exit 2
    fi
    mapfile -t targets < "$enum_log"
    rm -f "$enum_log"
    if [ "${#targets[@]}" -eq 0 ]; then
        echo "req-mutation: no enforced requirements found — that is a broken query, not a pass." >&2
        echo "REQ-MUTATION: FAIL"
        exit 2
    fi
    echo "req-mutation: ${#targets[@]} enforced requirement(s) to check"
elif [ -n "${1:-}" ]; then
    targets=("$1")
else
    echo "usage: scripts/req-mutation.sh {REQ-ID|--all-enforced}" >&2; exit 2
fi

# `grep -c .` on a file of only blank lines prints 0 AND exits 1, so `[ -s "$f" ] && grep -c . "$f"
# || echo 0` emits "0\n0" — which makes the `$((…))` below an arithmetic syntax error, and under
# `set -u` bash EXITS there, printing no `REQ-MUTATION:` line at all. Unreachable with today's
# cargo-mutants (it never writes a blank line) but a silent-no-verdict failure is the wrong thing to
# leave latent in a gate.
count_lines() { local n; n=$(grep -c . "$1" 2>/dev/null); echo "${n:-0}"; }

rc=0
for rid in "${targets[@]}"; do
    echo "=== $rid ==="
    # Validated before it reaches `rm -rf` below. trace.py refuses an unknown id, but a path
    # traversal must not depend on a downstream component's manners.
    case "$rid" in
        REQ-[A-Z]*-[0-9]*) ;;
        *) echo "  '$rid' is not a requirement id"; rc=1; continue ;;
    esac

    files=(); tests=(); testpkgs=()
    while IFS=$'\t' read -r kind val; do
        [ "$kind" = "CODE" ] && files+=("$val")
        [ "$kind" = "TEST" ] && tests+=("$val")
        [ "$kind" = "TESTPKG" ] && testpkgs+=("$val")
    done < <(python3 scripts/lib/trace.py scope "$rid")

    if [ "${#files[@]}" -eq 0 ]; then
        echo "  $rid: no capability code to mutate — is it covered? (trace check owns that)"; rc=1; continue
    fi
    if [ "${#tests[@]}" -eq 0 ]; then
        echo "  $rid: no // VERIFIES bound test to run — MISSING-BINDING (trace check owns that)"; rc=1; continue
    fi

    args=(--no-shuffle --no-default-features)
    for f in "${files[@]}"; do args+=(-f "$f"); done
    # WITHOUT THIS, A CROSS-CRATE BINDING IS A FALSE VACUOUS-BINDING. cargo-mutants runs each
    # mutant's tests in the MUTATED file's package alone (`lab.rs` -> `PackageSelection::Explicit`),
    # so a requirement whose capability code and bound tests live in different crates runs ZERO
    # tests and every mutant survives. Measured on REQ-CTL-01's shape: `cargo test -p
    # openpulse-linksec -p openpulse-keystore -p openpulse-config -- noise_client_exchanges…`
    # reports "running 0 tests" six times, rc=0 — 303 mutants would all have been MISSED, at ~20
    # minutes, for a defect in the gate rather than in the tests. REQ-DCD-01, REQ-FUN-10,
    # REQ-FUN-11 and REQ-SEC-14 are partially the same shape.
    for p in "${testpkgs[@]}"; do args+=(--test-package "$p"); done
    testfilter=""; for t in "${tests[@]}"; do testfilter="$testfilter $t"; done

    out="target/mutants-$rid.log"
    # ONE OUTPUT DIRECTORY PER REQUIREMENT. `--output target` put every requirement's results in the
    # same `target/mutants.out`, rotating the previous to `mutants.out.old` and deleting that on the
    # third run — so an `--all-enforced` run destroyed the evidence for all but its last two.
    odir="target/mutants/$rid"
    rm -rf "$odir"; mkdir -p "$odir"
    echo "  mutating: ${files[*]}"
    echo "  bound tests: ${tests[*]}"
    [ "${#testpkgs[@]}" -gt 0 ] && echo "  test packages: ${testpkgs[*]}"
    # `--` twice: cargo-mutants passes trailing args to `cargo test` POSITIONALLY, and cargo accepts
    # exactly one positional. Measured — `cargo mutants -- a b` issues `cargo test --package=X a b`
    # and dies with `error: unexpected argument 'b' found`, failing the BASELINE, which this script
    # then reported as DID-NOT-RUN. `-- -- a b` issues `cargo test --package=X -- a b`, and libtest
    # takes several filters. Every requirement binding more than one test was red on arrival:
    # REQ-RX-02, REQ-RX-03, REQ-SEC-13, REQ-SEC-14.
    cargo mutants "${args[@]}" --output "$odir" -- -- $testfilter > "$out" 2>&1
    mrc=$?

    res="$odir/mutants.out"
    killed=$(count_lines "$res/caught.txt")
    missed=$(count_lines "$res/missed.txt")
    unviable=$(count_lines "$res/unviable.txt")
    timedout=$(count_lines "$res/timeout.txt")
    viable=$((killed + missed + timedout))
    total=$((viable + unviable))
    echo "  mutants=$total viable=$viable killed=$killed missed=$missed timeout=$timedout unviable=$unviable (log $out, exit $mrc)"

    # DID THE RUN FINISH? The outcome files are opened at start and APPENDED per mutant, so a run
    # killed part-way (OOM, SIGTERM, a CI job cancelled at its timeout) leaves a well-formed but
    # PARTIAL record — measured: a probe SIGTERMed 14 s in left caught=5 missed=1 of 19 mutants and
    # `end_time: null`. Counting those would report a verdict on a fraction of the mutants, and a
    # partial run with no kills yet is indistinguishable from a vacuous binding. `end_time` is the
    # tool's own completion marker; the exit status corroborates (0/2/3 are complete runs, 4 is a
    # failed baseline, everything else is an error).
    finished=$(python3 -c "
import json,sys
try:
    print('yes' if json.load(open('$res/outcomes.json')).get('end_time') else 'no')
except Exception:
    print('no')
" 2>/dev/null)
    if [ "$finished" != "yes" ] || { [ "$mrc" -ne 0 ] && [ "$mrc" -ne 2 ] && [ "$mrc" -ne 3 ]; }; then
        echo "  $rid: INCOMPLETE — the run did not finish (exit $mrc, end_time recorded: $finished)."
        echo "                     Whatever is in $res covers only part of the mutants, so it is not"
        echo "                     a verdict either way. See $out."
        rc=1
        continue
    fi
    if [ "$total" -eq 0 ]; then
        echo "  $rid: DID-NOT-RUN — cargo-mutants recorded no outcome in $res (exit $mrc). This is"
        echo "                     NOT a pass; see $out. Common causes: the baseline build failed,"
        echo "                     the test filter matched nothing, or the run was killed."
        rc=1
        continue
    fi
    if [ "$viable" -eq 0 ]; then
        echo "  $rid: NO-VIABLE-MUTANTS — all $unviable mutant(s) failed to compile, so the bound"
        echo "                     tests were never given anything to catch. Not a vacuous binding,"
        echo "                     and not evidence either."
        rc=1
        continue
    fi
    # "THE TESTS PROVE NOTHING" AND "NO TEST RAN" ARE DIFFERENT FINDINGS, and they are
    # indistinguishable from the counts alone — both give killed=0 with every mutant MISSED. The
    # second is a broken scope, and it is reachable two ways: a filter that matches no test name,
    # and the cross-crate case above if `--test-package` were ever dropped. Worth separating because
    # the issue this gate files would otherwise accuse a perfectly good test of being vacuous.
    #
    # Note the baseline cannot be used for this: measured, cargo-mutants runs the BASELINE against
    # the mutated package even when `--test-package` names another, so on a cross-crate requirement
    # the baseline reports `ok` having run `running 0 tests` twice. Only the per-mutant logs say
    # whether the bound tests actually executed.
    if [ "$killed" -eq 0 ]; then
        if grep -rqE 'running [1-9][0-9]* tests?' "$res/log/" 2>/dev/null; then
            echo "  $rid: VACUOUS-BINDING — the bound tests RAN and killed ZERO of $viable viable"
            echo "                     mutants in the capability code."
        else
            echo "  $rid: FILTER-MATCHED-NOTHING — no bound test executed in any mutant run, so the"
            echo "                     $viable surviving mutants are evidence about the SCOPE, not"
            echo "                     about the tests. Check the // VERIFIES names and the test"
            echo "                     packages against $res/log/."
        fi
        rc=1
    fi
done

[ "$rc" -eq 0 ] && echo "REQ-MUTATION: PASS" || echo "REQ-MUTATION: FAIL"
exit $rc
