---
project: openpulsehf
doc: docs/dev/project/release-1.0-criteria.md
status: living
last_updated: 2026-08-03
---

# What 1.0 means

**Draft, except for the *Decided by the maintainer* section below — those three are settled.** This
exists because "pre-1.x" was undefinable:
the only reference to 1.x anywhere in the repo was backlog item 12 ("wide-channel, targeted at a
future 1.x"), so there was no bar to plan against. This proposes one.

Current version: **v0.15.0**. Requirement coverage: **118 of 141 ✅ covered, 16 ⚠ gap, 7 📝 planned
(1.x)** per [traceability-matrix.md](traceability-matrix.md).

---

## The claim 1.0 makes

> **1.0 means: an operator can run this on a real radio, on the air, legally, and the behaviour
> matches what the documentation says — with the adaptive rate control exercised on air in both
> directions, not only in a simulator.**

Everything below follows from that sentence. If a criterion does not serve it, it belongs in 1.x.

The emphasis on *measured on air* is deliberate and is the single largest thing standing between
today and 1.0. v0.13.0 → v0.15.0 is three consecutive releases of HF-fade work validated against the
Watterson channel simulator only. v0.14.1 already caught the simulator misleading us — the link
simulator could not transmit the sub-floor rung, so a fading run read as a total link failure when
the truth was 20/20 frames delivered at ~5 bps. Every fade number produced before that fix was
suspect. A 1.0 that ships fade behaviour no one has heard on a radio is making a claim it has not
earned.

---

## Exit criteria

Each criterion is either **objectively checkable** (a command, a gate, a document) or explicitly
marked as a judgement call. Nothing here should require interpretation to score.

### A — On air (the load-bearing group)

| # | Criterion | How it is scored |
|---|---|---|
| A1 | A two-station on-air QSO completes over HF using the `hpx_hf` ladder, with logs retained | Evidence bundle in `docs/dev/test-reports/on-air/` via `scripts/onair-bundle-evidence.sh` |
| A2 | The rate ladder is observed **responding correctly to real on-air link conditions, in both directions**, with two-ended evidence | See A2 scoring below — a count of transitions is explicitly *not* the measure |
| A3 | One end-to-end Winlink message exchanged over RF with a real CMS/RMS gateway | Retained session log + the delivered message |
| A4 | Station ID cadence verified on air against the operator's national rules | Regulatory checklist run, exceptions documented |
| A5 | PTT keying verified fail-safe on the real rig (release on error, release on abort) | Deliberate fault injection during an on-air window |

**A1–A3 are the gate.** A4–A5 are safety items that must also pass, but they are checks rather than
discoveries.

#### A2 scoring (revised 2026-08-05 — see *Decided by the maintainer*)

**Why it changed.** The original wording scored ≥3 transitions "driven by real channel conditions".
Natural fading is not controllable, so that made the gate depend on the ionosphere cooperating on the
day: a quiet band fails it while proving the controller is fine, and a disturbed band passes it
without proving more. It measured propagation, not software.

**Scored by a retained two-ended session log plus capture, showing all three of:**

1. **≥1 climb in the receiver-led direction**, where the sender's *subsequent* frame at the new mode
   **decodes**. A climb that is recommended but never successfully used proves nothing; closing that
   loop is the genuinely on-air-only property, since the simulator already exercises the controller's
   direction behaviour (`apps/openpulse-linksim`) and its lockstep under arbitrary ACK loss.
2. **≥1 demotion in the receiver-led direction**, following at least one logged decode failure.
3. **Stability**: over a window of clean decodes on a stable link, **no demotion occurs**. This is
   the clause that can actually fail — a broken estimator produces a sawtooth (drop-to-floor on each
   blip, evidence-climb back up), and a quiet band supplies the precondition for free. The
   maintainer's objection is what makes this clause cheap to satisfy honestly.

**Corroboration, because the log is written by the software under test.** Each transition must appear
in *both* stations' logs — a receiver-led recommendation must show up at the sender as an adopted
level and a changed transmit waveform — and be visible in the retained capture, where a mode change
is physically identifiable by baud and occupied bandwidth without trusting the modem. A transition
attributable *only* to the SNR estimate does not count: the estimate is the model, the decode is the
observation, and this repo's rule is that the observation wins.

**Induced level changes are allowed and must be recorded.** Reducing TX power or changing antenna is
legitimate — the controller consumes `(RxOutcome, snr_db)` and does not know why the level moved.
Manual `lock_level` commands do **not** count.

**Residual, stated rather than hidden.** Induced runs exercise the SNR-*explained* branches. The two
mechanisms added in #934 precisely because the estimate is uninformative on a fade in principle — the
evidence-based climb and the adequate-SNR hysteresis — are plausibly *not* exercised by level
inducement. That regime stays simulator-tier unless natural fading happens to be captured. 1.0
therefore claims the rate control works on air; it does **not** claim it has been characterised
across HF conditions.

**Prerequisite (open engineering, not bookkeeping).** As of 2026-08-05 this criterion is
**unscoreable**: the production OTA path emits no rate-transition event. `EngineEvent::RateChange` is
raised only from the legacy `rate_policy` path (`engine.rs:1393/1405/1459`); the OTA sites
(`engine.rs:1589`, `2253`, `2257`) emit nothing, and the daemon logs no level transition. A2 cannot
be scored until that event exists, carrying `(from, to, RxOutcome, snr_db, branch taken)`. Phase 5.5-reg in [onair-status.md](../onair-status.md) already carries the execution
checklist; this group is that checklist reaching completion, not a new workstream.

### B — Requirement bookkeeping is true

The matrix currently shows 16 gaps. **Several are bookkeeping, not engineering** — the capability
exists and is enforced, but the row was never re-assessed. Verified while drafting this:

- `REQ-DOC-01` (version bumps update changelog + release notes) — **enforced** by
  `scripts/check-version-bump-docs.sh`; it was run and passed for the v0.15.0 cut.
- `REQ-DOC-02` (docs pass frontmatter validation) — **enforced** by
  `scripts/validate-doc-frontmatter.sh`; passes today.
- `REQ-PLAT-05` (ARM64 in regular compatibility testing) and `REQ-NFR-01` (Linux/macOS buildability)
  — the CI jobs exist and are correct (`cross-aarch64-linux`, `macos-build`), but the `CI` workflow
  is `disabled_manually`, so nothing runs them automatically.

| # | Criterion | How it is scored |
|---|---|---|
| B1 | Every ⚠ gap row is re-assessed: either ✅ with evidence, or restated as a genuine open item with an owner | `grep "^| REQ-" traceability-matrix.md \| grep -v "✅ covered"` reviewed row by row |
| B2 | No requirement is marked ✅ on the strength of a test that does not exercise it | Follows from D1 below |
| B3 | The regulatory requirements (`REQ-REG-01..12`) are either satisfied, or restated as **operator responsibilities** with the supporting documentation shipped | `docs/regulatory.md` states which is which, per jurisdiction |

**On the CI-dependent rows specifically:** CI being disabled is your deliberate choice and the gates
are run locally before every merge. That is a legitimate answer — but it is not what the requirement
*says*. 1.0 should either re-word those requirements to describe local pre-merge gates, or turn CI
back on. Leaving them as "gap" while the work is actually being done is the third option and the
worst one, because it makes the matrix untrustworthy in both directions.

### C — Security posture is stated honestly

1.0 ships a mesh relay, a transmitter-commanding control channel, and a signed handshake. The bar is
not "no known weaknesses" — it is **no undocumented ones**.

| # | Criterion | How it is scored |
|---|---|---|
| C1 | `WireEnvelope.auth_tag` is either verified, or documented as unverified with the operator-facing consequence spelled out | E1/E3 in the [handshake-trust audit](../reviews/2026-07-15-handshake-trust-audit.md) closed or explicitly deferred in writing |
| C2 | Every autonomous/outward action is off by default and terminable | Already true; re-verified as a checklist |
| C3 | The control channel's auth story is complete **or** its limits are documented | Today: TCP is Noise-authenticated and fails closed; the WebSocket port is *disabled* when auth is required on a non-loopback bind. That is safe but means no authenticated remote panel — state it or fix it |
| C4 | No security claim in the docs outruns the code | The 2026-07-18 consistency audit found one such claim ("Winlink Type C wire-compatible") 148 lines below its own retraction; this criterion is that class staying closed |

### D — Test integrity

The suite is large (2146 passing). Size is not the property that matters.

| # | Criterion | How it is scored |
|---|---|---|
| D1 | Every acceptance-criteria row names a test that exists, runs as written, and asserts the property claimed | Each command in the CLAUDE.md table executed and returning a non-zero test count |
| D2 | Coverage is **measured** and a threshold agreed | No coverage tooling exists in the tree today — this is new work |
| D3 | No known vacuously-passing gate | Five have been found and fixed (`bpsk_hardening` SNR sweep, `tx_limiter`, CAT `write_log`, `fec_decision_gate`, `relay_empty_buffer`); the criterion is that a fresh sweep finds no more |
| D4 | The benchmark and goodput regression gates pass at the release commit | `benchmark run` 10/10 with `mean_transitions ≤ 20`; `goodput_gate` |

D1 was satisfied as of 2026-07-18 and should be **re-checked at the release commit**, not assumed —
three of those rows were unrunnable as written until that date, and two named tests that never
decoded anything.

### E — Documentation matches the code

| # | Criterion | How it is scored |
|---|---|---|
| E1 | A consistency audit over docs/code/comments/tests finds no unresolved contradiction | Repeat of the [2026-07-18 audit](../reviews/consistency-audit-2026-07-18.md) at the release commit |
| E2 | The published decode specification is complete enough for a third party to write an interoperating decoder (`REQ-REG-02`) | `docs/dev/design/protocol-wire-spec.md` + the mode/FEC ladder reviewed against that standard |
| E3 | Operator-facing docs cover install → configure → first QSO without reference to dev docs | `docs/openpulse-manual.md` walked end to end by someone who has not read the source |

E2 is worth singling out: FCC §97.309(a)(4) requires a *published* specification, so this is a
regulatory obligation for US operators, not documentation polish.

---

## Explicit non-goals for 1.0

Naming these matters as much as the criteria — it is what stops 1.0 from receding.

- **Wide-channel VHF/UHF (12.5/25 kHz, `REQ-BW-01..07`)** — backlog item 12, explicitly 1.x. Its
  Phase 1 (sample-rate generalization off the hard-coded 8 kHz) is worth doing sooner because it also
  unblocks `hpx_narrowband_hd`, but it is not a 1.0 gate.
- **On-air validation of FF-15 (JS8 discovery) Phase H and FF-16 (file transfer) Phase F** — both
  subsystems are off by default. 1.0 may ship them as documented-experimental rather than block on
  their on-air campaigns.
- **Relay envelope authentication** — genuinely blocked on a key-distribution design decision. C1
  requires it be *documented*, not solved.
- **Proprietary-protocol compatibility (`REQ-PERF-05/06`)** — requires legal review; out of scope.
- **A GUI feature-parity target with VarAC or similar** — see the
  [gap analysis](../research/varac-feature-gap-analysis.md); research, not a gate.

---

## What this implies about sequencing

The criteria sort into three groups by what unblocks them:

1. **Needs hardware only (A).** Cannot be started at a desk. Largest risk, longest lead time,
   and the only group that retires the fade-arc uncertainty.
2. **Needs a decision (B3, C1, C3).** Doable today, at a desk, in hours — mostly writing down which
   of two honest positions you are taking.
3. **Needs engineering (B1, D2, E2).** Days to weeks: matrix re-assessment, coverage tooling from
   scratch, and reviewing the wire spec to third-party-implementable standard.

Group 2 is the cheapest and is currently blocking nothing but itself. Group 1 sets the release date.

---

## Decided by the maintainer

1. **On-air evidence is a HARD GATE for 1.0** (decided 2026-08-03). "Simulator-validated, on-air
   pending" is **not** an acceptable 1.0, with or without a release-note caveat. The A-series
   criteria below are therefore blocking, not aspirational, and no amount of simulation substitutes
   for them — the evidence tiers are independent (`CLAUDE.md`, *evidence-tiers*: unit test < model <
   hardware-in-the-loop < field).

   **Consequences, so this is not just a line in a table:**
   - The critical path to 1.0 runs through the rigs, not the modem. Work that cannot be validated
     on air does not shorten the release.
   - The FT-991A receive-path blocker (A→B fails offline too, so it is in the receiver) is on the
     critical path, and so is FF-15 Phase H.
   - Any wire-format change (#1062) re-opens the on-air evidence, because the recorded corpus in
     `crates/openpulse-modem/tests/captures/` contains the **old** preamble — so the corpus must be
     re-recorded after the break, and the campaign run against the final format.

2. **The wire format may change freely until 1.0 is tagged — and that is the reason to maximise
   maturity before tagging** (decided 2026-08-03). The tag is the irreversible act; the on-air
   campaign is cheap and repeatable. Post-1.0 a format change costs a compatibility-mode discussion,
   permanently. The order is therefore:

   > **wire format / modem maturity → on-air campaign → tag 1.0**

   This **supersedes** the "paying for the campaign twice" objection above as a reason to *defer* a
   format break: that objection only bites once a campaign has already been paid for, and none has.
   #1062 is consequently on the critical path and lands **before** the campaign, not after.

3. **Add-on DSP stays runtime-optional so on-air runs can attribute its effects** (decided
   2026-08-03). The bare modem is the low-risk part — it re-implements known-to-work references and
   invents nothing. The risk sits in what is layered on top (receiver notch, capture AGC, CE-SSB),
   which must be **proven on air, not assumed**. Each must therefore be switchable at runtime so a
   campaign can be run with and without it and the difference attributed — the repo's standing
   ablation discipline applied to the deployment surface. The flags already exist
   (`modem.notch_enabled`, `modem.agc_enabled`, `modem.cessb_enabled` in `openpulse-config`); what is
   missing is an on-air runner that sweeps them and records which combination produced each evidence
   bundle.

4. **A2 is scored on correct response, not on a transition count** (decided 2026-08-05). Natural
   fading cannot be controlled, so requiring "≥3 transitions driven by real channel conditions" made
   the gate depend on propagation rather than on software — a quiet band would fail it while proving
   the controller fine. A2 now requires one climb (whose new mode is then used successfully), one
   demotion, and **stability on a good link**, all corroborated across two stations' logs and the
   retained capture. Induced level changes count and must be recorded.

   This **narrows decision 1 without weakening it**: the evidence stays hardware-tier — real RF, real
   estimator on real captured audio, real ACK channel, real PTT turnaround — and no simulator result
   is substituted. What it gives up is any claim about *severe* fading, which is why the headline
   claim above was edited in the same change rather than left to outrun the criteria.

## Open questions for the maintainer

1. **CI: re-enable, or re-word the requirements to describe local pre-merge gates?** Both are
   defensible; the status quo (requirements describing CI that does not run) is not.
2. **Is a coverage threshold wanted at all,** or is the acceptance-criteria table considered the
   real quality gate? D2 assumes yes; it is genuinely optional.
3. **Which jurisdictions does 1.0 claim compliance documentation for?** `REQ-REG-07..12` name FCC,
   CEPT/EU, BNetzA and Ofcom. Claiming fewer is faster and more honest than claiming all four.
4. **If an add-on cannot be proven on air by 1.0, does it ship off by default or get removed?**
   Off-by-default keeps the code path alive but makes 1.0's shipped configuration the bare modem.
