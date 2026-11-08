---
project: openpulsehf
doc: docs/dev/reviews/review-gate-nextest-2026-11-08.md
status: resolved
last_updated: 2026-11-08
---

# Review: the gate's workspace tests under cargo-nextest (2026-11-08)

Review by Fable of branch `ci/gate-nextest` at `903f3bc7`, because the change replaces the runner
behind every `GATE:` verdict and adds three new scripts and a config that decide what the gate
counts. One round; the disposition of each finding is at the end.

## Prompt

The reviewer was given the diff, the gate's contract (CLAUDE.md verification mechanics,
`docs/dev/verification-mechanics.md`) and the measurements to that point (baseline set, nextest-only
run, gate runs 2–4, the sabotages), read-only, and asked to falsify:
- **coverage**: can nextest run fewer tests than `cargo test` without the parity step noticing
  (binaries the `--no-run` JSON parse misses, the `profile.test` filter, examples/benches, list-time
  cwd/env, doctests, `#[ignore]`, filter-match reasons);
- **counting**: can `count_field` or the JUnit parse give a wrong `tests_failed`/`tests_passed`/
  `suites`, or an incomplete failure list, in any failure mode (build failure, stale or missing
  report, crash, timeout, leak);
- **evidence**: can trace.py now accept a test as passed that did not pass, or refuse one that did;
- **isolation**: other per-process/per-binary state, fixed ports, paths, env or cwd the
  process-per-test model breaks; check the 19xxx port inventory;
- **wall-clock tests**: whether run 2/run 3's failures are a cross-session port collision or CPU
  contention, and what experiment separates them;
- **the thread bound**, **CI pinning** and the JUnit path in CI, **false statements** in comments
  and docs;
- **"Does this block Release 1? If not, say so."**

## Verdict

**APPROVE WITH CHANGES.** Does not block Release 1: the gate is apparatus, and nothing on the M3/M4
path depends on its runner; a cheaper gate serves decision 9.

Findings (severity; disposition):
1. A `<skipped>` JUnit testcase rendered as `ok` — latent fail-open into trace.py's evidence if
   `junit.report-skipped` is ever enabled (MEDIUM; **applied**: refused with exit 2, probe G2).
2. `COUNTED_STEPS` hand-copies two step names; a rename gives `GATE: PASS` on zero tests, and the
   gate (unlike the self-test) never checks the run's total against the selection (MEDIUM;
   **applied**: names defined once, the report step requires the report to hold exactly nextest's
   selection, and a full run counting no tests cannot pass).
3. `.config/nextest.toml` claimed the thread count was "chosen from a measured peak" in a ledger
   entry that did not exist (MEDIUM; **applied**: entry written, comment says what was measured).
4. `docs/dev/release-checklist.md` still named `cargo test` as the gate's command (LOW; **applied**).
5. Run 2's `id_rides_in_own_keying` failure is not shown to be a collision; contention fits equally
   (MEDIUM; **applied** to the wording; the collision signature of run 3 was then reproduced with a
   planted listener; `threads-required = 4` added for daemon integration tests; five quiet-host
   gates run, all PASS).
6. 16 threads is not a measured bound (MEDIUM; **partly applied**: summed test-process RSS measured
   at 264–285 MB; no cgroup cap added — parked).
7. Parity compares selections, not executions (LOW; closed by finding 2's count check).
8–11. Counting edge cases, evidence channel, isolation inventory, CI pinning: checked, sound.
12. Pre-existing: `post-merge-gate.yml` reads `GATE:` and the failure block from the log, but gate.sh
   prints both to stdout only (parked).

Recommended and not done: `spawn_pair` (`crates/openpulse-daemon/src/twin.rs`) failing fast when a
daemon cannot bind, instead of `wait_for_port`'s warn-and-continue — it would turn a 300 s timeout
into a 4 s named EADDRINUSE. It changes test-harness code, so it is left for its own change.

## Consumer

`scripts/lib/trace.py` (`_evidence_log`, `_passed_tests`: reads `target/gate-verdict.json`'s
`result`, `toolchain`, `commit`, `log`, and the log's `test … ok` / `=== end` lines; the new
`nextest` field is read by nothing); `.github/workflows/ci.yml` and `post-merge-gate.yml` (artifact
upload; the issue body's log read, finding 12); traceability-ledger prose quoting `suites`/`passed`
(the `suites` meaning changed, 398 → 350, recorded in the ledger and verification-mechanics.md).

## Prior art

`docs/dev/reviews/2026-08-30-1224-gate-log-evidence.md` and `trace.py`'s refusal to match a
hand-transcribed step name (finding 2's shape); #1052 and `--no-fail-fast` (the self-test's total
check); #1176 (daemon test port overlap, `twin_daemon_bridge.rs`'s window assert); #1066 (wall-clock
verdicts); #1151 (drift guard, unchanged). No earlier nextest attempt in the tree.

## Twins

`scripts/gate.sh` and its `--self-test` — switched, sharing `run_nextest`. Not switched, by design:
`.cargo-husky/hooks/pre-push` (touched crates, `cargo test`; note it now runs serial binaries where
the gate runs parallel processes), `.github/workflows/release.yml` (`cargo test --workspace`,
pre-existing without `--no-fail-fast`), `scripts/slow-tests.sh` (held-out `--ignored` suites),
`coverage.yml` (cargo-llvm-cov), `scripts/req-mutation.sh`, `scripts/onair-preflight.sh`.
