---
project: openpulsehf
doc: docs/dev/project/release-1.0-criteria.md
status: living
last_updated: 2026-08-18
---

# What 1.0 means

**Draft, except for the *Decided by the maintainer* section below — those four are settled.** This
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

**Prerequisite (open engineering, not bookkeeping) — corrected 2026-08-05.** An earlier version of
this paragraph said the production OTA path "emits no rate-transition event" and that A2 was
therefore unscoreable. **That was wrong**, and the error is instructive: the check behind it grepped
`server.rs` for `info!`/`debug!` level logging and found none. The filter was sound; the corpus was
the wrong one. The daemon's observability surface is **ControlEvents**, not tracing, and
`ControlEvent::OtaStatus` (`lib.rs:2624`) already carries `tx_level`, `rx_recommended_level`,
`rx_confirmed_level` and `is_locked` — broadcast at ~1 Hz during a session and again immediately
after every `apply_ota_ack` (`server.rs:1029/1640/1677`), reaching `events.ndjson` through the audit
recorder when `observability.audit_mode` is on, and already consumed by the panel.

So level *state* is observable and transitions are reconstructible by diffing snapshots. What is
genuinely missing is **attribution**: which branch of the controller fired, whether a climb came from
the SNR model or from decode evidence (the #934 distinction this criterion turns on), the outcome and
SNR that drove each decision, and per-decision rather than 1 Hz granularity on the receiver side.
Failed decodes are also unobserved — `FrameReceived` is success-only — which clause 2 needs.

A2 is therefore **not attributably scoreable** rather than unscoreable, and #1081 is scoped to
attribution. Note also the operational precondition: `observability.audit_mode` must be on for the
session, or the events are broadcast but not retained.

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

---

## Status and sequencing — snapshot 2026-08-18

Written after the maintainer asked three questions: are we going in circles, what is left, and does
it make sense to keep building without testing against recordings and noise sources when other
projects have less-tested software that is proven in practice. Adversarially reviewed; the review
corrected the first draft in two load-bearing places, both marked below.

### Are we going in circles?

**No — a convergent spiral with an identified cause.** Between 11 % and 18 % of the last 60 days'
commits touch acquisition (98 by subject keyword, 107 by file, 157 by a looser keyword set — the
figure is methodology-dependent, the regime is not). Thirteen open issues sit in that thread. One
subsystem, seven repairs.

Two causes are on record, and the second was only named this week:

1. **The energy gate was the wrong primitive** — five issues (#1020, #1021, #1039, #1040, #1045) were
   one design choice, closed by #1049's correlation veto.
2. **The tested surface was not the shipping surface** (#1118): the acquisition chain ran only on the
   CLI listen path, so #1053/#1059/#1060/#1062 were refining a path a real station never executed.
   Measured: the daemon could not acquire a station **50 Hz off frequency**, while `REQ-PHY-03`
   requires ±50 Hz and the one clean inter-rig measurement on this hardware is **−64 Hz**.

**Why "circles" undersells it.** That work produced the capture corpus, the regression gates and the
runtime calibration — the ratchet that makes the *next* defect of this class cost no radio time. The
loop has been closing, not repeating.

### On the maintainer's doubt about testing versus shipping

The comparison to projects that are "less tested but proven in practice" inverts on inspection:
those projects have **more** real-world evidence, not less — operator field-hours substituting for a
suite. This project has approximately **zero field hours**. So the comparison is an argument *for*
the on-air campaign, not against the testing.

And within testing, the split matters. Every synthetic-only conclusion in this thread that met
reality was overturned: "the acquisition chain earns its keep", "a 200 Hz filter makes BPSK250
undecodable", "~400 Hz inter-rig offsets". Recordings and rig captures are what has been *breaking*
the loop. Simulation-first is what produced it.

**Worth adopting from those projects, after the format freezes:** binaries in a handful of other
operators' hands, labelled experimental. A small fleet is a rig-diversity and noise-source generator
no corpus can replicate, and it is the only route to a "characterised across HF conditions" claim —
which 1.0 explicitly does not make.

### What remains

| Group | Status |
|---|---|
| **A — On air** | A1 partly (one direction decoded 2026-07-30); A2 **not scoreable until #1081 attribution lands** — failed decodes are unobserved and that is open engineering, not radio time; A3 not started; A4/A5 unverified. Newest evidence bundle: 2026-07-30. **Lead item is a purchase**: G0 galvanic USB isolation blocks the campaign and is neither code nor radio time. |
| **B — Bookkeeping** | 16 matrix rows, several bookkeeping-only; CI is `release/**`-only so some rows describe gates nothing runs (#1144, #1129). |
| **C — Security honest** | Mostly there; C3 (no authenticated remote panel) to state or fix. |
| **D — Test integrity** | Strong. D2 (coverage tooling) does not exist — days to weeks, from scratch. |
| **E — Docs match code** | Repeat audit at the release commit. **E2 is a regulatory obligation** (§97.309(a)(4) third-party-implementable spec), not polish. |

### The wire-format break package (proposed 2026-08-18)

Decision-block item 2 above settles that a break happens and that **#1062 lands before the campaign**;
that row therefore carries the maintainer's authority, and the rest of this section is this
write-up's proposal awaiting ratification. What was open is *which* changes go in the one window.
This answers it from an inventory of what actually goes on the air, rather than from the set of
issues that happen to be filed — because item 1's consequence means a straggler after the campaign
re-opens the campaign.

**The window contains one break; each item ships as its own PR with its own gate.** Bundling the
*decision* is what matters; bundling the *changes* would only make the diff unreviewable.

#### In the package

| item | what changes | why it cannot wait |
|---|---|---|
| **#1062** preamble | period-4 `--++` run → PN/chirp sync word | decision item 2 puts it on the critical path. It also reshapes the **unwhitened, pre-FEC** region — the only place a future format-epoch marker can live |
| **#1147** handshake encoding, **classical and PQ** | serde JSON (`Vec<u8>` as number arrays) → binary, on both handshakes | 710 B CONREQ ≈ 23 s uncoded at `active_mode` (BPSK250); the PQ Hybrid CONREQ is 17 939 B ≈ **9.6 min**. Maintainer decision 2026-08-19: PQ is scoped **in** — see the rider below |
| **#1148** whitener | 21-bit effective period → the intended x⁹+x⁵+1, period 511 | one line, and permanent once tagged |
| **QSY frames** (#1162) | add a version token | `openpulse-qsy/src/frame.rs` is versionless *and* magicless — plain CR-terminated text lines |
| **rendezvous codec** (#1163) | add a version token | `openpulse-discovery/src/rendezvous.rs` ships on-air tokens over JS8 directed free text with no version. Its siblings have one (`FILEXFER_VERSION`, `HINT_VERSION`), so leaving it out would be oversight, not a ruling |
| **`WireEnvelope`** (#1164) | make the version byte authoritative | today it is deliberately non-authoritative (`wire_query.rs:204`, forward-compat by intent): the byte is never bound and v1/v2 is resolved by trailer length. Make it authoritative before the tag, while that is free |
| **FreeDV auth beacon** (#1206) | add a wire magic + version (`OPAB` + version byte) and move the signing domain in-band | `openpulse-freedv-auth/src/beacon.rs` had a `[u16 BE len]` prefix but no magic and no version — on the one message whose whole purpose is a verifiable identity claim. Missed from this table by oversight, not by ruling. The crate has **no in-repo dependent and no binary**, so the signature change is free exactly once; `docs/openpulse-manual.md:797-805` invites external companion processes, which are precisely the consumers that cannot be rebuilt in lockstep |
| **`AckFrame`** (#1165) | ~~reject non-zero reserved bits 7:5 on decode~~ **DONE** — enforced in both decoders through one shared `check_b0_reserved`, so they cannot drift; in the authenticated path it runs *after* the MAC (authenticity first, then format) | `ack.rs:129-158` never checks them and both encoders leave them zero, so this is **not a break** — and they are the clean version headroom a 5-byte frame has |
| **negotiation fields** (#1166) | ~~decide the fate~~ **DECIDED and DONE** — both deleted in the #1147 wire break (PR #1189); the signing-mode membership check was added in their place | the daemon sends them empty and hardcodes `None` in the CONACK (`openpulse-daemon/src/lib.rs:1548-1549, 1974-1975`) — the format's one negotiation mechanism is unwired |
| ~~**`Frame` payload length** (#1167)~~ **decided: keep `u8`** | no change | SAR carries objects to 64 005 B, so the cap is not a functional limit; header overhead at 255 B is 3.9 %, and a longer frame loses more per fade outage. **Falsifier:** a top wideband rung whose goodput proves *turnaround-bound* rather than payload-bound reopens it — a linksim measurement, not an argument |

#### Two items that carry more than their issue says

* **#1147 covers the signature domain, and the PQ handshake is scoped in** (maintainer, 2026-08-19). `encode_pq_conreq` is a bare
  `serde_json::to_vec` — no magic, no version, no length prefix (`pq_handshake.rs:488-505`) — and
  ~5 KB of key material expands ~4× as JSON number arrays, on the order of 10+ minutes at BPSK250.
  Both handshakes sign **serde declaration order** (`handshake.rs:307-322`, `pq_handshake.rs:254-257`);
  "canonical" in those doc comments is a label, not key-sorting (only `pki-tooling` sorts). So
  re-encoding is a **signature-domain** change on the classical CONREQ/CONACK as much as on the PQ
  path — treat it with that risk class, not as a codec swap.

  **What scoping PQ in does not buy, so nobody reads it as more than it is.** Binary encoding takes
  the PQ Hybrid CONREQ from 9.6 min to ~2.7 min at BPSK250, which is still unusable for a handshake:
  PQ on this link needs cached identities or out-of-band key distribution, a separate design
  question that is *not* part of 1.0. What the decision buys is that the **format is finished before
  the tag**, so wiring PQ later is not a wire break. It is also low-risk work: verified with a
  positive control (the classical `ConReq::create_full` has three daemon callers), the PQ handshake
  has **zero production callers** — 38 references in its own integration test, 6 in its own module —
  so the only consumer that must move with the format is that test.
* **#1148 costs more than its one-line fix.** The **four recorded-frame** replay gates
  (`capture_replay_corpus.rs:211, 266, 320, 370`) decode real captures whitened with the 21-bit
  keystream and go red on the change; the synthesized-frame gates (`:145, 418, 500`) build both ends
  from the same build and stay green. **Corrected 2026-08-21: this said "three" and listed three —
  `the_settle_recovery_reaches_the_frame_without_crawling` (`:370`) decodes a whitened capture too,
  and the miscount was carried into the package section from here.** They are now `#[ignore]`d with
  a tracking-and-epoch reason, and **un-ignoring them is part of the re-record's definition of
  done**. The "test-only legacy keystream" alternative is **struck**: `#[cfg(test)]` cannot reach
  integration tests, so it would require a cargo feature carrying a second wire format in production
  source, and it would attest a build no station runs — the opposite of what a real-capture gate is
  for. These captures also carry the pre-#1062 preamble, so anything that keeps them alive through
  #1148 dies again at #1062 within the same window: land the break PRs in tight succession and
  re-record **once**, against the final format. The change must also add a **period gate** — all six existing scramble tests
  (`scramble.rs:142, 181, 224, 251, 264, 281`) pass on a 21-bit keystream because none measures
  period (`the_keystream_is_not_degenerate` checks only non-constancy and ones-balance), so the next
  tap typo would be equally invisible. Sabotage-verify that gate against the current taps.

  Severity, stated honestly: #1148 is **not acutely broken**. The #1021 dead-carrier property holds
  at period 21. The residuals are 21-byte-periodic payload content re-creating runs, spectral lines
  in the whitening, and the #1139 onset aliasing. The reason to fix it is "free now, permanent
  later", not breakage.

* **#1206 ships the token, not the re-encode — recorded so the omission is a decision.** The beacon
  is **356 B of JSON / 363 B on the wire** (measured, both callsign lengths) against the **144-byte binary** budget its own research doc
  costed (`docs/dev/research/freedv-auth-research.md:64,72,220`) — roughly 30 s of FreeDV 1600 text
  channel versus the 12 s designed for. So if FF-11 is ever wired, a JSON-to-binary re-encode of the
  #1147 shape is close to inevitable, and it carries #1147's subtlety too: `beacon.rs`'s "canonical
  JSON" is serde declaration order, a label rather than key-sorting. The version token is exactly
  what turns that later migration into a **branch instead of a break**, which is why deferring the
  re-encode is defensible and why the token is not.

  **An explicit exemption was considered and declined.** It needs the premise "FF-11 is
  documented-experimental", and that premise is already false in this repo: `roadmap.md:1331` marks
  FF-11 ✅ Done, CLAUDE.md lists the FF series complete, and the manual presents it as a supported
  integration path. Writing a defensible exemption therefore means **demoting FF-11 across roadmap,
  manual and book** — more churn than the token, and a status downgrade outside #1206's scope. An
  exemption is also a standing claim that must *remain* true, which is the #1120/#1144 archetype: a
  true statement invalidated by a later change nobody sweeps. The token is fire-and-forget.
  **Retiring the crate** was the one real alternative — it is incomplete against its own design (the
  research doc specifies a binary that was never built) — but that contradicts the roadmap's ✅ and
  is a product call above this issue's pay grade. Named here as considered-and-declined.

* **`AckFrame` byte 3 stays unenforced, deliberately** (#1211, ruled by the maintainer 2026-08-28).
  #1165 enforced byte 0's reserved bits 7:5; the symmetric tightening of byte 3 was measured
  (`b[3] = 0xFF` with both presence flags clear decodes fine) and then **declined**. Byte 3 is
  payload rather than a reserved region — fully allocated when both flags are set — so an extension
  must announce itself in byte 0, which is now enforced and already provides the detection.
  Enforcing byte 3 would also convert benign CRC-8 collisions in those bits into lost ACKs on the
  legacy path. Contract recorded in `protocol-wire-spec.md` §5.1: *must be zero on transmit, ignored
  on receive.* **Falsifier:** an extension mutually exclusive with both `reverse_ack` and
  `recommended_level` — an extended reason code on a `Nack` — reopens it.

* **The SAR 4-byte sub-header stays versionless, deliberately.** Raised by #1206's closing note and
  recorded here so it does not stay absent-by-oversight the way the beacon did. `sar.rs`'s
  `segment_id | fragment_index | fragment_total` header carries no version of its own and escapes
  via the coarse `OPLS` `Frame` version byte, which every fragment already sits inside. That
  containment is real — a `Frame` version bump re-labels the SAR layer with it — so the cost of a
  dedicated token (4 bytes off a 251-byte fragment payload, ~1.6 %, on **every** fragment of every
  multi-fragment object) is not worth buying a second version of the same fact. **Falsifier:** a
  change that alters SAR framing *without* altering `Frame` reopens it, because then the `OPLS` byte
  no longer moves when the SAR layer does.

#### Why #1062 stays in, against the two arguments for dropping it

The *Sequencing* section below records two grounds for declining: #1062's own `demod_parity`
measurement (no resolvable decode gap versus PN/Barker candidates at n = 96), and that "#1157's
calibration and #1118's seam have since reframed" the break's justification. Both stand as
statements; neither carries the decline.

1. **`demod_parity` measures the benefit PN does not provide.** F7 is two-sided: only *duration*
   buys noise-floor margin, while **PN buys onset placement (peak sidelobe 0.997 → 0.234) and
   interferer refusal**. Those are the open defects — #1049 point 3 (onset placement is unresolvable
   with a *periodic* preamble, in both fixture cases), #1049 point 2 (a tone on a spectral line
   scores ρ ≈ 0.70 at any grid width), #1139 (HARQ needs an absolute onset). A decode-parity harness
   at n = 96 measures none of them. A longer *periodic* preamble is not a cheaper substitute either:
   its time-bandwidth product is O(1) however long it runs, so duration only pays for a PN template.
2. **#1157's CFAR removes the veto where the noise floor is worst.** It can only raise the threshold,
   and when the derived level passes the mode's delivered-frame bound it **stands down to
   energy-only, with hysteresis** — measured at 309 Hz, deriving 0.618 and standing down. So at a
   narrow filter the calibration does not cost detection; it costs *protection*, which is an argument
   for the preamble work rather than against it.
3. **#1118 reframes the seam in the opposite direction to the decline.** Before it, the acquisition
   chain ran on almost no shipping surface (CLI `receive --listen-ms` only), which is why preamble
   quality could be treated as a research concern. #1118 puts that chain on the daemon's streaming
   path — the surface a station actually receives on — so preamble quality now matters in production.

#### What this package does *not* buy

There is **no backwards-compatibility mode for the data-plane wire format**, and this package does
not create one. The mechanism that exists — `docs/dev/design/ladder-versioning.md`, wired as
`profile_name` + `profile_fingerprint` in the signed CONREQ/CONACK, with a fingerprint mismatch
dropping the receiver out of the OTA arm (`openpulse-daemon/src/lib.rs:1663-1683`, gate
`server.rs:858`) — scopes only *what a SpeedLevel number means*. Every other versioned structure
validates equality and rejects (`frame.rs:74`, `handshake.rs:349, 578`), and the `Frame` version byte
is itself whitened, so a whitener or preamble change is never even reached by a version check. The
ACK is whitened too, which is why the unwhitened pre-FEC region is the only real estate a format
epoch could occupy.

A post-1.0 change therefore remains possible but is not free: recovery would need dual-decode, and
descrambling sits **before** FEC at nine `demodulate_soft` call sites plus the hard seam
(`engine.rs:320, 6806`), with HARQ accumulators needing partitioning per keystream — inside the most
budget-constrained subsystem in the repo. That is the argument for closing the whole inventory in one
window rather than leaving stragglers.

### Sequencing (corrected)

An earlier draft said "park the acquisition backlog and go on air". **That contradicted a settled
decision in this document** (*Decided by the maintainer*, item 2): the wire format may change freely
until the tag, so **#1062 is on the critical path and lands before the campaign**, because a format
break re-opens the on-air evidence and forces the corpus to be re-recorded.

The correction is to split the backlog by kind:

1. **Merge #1118** — the daemon's acquisition fix; everything downstream assumes it.
2. **Decide the wire-format package**: #1062 (preamble sequence), #1147 (handshake binary encoding),
   #1148 (whitening period). *Decide*, not defer. The grounds for declining were: #1062's own
   `demod_parity` measurement found no resolvable decode gap between the shipped preamble and
   PN/Barker candidates at n=96, and the break's justification was largely "make the veto work",
   which #1157's calibration and #1118's seam have since reframed.
   **Status: decided — see *The wire-format break package* above.** Both grounds stand as
   statements and neither carried the decline; the package section answers each in turn, and widens
   the window from three issues to an inventory of nine.
3. **Park the threshold-tuning residue** — #1053, #1059, #1146, #1160 — under three conditions that
   make it safe (verified in code): #1118 merged first; every evidence bundle **CAT-reads and records
   the rig's filter width and frequency trim** (the preflight verifies stand-down but never reads the
   rig, and `M PKTUSB 0` does not restore an IC-9700 width); and any A2 decode-failure anomaly at a
   no-template rung is checked against the SDR capture before being booked as ladder evidence.
4. **Land #1081 attribution** and the bundle filter/trim read-back — before the radio window, or the
   window is wasted.
5. **G0 hardware** (galvanic USB isolation).
6. **A1–A3 with the SDR as trusted witness.**
7. **B, D1, E at the release commit.**

### The "acquire wide, decode narrow" idea

The maintainer proposed acquiring on the unfiltered signal and decoding on the filtered one, as an
answer to #1060's narrow-filter problem. **The instinct is the textbook architecture, and the decoder
already implements it**: `plugins/bpsk/src/demodulate.rs` multiplies by I/Q reference carriers and
matched-filters per symbol, and a per-symbol matched filter *is* a band-pass of ~baud width around
`fc` — the optimal linear receiver for stationary noise. A software band-pass in front of it adds
nothing to the decision statistic. The rig's narrow filter is a second, worse-shaped filter ahead of
the good one.

Three consequences:

* **Not implementable as stated** — the software receives one already-filtered audio stream and
  cannot un-narrow it. The remainder is operator behaviour.
* **Not measurable from the current corpus** — the 500 Hz and 250 Hz captures are *idle noise only*;
  no frame was recorded through a narrow rig filter. #1060 measured the acquisition side; the decode
  side has no fixture.
* **At the narrow end the premise inverts**: a 250 Hz filter is narrower than BPSK250's own mainlobe,
  and #1160 records 0 of 180 decodes through that mask.

What is worth doing, and is cheap: **(a)** operator guidance to run the receive filter wide (≥2.4 kHz)
for these modes, stated conditionally because a narrow rig filter does one thing software cannot —
protect the rig's AGC and ADC from a strong adjacent signal; **(b)** a noise-bandwidth estimator that
*warns* the operator their filter appears narrow, validatable against the three same-session captures
(measured −20 dB bands 2470 / 554 / 309 Hz) and belonging on the daemon control plane where #1157's
`rho_*` getters already sit dormant awaiting #1118; **(c)** nothing else — the calibration half
shipped as #1157.
