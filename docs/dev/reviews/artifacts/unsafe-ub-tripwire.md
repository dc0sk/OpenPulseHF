---
project: openpulsehf
doc: docs/dev/reviews/artifacts/unsafe-ub-tripwire.md
status: review
last_updated: 2026-09-16
---

# Decision record — where Miri belongs, and why the gate gets a tripwire instead

Design-class: adds a new `scripts/**` file and a `gate.sh` step.

## Consumer

- `scripts/gate.sh` — the new `run_step "unsafe/UB tripwire"`, beside the other cheap no-build steps
  (`doc frontmatter`, `ledger ordering`, `re-homed docs lint`).
- `scripts/check-unsafe.sh` itself, and `--self-test`, which the gate's own sabotage discipline
  requires for a checker.
- Nothing in production reads it; it is a gate step, not a library.

## Prior art

- **The `code-quality-gates` skill already states the rule and its exit condition:** *"A crate with
  `unsafe` gets a Miri … job in this same gate … a workspace with none of them skips the step
  honestly."* This is that honest skip, made self-arming. **No skill edit** — the rule exists, is
  correct, and covers the empty case; adding text would dilute a sharp rule.
- **`memory-safety`** owns what Miri proves and the SAFETY-comment discipline; the failure message
  points there rather than restating it.
- `scripts/check-rehomed-docs.sh`, `scripts/check-trailer.sh` — the shape copied: a cheap
  no-build gate step that ships a `--self-test`.
- **Measured, not assumed:** `grep -rnE 'unsafe (\{|fn |impl |trait )'` over `crates plugins apps
  tools pki-tooling` returns **0**. An earlier count of the *word* returned 8 — all string literals
  such as `"unsafe trust store permissions"`. Prose counted as code, the #1192 shape, in the very
  census that was supposed to justify this work.
- `rustup` is absent on this host, so there is no nightly and `cargo miri` cannot run at all.

## Twins

- **The other cpal-gated crates** (`openpulse-audio`, `-ardop`, `-kiss`, `openpulse-testbench`)
  share the type-checking gap filed as #1380 — a different hole, same cause (`--no-default-features`
  everywhere), and NOT closed by this change.
- **`#![forbid(unsafe_code)]`** is the compiler-enforced alternative: strictly stronger where the
  answer is "no unsafe, ever", needing no gate at all. It is named in the failure message rather
  than imposed, because forbidding and gating are different policies and the choice is the
  maintainer's per crate.
- `scripts/req-mutation.sh` is the sibling pattern for "a check that should exist but must not lie
  when it cannot run" — its two fail-opens were repaired the same day (#1279).

## Prompt

Maintainer, 2026-09-15: *"in which place should we integrate code checking with miri? On one of the
project's hooks or in a skill?"* — then, on the finding below, *"yes, add it"*.

## Verdict

**Neither a hook nor a skill change. A self-arming gate step.**

- **Not a hook.** Miri in pre-push would be a gate that cannot fail (zero `unsafe`), which this repo
  treats as a defect in its own right — and it is unrunnable here without rustup. It is also slow;
  the hook's whole design is to stay fast enough that nobody reaches for `--no-verify`.
- **Not a skill change.** The rule is already written and already handles the empty workspace.
- **A tripwire, because the risk is temporal.** Today's answer ("no unsafe, so no Miri") is correct
  and will stay correct until the day it silently is not. The check passes while there is no
  `unsafe`, and fails the moment any appears without a Miri step wired — converting a future
  obligation into a present, mechanical one.

**It is a tripwire, not a ban:** `unsafe` accompanied by a wired Miri step passes. That distinction
is pinned by a self-test case, or the check would quietly be a prohibition wearing a gate's clothes.

### What the self-test covers, and why each case exists

| case | asserts | why |
|---|---|---|
| clean tree | rc=0, "0 unsafe" | the honest-skip path |
| the word in a **comment** | ignored | live in this repo |
| the word in a **string** | ignored | `"unsafe trust store permissions"` — the shape that made a hand count read 8 |
| identifier **ending** in the keyword | ignored | `if is_unsafe {` **matched** before a word boundary was added — found by testing, not reading |
| real `unsafe`, no Miri | **FAILS**, names the file | the armed path |
| real `unsafe`, Miri wired | passes | proves tripwire ≠ ban |

### Stated limits

- **Line-based**, so `unsafe` and `{` split across lines would be missed. Safe **only** because
  `gate.sh` runs `cargo fmt --check` before this step and rustfmt collapses `unsafe\n{` to
  `unsafe {` — verified, not assumed. Moving the check out of that ordering invalidates it.
- **It does not check that `unsafe` is correct.** It checks that UB tooling is wired before `unsafe`
  exists. Miri/ASAN/TSAN judge correctness.
- **Verification note:** the first sabotage run reported PASS with a real `unsafe` block planted —
  because the script was executed from the scratchpad and derives its root from its own location, so
  it scanned the wrong tree. `UNSAFE_GATE_ROOT` now exists so a check can be pointed at another
  tree, with that reason recorded inline. A sabotage that does not reach its subject is
  indistinguishable from a passing check.
