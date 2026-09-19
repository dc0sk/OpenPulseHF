---
project: openpulsehf
doc: docs/dev/reviews/review-1418-shipped-config-lint.md
status: resolved
last_updated: 2026-09-19
---

# Review — #1418, linting the shipped (feature-off) configuration

## Prompt

Fable was asked to **falsify**, not confirm, a finding and its write-up before either entered an
issue. The framing sent was: "the gate's own `--all-targets` turns the `instruments` feature ON, so
the library is never linted in the configuration downstream crates and shipped binaries link." It was
given the full apparatus (five clippy invocations with their warning counts), the draft issue text,
and six numbered attack points — the sharpest being #4, *"is 'never linted in the configuration
downstream crates link' actually true? Downstream workspace crates build openpulse-modem as a
dependency during the gate run — does that build see instruments on or off? If off, my headline claim
is wrong. Test this hard."* House rules on pipelines-never-carry-verdicts, positive controls for
grep-derived absences, and no build dirs in the worktree were included.

## Verdict

**Finding upheld, three corrections, and a re-classification from cosmetic to build-break.**

1. **Mechanism confirmed but mis-attributed.** I blamed the #1277 self dev-dep alone. There are
   **three** sites enabling `instruments`: `crates/openpulse-modem/Cargo.toml:41`,
   `crates/openpulse-daemon/Cargo.toml:92`, `crates/openpulse-kiss/Cargo.toml:59`. Under `--workspace`
   any one suffices, so removing the self dev-dep would not have closed it. Verified independently by
   grep over all `Cargo.toml`.
2. **`--all-targets` is a proxy, not the trigger.** The trigger is *dev units in scope*; `--tests`
   alone flips it, `--bins`/`--lib` do not. Resolved per-unit from cargo's `--message-format=json`
   `compiler-artifact` records — `cargo tree -e features` cannot discriminate, as it prints the dev
   edge either way.
3. **Point 4 was half-wrong, and the correction matters.** During the gate run there is exactly ONE
   non-test `openpulse_modem` lib unit, features `['instruments']`, and all 14 dependents link it — so
   in the gate, downstream crates link the **ON** lib. It is the **shipped binaries** (`release.yml:60`)
   that link OFF. "Never linted" survives; "downstream crates link OFF" does not, and the issue text
   was changed accordingly. Also corrected: OFF *is* type-checked (macOS build, `cross check`,
   `release.yml`) — never **linted**. Do not let "never linted" harden into "never compiled".
4. **Severity raised.** I had filed it as two cosmetic warnings. A planted **ungated production
   caller** of an instruments-only accessor is invisible to both automated checks and breaks the
   release build — re-derived here rather than taken on trust:

   ```
   cargo clippy --workspace --no-default-features --all-targets -- -D warnings  -> rc=0    (gate)
   cargo test  -p openpulse-modem --no-default-features --no-run                -> rc=0    (hook)
   cargo clippy -p openpulse-daemon --no-default-features -- -D warnings        -> rc=101  E0599
   ```

   Corollary: #1277's "a production call cannot compile without a visible `Cargo.toml` diff" is
   enforced by **release builds alone**.
5. **Fix shape cleared, with two constraints.** Must be an **added** pass (dropping `--all-targets`
   stops linting test code — the `session_key.rs` rot), and must be **`--workspace`**: narrowing to
   `-p openpulse-modem --lib` would miss a downstream crate's production code calling an instruments
   item, since that crate's lib builds against the ON modem. Runtime 9.1 s cold / ~1 s warm.
6. **Bottom line: file separately, not as a comment on #1380** — #1380 closes with a
   `--features cpal-backend` PR that would leave this buried under a closed issue, and the fix is a
   different gate step. Maintainer agreed; filed as #1418 and the #1380 comment reduced to a
   cross-reference.

Four line numbers in my draft were off by ~1; Fable's were right and were used.

## Consumer

`scripts/gate.sh:192` (the new `run_step`) and `.cargo-husky/hooks/pre-push:76`. Both are the direct
production callers — this change *is* verification machinery, so its consumer is the gate itself. The
downstream consumer of the property is `.github/workflows/release.yml:60`
(`cargo build --release … --no-default-features`), the build whose configuration went unlinted.

## Prior art

`grep -n "clippy" scripts/gate.sh .cargo-husky/hooks/pre-push` → one invocation each, both
`--all-targets`, no feature-off pass anywhere. `grep -RniE "all-targets|dev-depend" skills/` over the
skills tree (with a known-present control phrase to prove the filter fires) → no existing rule. #1380
is the nearest existing mechanism and is the **inverse** case (feature off, code unchecked); its
proposed `cargo check --features cpal-backend` does not reach this.

## Twins

The hook is the twin of the gate and got the same pass — it is the only check that runs on every push
(#1144), and it was measured passing the planted E0599. The other candidate twins were checked and are
**not** affected: an awk census over every tracked `Cargo.toml`, scoped to `[dev-dependencies]`
sections enabling features on a workspace sibling, returns exactly the three `instruments` sites and
nothing else — so no other crate has this shape. `ci.yml`'s release-scoped jobs already compile OFF. `openpulse-cli`'s `cpal-backend` is the #1380 case, tracked
separately.
