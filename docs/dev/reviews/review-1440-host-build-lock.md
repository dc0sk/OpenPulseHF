---
project: openpulsehf
doc: docs/dev/reviews/review-1440-host-build-lock.md
status: resolved
last_updated: 2026-09-27
---

# Adversarial review — #1440: a host-wide lock for heavy builds

One round by Fable, read-only, prompted for falsification, on commit 738ef528 (the PR rebased onto
main ed8ea647). A full gate had already passed on that commit (`GATE: PASS 738ef528`, 2596 tests).
Outcome below the verdict.

## Consumer

- `scripts/gate.sh` — run by hand and by CI (`ci.yml:95`, `post-merge-gate.yml:85`, both
  `ubuntu-latest`). No script or hook calls gate.sh.
- `.cargo-husky/hooks/pre-push` — the SOURCE of the hook; git runs the copy cargo-husky 1.5.0
  installs in `.git/hooks/pre-push`.
- fnec-rust's `scripts/check-all.sh:34` and `.githooks/pre-push:9` source an equivalent helper
  at the same path; that hook runs `cargo test --workspace` directly, never check-all.sh.

## Prior art

None: `grep flock|lock|mutex|semaphore` over `scripts/`, `.github/`, `.cargo-husky/` finds only
this commit and an unrelated `OTA_LOCK` in `scripts/run-twin-station-audio.sh`.

## Twins

Heavy cargo entry points that do not take the lock: `scripts/slow-tests.sh:46` (the ~83 min
held-out suites, including the `ota_channel_adaptation` run this PR names), `scripts/coverage.sh:35`
(`cargo llvm-cov --workspace`), `scripts/req-mutation.sh:184` (`cargo mutants`),
`scripts/run-test-matrix.sh:18`, the demo/deploy release-build scripts, and the bare
`cargo test --workspace` in `CLAUDE.md:24,60`. CI runners are fresh machines, not twins.

## Prompt

"Try to break it": deadlock (can gate.sh and the hook both take the lock in one process tree;
does anything call gate.sh while holding it); fd/flock correctness when the lib is sourced (fd
collisions, release on exit, `set -e`/`set -u`, children inheriting the fd); portability (flock
missing, XDG_RUNTIME_DIR unset, permissions); whether the placement before START_HEAD keeps waiting
out of the drift guard and which modes must be exempt; whether fnec-rust's helper actually
excludes this one. Consumer, prior art and twins by file:line.

## Verdict

**Sound with changes.**

1. HIGH — the hook change cannot take effect: cargo-husky 1.5.0 does not reinstall a hook it
   already installed (`build.rs:79-95,199-201`), and the installed `.git/hooks/pre-push` (Sep 18)
   has no lock — nor #1418's or #1380's changes.
2. MEDIUM — fail-open with a false "lock acquired": when `exec 9>` fails (directory missing, file
   owned by another user) both `flock` calls fail with EBADF and the helper prints "waiting" then
   "lock acquired" and runs unlocked.
3. MEDIUM — the lock path is per environment: `$XDG_RUNTIME_DIR/heavy-build.lock` in a login
   shell, `/tmp/heavy-build.lock` where XDG_RUNTIME_DIR is unset (cron, sudo, some GUI clients).
4. LOW — fd 9 is inherited by every child; a future test that leaves a server running would hold
   the host lock after the gate exits.
5. LOW — `STAMP` is taken before the wait, so the log name records enqueue time. COMMIT/DIRTY and
   START_TREE/START_HEAD are after the lock, so waiting stays outside the drift guard.
   `--fingerprint` is correctly exempt; `--self-test` and `--quick` correctly take the lock.
6. Deadlock: none. Each entry point sources the lib once; neither calls the other; no other fd-9
   use. fnec-rust's helper is equivalent in path and fd semantics, so the projects exclude each
   other (subject to 3).
7. INFO — the twins above should source the helper.

## Outcome

- 2: fixed in this PR — the helper refuses (exit 1) when the lock file cannot be opened or the
  lock cannot be taken; a host without flock still runs unlocked and says so. Checked: a missing
  lock directory exits 1 with the reason. The same fix was applied to fnec-rust's helper.
- 1: the maintainer's installed hook was refreshed from the versioned one; the drift check is
  #1448.
- 3: kept by the maintainer's decision — the XDG path is the one the maintainer's global build
  rule prescribes for manual `flock` runs, and no gate runs from cron or sudo today. Recorded here.
- 4, 5: accepted as low; not changed.
- 7: #1449.
