---
project: openpulsehf
doc: docs/dev/reviews/review-1371-capability-ownership.md
status: review
last_updated: 2026-09-18
---

# Review — capability ownership curation (#1371)

## Consumer

Three production consumers read `capabilities[*].code`, and they want different properties — which
is the tension this curation sits on:

- `scripts/lib/trace.py:do_scope` (the `scope` command) — expands `code:` globs into the file list
  that `scripts/req-mutation.sh:116` turns into `cargo mutants -f` arguments. **This is the consumer
  that was silently failing**: a file owned by no capability is a file no mutation run can reach.
  Wants COMPLETE coverage.
- `scripts/lib/trace.py:751` (`pkgs_for` / the dormancy join) — resolves each binding's owning
  package to decide `DORMANT-ENFORCED` / `UNWIRED-BUT-REACHED`. Wants complete and crate-accurate.
- `scripts/check-trailer.sh` — **not yet a consumer of `code:` on `main`**; #1371's relevance check
  is parked on `fix/1371-trailer-relevance` (`267f95b0`, unpushed). Wants NARROW, precise ownership.

Completeness and precision pull in opposite directions, so no curation satisfies all three.

## Prior art

- `gh issue view 1371` — maintainer decision 2026-09-15 chose "curate the ownership data first, then
  enforce file-level overlap", with the order `curate → A/B replay → enforce`.
- Maintainer decision 2026-09-17 (this session) chose to extend capabilities into their consumers,
  over keeping them crate-shaped.
- `git log -S'CAP-45' -- docs/dev/project/requirements.yaml` → only `1da27abd`, the substrate
  commit: CAP-45's `code:` list has never been edited since it was created, so its omission of the
  daemon is original, not drift.
- The zero-mutant exemption has no prior art in this repo that I could find; it is new here.

## Twins

- **`tests:` is the untouched twin of `code:`.** Same schema position, same staleness risk, and it is
  the reason the rule scores 5/8 below — the three commits it misses touch only `tests/`. Deliberately
  NOT curated in this change; flagged rather than fixed, because doing both at once would make the
  A/B replay unattributable.
- `scripts/reachability.sh`'s baseline (`docs/dev/project/reachability-baseline.txt`) is a second
  hand-maintained ownership-ish map over the same tree. Checked: it did not need updating (ratchet
  passes), but it drifts by the same mechanism.
- The `satisfies:` edges were NOT touched. Adding `code:` paths changes which files a requirement
  mutates without changing which requirements exist.

## Prompt

Test these rather than confirm them, and flag anything wrong or unproven — especially anywhere I
curated until a number improved, which I caught myself doing once already (below).

### Claim 1 — the zero-mutant exemption

I assigned every unowned file that carries behaviour and left 12 unowned because
`cargo mutants --list -f <file>` reports **zero** mutants for them. The argument: owning an inert
file adds nothing to mutation scope, the dormancy join, or a future lint, while diluting the map.

Is that sound, or does it smuggle in an assumption? Specifically: `cargo mutants --list` reports what
*this version of cargo-mutants* can mutate. A file with zero mutants today could gain them when the
tool adds an operator, and nothing would notice the file is unowned. Is "zero mutants" a property of
the file or of the tool — and if the tool, is the exemption a latent gap?

Counter-evidence I did collect: naming would have been worse. 14 files are `lib.rs`/`error.rs`/
`mod.rs`/`main.rs` but only 12 are inert — `filexfer/src/lib.rs` (7 mutants),
`dict-trainer/src/main.rs` (16) and `testmatrix/runners/mod.rs` (2) carry behaviour.

### Claim 2 — the consumer extension, and the case where I over-reached

The 2026-09-17 decision says extend capabilities into consumers. Two attempts were wrong:

1. Extend to every crate importing the root crate → CAP-38 "Modem engine" gains **45** files, i.e.
   the workspace. Rejected as obviously unusable.
2. Restrict to *feature* (not infrastructure) capabilities → CAP-59 gains all **19** importers of
   `openpulse-radio`. This looked principled and was not: it turned `68a033ba` and `44e3de97` —
   both ARDOP/KISS **receive** fixes labelled with the PTT capability — from FAIL into PASS, because
   `bridge.rs` imports the radio crate for keying. I noticed only because I asked whether I was
   curating until the numbers improved.

Final: CAP-59 owns `crates/openpulse-radio/*` plus three files whose subject IS radio control —
`daemon/ptt.rs`, `cli/radio.rs`, `cli/commands/calibrate.rs`. The last was verified by reading it:
it imports `openpulse_radio::{SharedPtt, UnkeyOutcome, DEFAULT_PTT_MAX}`, calls
`build_ptt_controller`, and keys the rig (24 radio references, more than `radio.rs` itself).

**What I want tested:** is "files whose subject IS the capability" a rule, or a post-hoc rationalisation
of where I chose to stop? I cannot state a mechanical test that separates `cli/commands/calibrate.rs`
(kept) from `cli/commands/transmit.rs` (dropped) — both key a transmitter. If there is no such test,
say so, because then the boundary is judgement and should be recorded as such rather than implied to
be derived.

### Claim 3 — the A/B replay and what it proves

Replayed the parked rule over the 32 trailer-carrying commits in the last 400, old map vs new,
using the branch's own `owns()` logic rather than a reimplementation (I initially used my own and it
disagreed; the branch's prefix-matching is more permissive):

```
 11  PASS -> PASS      2  changed: 657b09a2 FAIL->PASS, ad2f2ccd FAIL->PASS
 10  SKIP -> SKIP      1  changed: 73333c98 SKIP->PASS
  8  FAIL -> FAIL
```

All three changes are false positives repaired, and each passes for a verified reason (657b09a2 via
`daemon/lib.rs`, the consumer extension; ad2f2ccd via `shared_ptt.rs`; 73333c98 via `calibrate.rs`).

**Against the issue's eight adjudicated commits the rule scores 5/8.** The three misses are its own
headline cases — `a7412113`, `54418b25`, `b8e34294`, the #1062 probe commits labelled CAP-33 when
CAP-76 was right. All three touch **only** `tests/` files, so no capability's `code:` owns anything
and the escape hatch stands down. Neither probe file is listed in any capability's `tests:` either.

I conclude: **a `code:`-only relevance rule cannot catch test-only mislabels by construction**, and
therefore recommend NOT enforcing the lint. Test that conclusion — in particular whether curating
`tests:` would actually fix it, or whether test-only commits are unattributable for a deeper reason.

### Claim 4 — the headline number

"2 960 mutants were invisible to `req-mutation.sh`" comes from summing `cargo mutants --list -f`
over the 79 previously-unowned files. Check the arithmetic and the framing: those mutants were
invisible to the *requirement-scoped* gate, not to a whole-repo mutation run, and no whole-repo run
is scheduled here. Is "invisible" overstated?

## Verdict

PENDING — not yet sent.
