---
project: openpulsehf
doc: docs/dev/reviews/review-1380-feature-rot-guard.md
status: resolved
last_updated: 2026-09-20
---

# Design review — the #1380 feature-rot guard

## Prompt

Fable was asked to **falsify** a design before implementation. The design: replace #1380's option 1
(`cargo check --features cpal-backend`) with one complete rule in `gate.sh` —
`cargo clippy --workspace --all-features --all-targets -- -D warnings` — on the reasoning that a
hand-maintained list of crate+feature pairs is the same rotting-mirror archetype the guard exists to
prevent. Seven numbered attack points went out with the feature census, the two live defects the
command found, and the claim that the three gate passes form a 2×2 grid. It was asked specifically
whether `--all-features` hides what a per-feature matrix would catch, whether ci.yml's GPU job should
be removed as subsumed, and what `--all-features` would newly fail that a warm local build had not hit.

## Verdict

**Build with changes — seven of them, all applied.** The rule is right; the design was under-scoped.

**The finding that would have broken `main`.** `--all-features` adds 22 packages, among them
`alsa-sys`, `libudev-sys` and `libdbus-sys`, whose `build.rs` each call `pkg_config` and panic when a
`.pc` file is absent. The ubuntu-24.04 runner image ships **none** of `libasound2-dev`, `libudev-dev`,
`libdbus-1-dev` (`pkg-config` itself is present — that was the filter control). Neither
`pr-hook-long-runner` nor `post-merge-gate.yml` installs anything. My clean `rc=0` described this host
**after a root system update stamped those three `.pc` files on 2026-09-19 14:16** —
`traceability.md:1030` records `libdbus-sys` failing at `pkg-config` on this very machine four days
earlier. Unmodified, the post-merge gate would have gone red on arrival and auto-opened an issue: the
#1074 red-on-arrival archetype.

**The other six.**

1. **A per-feature matrix buys nothing** — `git grep` finds no `cfg(all(feature = A, not(feature = B)))`
   anywhere, and none of the 23 `cfg(not(feature))` arms nests inside another feature's region, so
   every arm is reached by some pass. A powerset over 17 features would compile 2¹⁷ configs to find
   zero defects. Noted honestly: that is a census, not a construction.
2. **My 2×2 grid claim is wrong** and would mislead the next reader. The axes are not independent:
   under `--all-features`, `instruments` is on regardless of `--all-targets`, so the fourth cell is
   not "the shipped feature-on configuration". They are three named configurations with a residual,
   not a grid.
3. **Stated residual, off the grid entirely.** The shipped hardware recipe is `{cpal, gpu}` with
   `instruments` OFF. An instruments-only accessor called from a `cpal-backend`-gated production path
   passes all three passes and fails only `cargo build --release -p openpulse-cli --features
   cpal-backend`. Zero instances today; recorded as a limit, not a claim of completeness.
4. **`gpio.rs` was a live latent defect**, not merely a portability nicety: `gpiocdev` is declared
   under `[target.'cfg(target_os = "linux")']` while the code was gated on `feature = "gpio"` alone.
   Cargo enables a feature whose optional dep is target-filtered out, so `--all-features` on macOS
   compiles that code with no crate → E0433. Retargeted to `all(target_os = "linux", feature = "gpio")`
   at all five sites. Not verified on darwin here (no darwin std on this host) — stated as such.
5. **Compiling is not running.** `LinkParams` gained `cessb_enabled` in `b883b5f8` and `notch` in
   `26696f98` (both 2026-06); the test was last touched 2026-06-21. The `serve` feature's only tests
   had been uncompilable, therefore **never run, for ~3 months**. Now run: 2 passed, 0 failed.
6. **Remove the GPU job, but only together with the apt step**, and the `--all-features` pass covers
   its third step too, since `hardware-tests` is a feature and `tests/kernel_equivalence.rs` is
   `#![cfg(feature = "hardware-tests")]`.

**Evidence honesty, Fable's point and worth repeating:** both live defects are in `openpulse-linksim`,
a crate under a 2026-09-10 direction to be replaced, and the yield on shipped cpal/gpu/serial paths in
this run was **zero**. The justification is the class — three months undetected, PR #424 precedent —
not today's haul.

**Sabotage, #1380's own probe.** A planted type error in the cpal-gated `run_drive`
(`calibrate.rs:320`): pass 1 `rc=0`, pass 2 `rc=0`, pass 3 **`rc=101`**. The preflight was separately
sabotaged with a nonexistent library name and correctly printed
`SKIPPED (missing pkg-config: zz-nonexistent-lib)` plus the package names, and failed.

## Consumer

`scripts/gate.sh:236` (the new `run_step`) and `.cargo-husky/hooks/pre-push:102`. In CI the consumers
are `ci.yml`'s `pr-hook-long-runner` and `post-merge-gate.yml`, both of which now install the three
`-dev` packages. The property's downstream consumer is every feature-gated production path, of which
the on-air recipe `cargo build --release -p openpulse-cli --features cpal-backend` is the one that
ships.

## Prior art

`grep -n "features" .github/workflows/ci.yml` → `gpu-feature-gates` (ci.yml:191-236), the existing rot
guard for one feature, with PR #424 as its recorded precedent; this change generalises it and deletes
it. `grep -n clippy scripts/gate.sh` → the two existing passes (the second added by #1418 the day
before). No other mechanism compiles feature-gated code: `git grep -n 'all-features'` over the repo
returned **zero** matches on `origin/main` (control, same command: `no-default-features` matches 105 files).

## Twins

The pre-push hook is the gate's twin and gets the same pass and the same preflight — it is the only
check that runs on every push (#1144), and rot is a per-push class. The two CI jobs that invoke
`gate.sh` are twins of each other and both got the apt step; `ci.yml`'s aloop job already installed
`libasound2-dev` and was left alone. The doc twins asserting the old state were swept: `docs/features.md`,
`docs/openpulse-book.md`, CLAUDE.md's acceptance row naming the `gpu` job, and CLAUDE.md's
"`KeychainStore` body is still never type-checked … #1380 is contained, not closed", which this change
makes false on Linux. #1418 is the inverse sibling — that pass narrows targets, this one widens
features — and neither closes the other.
