---
project: openpulsehf
doc: docs/dev/reviews/review-1371-split-and-enforce.md
status: resolved
last_updated: 2026-09-18
---

# Design review — split the JS8 waveform out of CAP-70, then enforce the trailer-relevance lint

Two changes proposed together because the first sharpens the second, and doing them in the other
order means re-litigating trailers that the lint has already blessed.

## Consumer

- **The lint (proposed enforcement):** `scripts/check-trailer.sh`'s `irrelevant_caps()`, parked on
  `fix/1371-trailer-relevance` (`267f95b0`, unpushed). Consumes `capabilities[*].code`.
- **`scripts/lib/trace.py:do_scope`** → `scripts/req-mutation.sh:116` — the requirement-scoped
  mutation gate. Reads the same `code:` lists; a capability's size is its mutation cost.
- **`scripts/lib/trace.py` REQ-GAP / dormancy join** — reads `satisfies:`/`covered_by:`, which the
  split edits. REQ-DISC-01 is covered by CAP-70 **alone** today, so the split must not orphan it.

## Prior art

- `git grep -n 'CAP-1[2-9]:' docs/dev/project/requirements.yaml` — every other waveform already has
  its own capability: CAP-12 BPSK, CAP-13 QPSK, CAP-14 8PSK, CAP-15 64QAM, CAP-16 FSK4-ACK,
  CAP-17 OFDM, CAP-18 SC-FDMA, CAP-19 Pilot, CAP-75 MFSK16. JS8 is the sole exception, and only
  because #1371's curation put all 17 plugin files under the discovery capability yesterday.
- Enforcement mechanism: no new machinery. `is_prod` + `irrelevant_caps()` already exist on the
  parked branch; this proposes one predicate change and a scope decision, not a new checker.
- The A/B replay method is the one the 2026-09-15 maintainer decision prescribed and that
  `review-1371-capability-ownership.md` used.

## Twins

- **The other multi-concern capabilities.** CAP-55 (daemon) and CAP-71 (filexfer) share
  `openpulse-daemon`; CAP-36/38/61/65/76 all share `openpulse-modem`. If "one capability per
  separable concern" is right for JS8, those are the same shape and are NOT being split here —
  stated so the precedent is explicit rather than implied.
- **`tests:` is the untouched twin of `code:`** and stays untouched. Note
  `plugins/js8/tests/snr_estimate.rs` is cited by no capability; `DANGLING-TEST` only checks that a
  cited test exists, never that an existing test is cited, so the gap is invisible.
- **The lint's own trailer.** `check-trailer.sh` changes are `scripts/`, not production code, so
  `is_prod` will skip the lint's own commit — the gate cannot police the change that introduces it.

## Prompt

Test these rather than confirm them. My track record today is poor: two headline claims, a
stop-criterion, a recommendation and my own finding's mechanism were all overturned on measurement.

### A. Split `plugins/js8` out of CAP-70

CAP-70 "JS8-based station discovery + rendezvous (FF-15)" currently owns **29 files** (17 JS8 plugin
+ 10 discovery + `cli/commands/daemon.rs` + `daemon/server.rs`), ~1 763 mutants, and satisfies ten
requirements. So `Refactors: CAP-70` cannot distinguish an LDPC-decoder change from a rendezvous
change — precisely the attribution the lint exists to enforce.

Proposed:

- **new `CAP-79` "JS8 waveform plugin"** — `code: plugins/js8/src/*` (17 files),
  `tests: plugins/js8/tests/{snr_sweep,snr_estimate}.rs`, `satisfies: [REQ-DISC-01]`,
  `traceability: baseline` (matching CAP-70; this is not a claim that it is enforced).
- **CAP-70 keeps** the 10 discovery files + 2 consumer files, its 3 existing `tests:` entries, and
  `satisfies: [REQ-DISC-02…07, REQ-FUN-13, REQ-FUN-14, REQ-REG-04]`.

REQ-DISC-01 is "Native JS8-compatible waveform interoperable with stock JS8Call, no external
process" and is covered by CAP-70 alone, so it moves wholesale to CAP-79.

**What I want tested:** (1) does moving REQ-DISC-01's sole coverage break REQ-GAP, the dormancy
join, or `EMPTY-CAP`? (2) Is REQ-DISC-01 really satisfied by the *plugin alone*, or does the
"interoperable with stock JS8Call" clause depend on the discovery runtime that drives it — in which
case the requirement should be covered by both and I am proposing a false separation. (3) Is this a
split worth making at all, given the twins above are left unsplit; say so if the consistency
argument cuts the other way.

### B. Enforce the relevance lint, failing only when EVERY named capability is irrelevant

Maintainer decision 2026-09-18, after the corpus showed 7 true mislabels and ≤1 false positive in 22
src-touching commits, four of the seven landing after the issue was filed.

The parked `irrelevant_caps()` returns every named id owning none of the touched files, and the
caller fails if that list is non-empty — i.e. **any** irrelevant id fails. Proposed change: fail only
when **all** named ids are irrelevant. Rationale: the observed defect is a single wrong id; the one
false positive in the corpus (`09048b84`, `Refactors: CAP-33 CAP-38`, where CAP-38 was right) is a
multi-id trailer where one id was correct.

**What I want tested:** (1) does "all" weaken it below usefulness — can a mislabel now be laundered
by adding one relevant id alongside the wrong one, and is that a realistic failure mode or a
theoretical one? (2) Re-run the corpus replay under BOTH predicates and report the true/false split
for each; I want the numbers checked, not taken from me. (3) The `SKIP` escape (no capability owns
any touched file) remains — with the map now curated, how reachable is it, and can it be gamed by
touching one of the 11 remaining unowned files? (4) `is_prod` means the lint cannot police its own
introducing commit; is that acceptable or does it want a carve-out?

### C. Ordering

I propose split-then-enforce in one PR. The alternative is enforce first and split later, which
means commits blessed under `CAP-70` become ambiguous retroactively. Is one PR right, or should the
split land alone so the enforcement's A/B replay runs against a stable map?

## Verdict

Reviewed 2026-09-18.

**A — split: ADOPT-WITH-CHANGES.** The premise holds: REQ-DISC-01's full text names the plugin's own
artifacts (8-GFSK, Costas 3x7, LDPC(174,87), CRC-12, "without depending on an external JS8Call
process"), and the plugin's only production consumer is `openpulse-discovery`, so CAP-79 → CAP-70 →
daemon is a live chain. Three defects in my proposal, all fixed here:
1. `traceability: baseline` FAILS — `NOT-GRANDFATHERED`, since `trace-grandfathered-ids.txt` only
   shrinks. CAP-76/77/78, the three capabilities created since the freeze, are all `enforced`.
   CAP-79 is `enforced`.
2. `code: plugins/js8/src/*` breaks BOTH consumers, differently: `trace.py` globs it and so also
   claims `lib.rs`, tripping the new `STALE-BASELINE`; the lint's `owns()` is prefix matching and
   does not glob at all, so the whole plugin would read as unowned and the rule would SKIP. No
   existing `code:` entry uses a glob. The 17 files are listed explicitly.
3. The two test suites are js8→js8 self-loopback and do not evidence the "interoperable with stock
   JS8Call" clause. The JS8Call-validated vectors live in the src test regions of `crc.rs`,
   `encode.rs` and `frame.rs`; those are cited too, so `EMPTY-CAP` clears on the right evidence.

The twins (CAP-55/71 sharing the daemon, CAP-36/38/61/65/76 sharing the modem) are **not**
disqualifying: those are shared *files* and a file cannot be split, whereas this moves a whole crate.
The precedent set is "one capability per crate-level concern", consistent with leaving them alone.

**B — ANY→ALL: REJECTED. Keep ANY.** My justification was a miscategorisation: `09048b84` is not a
rule false positive but a **map gap** — CAP-33 was semantically right and owns only `ota_rate.rs`
and `profile.rs`, not the engine's OTA arm. ALL would have been a second, silent relaxation layered
on a curation gap. The launder is measured, not theoretical: CAP-66 owns a touched file in **49 %**
of prod commits and six ids bless every `engine.rs` commit (31 %), so appending one broad id turns a
copy-pasted template into a permanent pass. Replay: ANY 8 FAIL / 14 PASS, ALL 7 / 15 — a 1-in-22
difference bought at that price. Enforcement is deferred to its own PR with two required changes:
the PR-body path must gain a relevance check (`traceability.yml:88` lints the body via
`--message-file`, which cannot inspect a diff — and **all seven mislabels on `main` are squash
messages**), and the self-test needs a probe that can tell ANY from ALL.

**C — ordering: two PRs, split first.** My retroactivity argument was wrong — the lint runs on a
PR's `base..HEAD` and on the PR body, never on `main` history, so nothing already blessed is
re-judged. Only two commits since the bright line touch `plugins/js8/src` or
`openpulse-discovery/src` and neither carries `Refactors:`, so the enforcement A/B is stable against
either map. This PR is the split alone.

**Also surfaced, filed separately rather than widening this PR:** relevance is checked on
`Refactors:` only — 22 of 101 prod-touching commits. The other 51 carry `Implements:` and are never
relevance-checked; five repeater commits carry `Implements: REQ-FUN-11` ("Support signed transfer
manifests") which is systematically wrong. And 6 `crates/openpulse-gpu/src/shaders/*.wgsl` files are
production to the lint's `is_prod` but invisible to `trace.py`'s `*.rs`-only orphan scan.
