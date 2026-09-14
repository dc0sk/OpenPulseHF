---
project: openpulsehf
doc: docs/dev/reviews/artifacts/1267-mac02-coverage.md
status: review
last_updated: 2026-09-15
---

# Decision record — REQ-MAC-02 is a gap, not covered (#1267, with #1344 and #1351)

Design-class because it edits `requirements.yaml`: changing which capability covers a requirement is
a decision about what the project claims, even when the code is untouched.

## Consumer

Who reads what this changes, by `file:line`:

- `scripts/lib/trace.py:890` — the `REQ-GAP` check, which reads `covered_by` and is what turns this
  edit into a visible warning rather than a silent one.
- `scripts/lib/trace.py:895` — the `BIDIR-DRIFT` check, which is why `CAP-31.satisfies` and
  `REQ-MAC-02.covered_by` must both change or neither.
- `docs/dev/project/traceability-matrix.md:102` — the human-readable row, and `:249` for CAP-31's
  description.
- `docs/dev/project/reachability-baseline.txt:63` — the `DORMANT` note (#1344 part 1).
- `CLAUDE.md` acceptance rows for the two `#[ignore]`d replay tests (#1351).

## Prior art

- **The gap-recording mechanism already exists and is in use.** 33 requirements carry an empty
  `covered_by` today (`REQ-BW-01..07`, `REQ-CAT-01..05`, …), so this needs no new machinery and no
  baseline entry. Derived with a script over `requirements.yaml`, not from recall.
- **#1268 / `EMPTY-CAP`** — the same family one step over: `REQ-GAP` is cleared by a *non-empty*
  `covered_by` and nothing asked what the covering capability contained. Here the capability is
  non-empty and real; what fails is that its mechanism is off by default.
- **#1359** — the precedent for retiring a claim rather than weakening it, and for keeping the
  reason at the code rather than in the register.

## Twins

- **REQ-MAC-01, -03, -04**, the three sibling rows under the same capability. **Checked and NOT
  changed**, because fixing one of four identical-looking rows is how the other three rot: -01 is a
  negative requirement (point-to-point need not sense), -03 asserts the reference algorithm exists
  (it does, at the emit seam), -04 asserts DCD derives from demodulated energy (`dcd.rs`). Only -02
  asserts that a surface actually senses.
- **The repeater's rig_b carrier sense (#1325)** is a different mechanism for a different problem
  (sensing a band the daemon never listens to) and is not evidence for this requirement.
- **`docs/features.md` and README capability prose** — swept for the same claim; they describe DCD
  and CSMA as capabilities, which remains true, rather than asserting per-surface coverage.

## Prompt

Put to the maintainer 2026-09-14 after verifying every claim in #1267 at `10eb6ef0`: is REQ-MAC-02
covered or is it a gap, given that `csma_enabled` defaults false (`engine.rs:978`) and the only
production callers of `enable_csma()` are both KISS (`kiss/main.rs:103`, `kiss/bridge.rs:176`)?

## Verdict

**Record the gap.** The matrix said ✅ covered via CAP-31, whose description claimed the CSMA "gates
all transmit paths uniformly". It is at the seam and **inert** there, and the requirement names
broadcast and relay — the surfaces that do not sense.

Three things settled with it:

1. **Both sides of the join change**, or `BIDIR-DRIFT` fires.
2. **CAP-31's description is corrected rather than deleted.** The capability is real; the false part
   was "uniformly". The corrected text names the default and the single arming site, so the next
   reader does not have to re-derive it.
3. **The capability gap is tracked as #1376**, carrying the reason it is not a flag flip:
   `RelayForwarder::forward` commits its dedup key *before* transmitting, so a `ChannelBusy` refusal
   becomes a permanent drop rather than a deferral. Anyone reaching for `enable_csma()` would ship
   frame loss.

**Predicted before editing, then confirmed:** `trace.py:638-645` routes a non-enforced entry's flags
to warnings, and REQ-MAC-02 is `traceability: baseline` — so an honest gap does not turn the gate
red. After the edit, `scripts/trace.sh check` reports `REQ-MAC-02: REQ-GAP — no capability covers it`
among 17 other recorded gaps, with `TRACE: PASS`.

Bundled in the same change, both record-only and both decided on their own issues: #1344 part 1 (the
`DORMANT(#1118)` note instructed a reader to do something #1118 landing made impossible) and #1351
(two acceptance rows cite `#[ignore]`d tests and named no owner).
