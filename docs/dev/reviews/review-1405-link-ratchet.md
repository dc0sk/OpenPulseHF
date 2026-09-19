---
project: openpulsehf
doc: docs/dev/reviews/review-1405-link-ratchet.md
status: resolved
last_updated: 2026-09-19
---

# Design review — the #1405 binding-linkability ratchet

## Prompt

Fable was asked to **falsify** a design before implementation, not confirm it. The design: a new
`trace.py` finding `UNREACHABLE-FROM-BINDING` as a **grandfathered ratchet** (baseline today's
unlinkable set, fail only on growth), after the maintainer chose that over the fail-or-warn pair the
issue framed. Seven numbered attack points were sent with the apparatus (15 measured baseline
candidates, the linkability semantics, the `_workspace_graph` contrast). The hardest, stated as such:
*"are my linkability semantics correct, and do they match cargo-mutants `--test-package`? I inferred
this and have NOT verified it — if cargo-mutants builds something else, the metric measures the wrong
thing."* It was also asked to argue the case for closing #1405 instead.

## Verdict

**Build, with seven changes.** All are implemented.

1. **Semantics confirmed against the tool, not assumed.** Fable read cargo-mutants 27.1.0's source:
   `lab.rs` selects `TestsForMutant::Explicit(packages)` — the `--test-package` list only, the mutated
   package is *not* added — and `cargo.rs` issues `cargo test --package=<testpkg>
   --no-default-features`. So linkable = P + P's normal/dev/build deps, then normal/build
   transitively. That is the rule implemented in `_link_graph`. Verified three further ways,
   including a `cargo check --tests --message-format=json` artifact set equal to the `cargo tree` set.
2. **"Exclude optional" was right for the wrong reason.** The real rule is feature resolution rooted
   at P under `--no-default-features`; exclusion is its *consequence* today, holding only because no
   internal dep spec names an optional internal target, plugin defaults are empty, and the daemon's
   default-on `gpu` is suppressed by the flag `req-mutation.sh` passes. Now a checked claim: a new
   `graph-self-test` probe fails when any internal dep spec activates an optional internal edge.
   Sabotage-verified — planting `features = ["gpu"]` on `openpulse-cli -> bpsk-plugin` produces
   `GRAPH-SELF-TEST FAIL: openpulse-cli -> bpsk-plugin activates optional openpulse-gpu`.
3. **Bin targets over-approximate.** A dependent's tests never build a dependency's `bin`. A file
   that is a bin root is linkable only from its own package. Latent rather than live: it matters the
   day CAP-55/CAP-67's `daemon/src/main.rs` goes enforced.
4. **`unwired` belongs in scope.** Excluding it parks REQ-CTL-04's three linksec files outside the
   baseline, all of which would fail on the day #1234 lands and it flips to `enforced` — a ratchet
   that ambushes whoever fixes the blocker. Baseline is 15 entries, not 12.
5. **`baseline − current` must FAIL, not warn** — the #1371 shape, where 74 of 86 orphan entries were
   found stale in one pass because "shrink this over time" was enforced by nobody. Implemented as
   `STALE-LINK-BASELINE`, naming all four causes rather than guessing between them.
6. **File-level key, mutant counts in the report.** Linkability is a *package* property, so file is
   already finer than the truth. But the disparity is real — measured 186 mutants in
   `linksec/async_channel.rs` against 4 in `modem/envelope_codec.rs`, 46× — so NEW findings carry a
   best-effort `cargo mutants --list` count, never a silent 0 (#1279).
7. **Naming.** `reachability.sh` already uses "reachable" for *production* reach; a second sense
   invites the over-read the design exists to prevent. Renamed to `UNLINKABLE-FROM-BINDING`.

**A false premise of mine, caught here.** I claimed #1403 would change these ratios via function-level
`code:` scoping and that this argued for waiting. #1403 is **closed** (8b5896fa, "no map change") and
neither its body nor its comments ever proposed function-level scoping — I asserted it from the issue
body without checking. Dropped from the plan; linkability is a package property regardless.

**On closing instead:** rejected, and the strongest argument is #1415's own history — the person
fixing #1405 created a fresh instance of it (REQ-PTT-04, 82/363 unlinkable) in the same week, caught
only by re-measuring by hand. That is the signature of a defect needing a mechanical check.
`req-mutation.sh` is structurally blind to it: its verdict is `killed > 0`, which is how REQ-FUN-11
passed at 7/408.

**Stated blindnesses**, in the finding text, the baseline header and the code: linkability is
necessary but not sufficient (#1415) — it cannot see a *vacuous* binding in the right package, which
is `req-mutation.sh`'s job — and it cannot see `cfg`-gated code inside a linkable file, which compiles
out and can also only be MISSED.

## Consumer

`scripts/lib/trace.py` `do_check`, which runs in `scripts/gate.sh:203` and per-PR in
`.github/workflows/traceability.yml:45`. The consumer of the *property* is `scripts/req-mutation.sh`,
whose per-requirement verdict this qualifies.

## Prior art

`grep -n "baseline" scripts/lib/trace.py` → the `NEW-ORPHAN` / `STALE-BASELINE` / `DEAD-BASELINE`
trio at lines ~962-985 and `docs/dev/project/{trace-orphan-baseline,reachability-baseline}.txt`; this
check mirrors that shape rather than inventing one. `_workspace_graph()` (trace.py:216) already
resolves the package graph but filters dev and optional edges for *production* reach, so it could not
be reused directly — `_link_graph()` is the test-linkability sibling, and the docstring says why.
Fable independently confirmed no existing mechanism covers this.

## Twins

`req-mutation.sh` is the twin consumer and is deliberately **not** changed: its blindness is a
separate defect (`killed > 0` as a verdict) tracked in the issue, and fixing both at once would make
neither attributable. The `reachability.sh` ratchet is the structural twin and was read for its
baseline discipline. Within this check, the two directions are themselves twins and both are
sabotage-verified — NEW and STALE each fail on their own planted defect, with the unmodified-tree
positive control still passing.
