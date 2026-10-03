---
project: openpulsehf
doc: docs/dev/reviews/2026-10-03-1459-reentrant-build-lock.md
status: review
last_updated: 2026-10-03
---

# Review — re-entrant host-wide build lock (#1459)

The change: `scripts/lib/host-build-lock.sh` continues instead of re-locking when fd 9 already
refers to the lock file and `flock -n 9` succeeds at once (an ancestor holds it). New
`scripts/test-host-build-lock.sh`, run as a `scripts/gate.sh` step, checks nesting, contention and
fail-closed on a scratch lock file. Origin: fnec-rust FND-184 (dc0sk/fnec-rust#496).

## Consumer

`grep -rn "host-build-lock" scripts .cargo-husky .github`:

- `scripts/gate.sh:71`: `source "$REPO_ROOT/scripts/lib/host-build-lock.sh" "gate"`
- `.cargo-husky/hooks/pre-push:55`: `source …/scripts/lib/host-build-lock.sh "pre-push"`
- `scripts/gate.sh:225` (new): `run_step "host-build-lock self-test" bash scripts/test-host-build-lock.sh`

No other caller in this repo. The lock file itself is shared with fnec-rust's copy of the helper.

## Prior art

`grep -rn "flock" scripts .cargo-husky .github`, excluding the helper and its test: no hits. There
is no other locking mechanism in this repo to reuse. The fix is the one already applied to fnec-rust's
helper (FND-184).

## Twins

fnec-rust's `scripts/host-build-lock.sh`: same helper, same lock file. It is not in this session's
checkout, so its current state is UNCHECKED here. The re-entrancy test reads fd 9 and so depends on
the twin also keeping the lock on fd 9.

## Prompt

Sent to Fable, read-only, against the branch merged with `main`:

> Try to FALSIFY the claim: the helper is re-entrant within one process tree, an unrelated process
> still waits, an unopenable path still fails closed, and the self-test proves all three
> (sabotage: main's helper fails the nested case with exit 124). Consider (1) the re-entrant
> branch admitting an unprotected process: fd 9 inherited but not held, siblings sharing one
> description, orphaned children, a description from a fnec-rust shell; (2) whether the self-test
> discriminates; (3) whether the gate step is safe while the gate holds the real lock, and whether
> it can hang the gate; (4) portability (`/proc`, `readlink -f`, `flock` absent). Does this block
> Release 1? If not, say so.

## Verdict

**Sound with caveats.** The re-entrant branch cannot admit a process while a stranger holds the
lock: everything it admits runs under a lock that really is held on fd 9's open file description.

- **Self-test discriminates.** The reviewer ran it with sabotaged copies outside the repo: this
  branch's helper → OK; `main`'s helper → `FAIL nested take: exit 124`; an always-re-entrant stub →
  `FAIL contention` and `FAIL fail-closed`.
- **Fixed in this PR — scratch-dir failure.** With `~/.cache` missing, `mktemp` failed, `SCRATCH`
  was empty, and the cases ran against `/lock`, `/inner.sh` and `/outer.sh`. As root the test still
  printed OK; as non-root it failed the gate spuriously. The test now uses `${TMPDIR:-/tmp}` and
  exits 1 if it cannot create the directory. Checked: `TMPDIR=/nonexistent/x` → exit 1 with
  `FAIL: cannot create a scratch directory`; the normal run → `host-build-lock self-test OK`.
- **Parked: fd 9 open but not locked.** A child then takes the lock on the shared description and
  prints "already holds". Exclusion still holds, but the message is wrong and the lock outlives the
  child. Reachable only if a caller opens fd 9 on the lock without locking it. This helper never
  does that; fnec-rust's copy is unchecked.
- **Parked: siblings run concurrently.** Two background children of a holder both continue. Not
  reachable from `gate.sh` or pre-push today, since neither forks heavy work in parallel. A shell
  that sourced the helper and then runs the gate and `git push` together would now overlap.
- **Not new: an orphaned background child keeps the lock** after its parent exits. That comes from
  fd inheritance, not this change.
- **Gate step is safe** while the gate holds the real lock. The scratch path differs, so children
  open their own description. Cases 1–2 are bounded by `timeout` (≤ 10 s; ≤ 2 s plus a 4 s wait),
  and case 3 fails fast. Case 2 has a 1 s ordering race that could flake on a heavily loaded host.
- **Portability:** without `/proc` (macOS) the helper behaves as before. Stock macOS lacks `flock`
  and `timeout`, but `gate.sh` already assumes Linux and CI's macOS job only builds.

**Does this block Release 1? No.** It is tooling only, and the gate's verdict path is unchanged.
