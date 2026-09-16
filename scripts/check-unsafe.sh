#!/usr/bin/env bash
# `unsafe` must arrive WITH a UB check, never before one — a tripwire that arms itself.
#
# WHY THIS SHAPE, and not `cargo miri test` in the gate (maintainer decision, 2026-09-15): this
# workspace has ZERO `unsafe` constructs, so a Miri step today would be a gate that cannot fail —
# the anti-pattern this repo treats as a defect in its own right. It is also unrunnable here:
# `rustup` is absent, so there is no nightly and `cargo miri` does not exist. The `code-quality-gates`
# skill states the rule and its exit condition — "a crate with `unsafe` gets a Miri job in this same
# gate … a workspace with none of them skips the step honestly" — and this is the honest skip, made
# self-arming so the obligation cannot be forgotten by whoever first writes `unsafe`.
#
# WHAT IT DOES NOT DO: it does not check that `unsafe` is CORRECT. It checks that UB tooling is
# wired before unsafe exists. Miri/ASAN/TSAN are what judge correctness; see the `memory-safety`
# skill, which owns those commands.
set -u
# `UNSAFE_GATE_ROOT` exists so the check can be exercised against a tree other than the one holding
# the script. Without it a sabotage run from outside the repo silently scans the WRONG TREE and
# reports a clean pass — which is exactly what happened the first time this was verified.
REPO_ROOT=${UNSAFE_GATE_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}
cd "$REPO_ROOT" || exit 2

# Real `unsafe` CONSTRUCTS, not the word. A hand count of this repo once read "8 unsafe" when the
# true count was 0: the other 8 were the STRING, e.g. "unsafe trust store permissions" in a log
# message. Prose counted as code — the #1192 shape. Comments are stripped before matching for the
# same reason.
# Shapes taken from Rust's grammar, not from ones that came to mind: `unsafe` introduces a BLOCK,
# `fn`, `impl`, `trait`, or `extern`. There is no fifth form.
#
# The leading boundary is load-bearing and was found by testing, not by reading: without it
# `if is_unsafe { … }` MATCHES, because the identifier ends in the keyword. `[^A-Za-z0-9_]` makes
# `unsafe` a word rather than a suffix.
#
# KNOWN LIMIT, stated rather than papered over: this is line-based, so `unsafe` and `{` split across
# two lines would be missed. That is safe HERE and only here, because `gate.sh` runs
# `cargo fmt --check` BEFORE this step and rustfmt collapses `unsafe\n{` to `unsafe {` (verified).
# Lift this check out of that ordering and the assumption goes with it.
#
# Two spellings of one pattern: grep -E wants `\{`, awk wants `[{]` (it warns on the escape).
CONSTRUCT='(^|[^A-Za-z0-9_])unsafe[[:space:]]*(\{|fn |impl |trait |extern )'
CONSTRUCT_AWK='(^|[^A-Za-z0-9_])unsafe[[:space:]]*([{]|fn |impl |trait |extern )'

scan_unsafe() {  # $1.. = roots; prints "path:line: text" for each real construct
    grep -rnE "$CONSTRUCT" --include='*.rs' "$@" 2>/dev/null |
        awk -v pat="$CONSTRUCT_AWK" '
            { split($0, a, ":"); path=a[1]; ln=a[2];
              body=$0; sub(/^[^:]*:[^:]*:/, "", body);
              sub(/\/\/.*/, "", body);                    # strip a line comment
              gsub(/"[^"]*"/, "", body);                  # strip string literals
              if (body ~ pat) print path ":" ln ":" body }'
}

if [ "${1:-}" = "--self-test" ]; then
    # Exercises the WHOLE script against synthetic trees, not just its scan function — the committed
    # sabotage, so it re-runs forever instead of being a thing someone once did by hand. All three
    # outcomes are covered: honest skip, armed failure, and the unsafe-with-Miri pass.
    self="$0"
    tmp=$(mktemp -d) || exit 2
    trap 'rm -rf "$tmp"' EXIT
    mkdir -p "$tmp/crates/probe/src" "$tmp/scripts"

    run_on() { UNSAFE_GATE_ROOT="$tmp" bash "$self" > "$tmp/out" 2>&1; echo $?; }

    # 1. KNOWN-PASS: a tree with no unsafe must skip honestly.
    printf 'fn a() {}\n' > "$tmp/crates/probe/src/clean.rs"
    rc=$(run_on)
    if [ "$rc" -ne 0 ] || ! grep -q "0 unsafe constructs" "$tmp/out"; then
        echo "SELF-TEST: FAIL — a clean tree did not skip honestly (rc=$rc)"; cat "$tmp/out"; exit 1
    fi

    # 2. KNOWN-PASS: the WORD in a comment and in a string must not count. Both shapes are live in
    #    this repo, and a naive count of them once reported "8 unsafe" where the truth was 0.
    printf '// unsafe { } in a comment\nfn b() {}\n' > "$tmp/crates/probe/src/comment.rs"
    printf 'fn c() { let _ = "unsafe { } in a string"; }\n' > "$tmp/crates/probe/src/string.rs"
    rc=$(run_on)
    if [ "$rc" -ne 0 ]; then
        echo "SELF-TEST: FAIL — a comment or string containing the word was counted as code"
        cat "$tmp/out"; exit 1
    fi

    # 3. KNOWN-FAIL: a real unsafe construct with no Miri wired must FAIL and name the file.
    # An identifier ENDING in the keyword must not count — `if is_unsafe {` matched before the
    #    boundary was added, which would have made the tripwire cry wolf on ordinary code.
    printf 'fn e(x: bool) { if x { } }\nfn f(is_unsafe: bool) { if is_unsafe { } }\n' \
        > "$tmp/crates/probe/src/suffix.rs"
    rc=$(run_on)
    if [ "$rc" -ne 0 ]; then
        echo "SELF-TEST: FAIL — an identifier ending in 'unsafe' was counted as an unsafe construct"
        cat "$tmp/out"; exit 1
    fi

    printf 'fn d() { unsafe { } }\n' > "$tmp/crates/probe/src/bad.rs"
    rc=$(run_on)
    if [ "$rc" -eq 0 ] || ! grep -q "bad.rs" "$tmp/out"; then
        echo "SELF-TEST: FAIL — real unsafe with no Miri step did not fail, or did not name the file"
        cat "$tmp/out"; exit 1
    fi

    # 4. KNOWN-PASS: the same unsafe, WITH a Miri step wired, must pass — or the check would be a
    #    blanket ban on unsafe rather than a requirement that UB tooling accompany it.
    printf '#!/bin/sh\ncargo +nightly miri test\n' > "$tmp/scripts/ub.sh"
    rc=$(run_on)
    if [ "$rc" -ne 0 ]; then
        echo "SELF-TEST: FAIL — unsafe WITH a Miri step was still rejected (this is a tripwire, not a ban)"
        cat "$tmp/out"; exit 1
    fi

    echo "SELF-TEST: PASS — clean skips; comment/string/suffix ignored; unsafe without Miri FAILS naming the file; unsafe with Miri passes"
    exit 0
fi

ROOTS="crates plugins apps tools pki-tooling"
hits=$(scan_unsafe $ROOTS | grep -v "/tests/" || true)
count=$(printf '%s' "$hits" | grep -c . || true)

# Is a UB check wired anywhere? Kept deliberately broad: any miri invocation in the gate, a script,
# or a workflow counts. The point is that SOMETHING runs it, not where.
if grep -rqE '(cargo )?miri' scripts/ .github/workflows/ 2>/dev/null; then
    miri_wired=yes
else
    miri_wired=no
fi

if [ "$count" -eq 0 ]; then
    echo "unsafe check: 0 unsafe constructs in production source — Miri step honestly skipped."
    echo "UNSAFE-GATE: PASS"
    exit 0
fi

if [ "$miri_wired" = yes ]; then
    echo "unsafe check: $count unsafe construct(s), and a Miri step is wired."
    echo "UNSAFE-GATE: PASS"
    exit 0
fi

echo "unsafe check: $count unsafe construct(s) and NO Miri step is wired:"
printf '%s\n' "$hits" | sed 's/^/  /'
cat <<'MSG'

UNSAFE-GATE: FAIL — `unsafe` has entered the workspace without a UB check.

  Add a Miri step before this lands. Miri needs nightly, which needs rustup:
      rustup toolchain install nightly && rustup +nightly component add miri
      cargo +nightly miri test -p <the crate that gained unsafe>

  Then wire it (a scheduled job or this gate) so the check runs, not just exists.
  See the `memory-safety` skill for what Miri does and does not prove.

  If the `unsafe` is not wanted, `#![forbid(unsafe_code)]` on that crate is the
  stronger answer: the compiler refuses it outright, with no gate to maintain.
MSG
exit 1
