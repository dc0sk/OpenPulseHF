---
project: openpulsehf
doc: docs/dev/reviews/review-1411-keying-requirement.md
status: resolved
last_updated: 2026-09-19
---

# Design review — register the PTT keying property that eight commits point at and no id carries (#1411)

Nothing is built. This is a proposal to add one or two requirements to `requirements.yaml`, which is
a decision site, reviewed before implementation.

## Consumer

- `docs/dev/project/requirements.yaml` — the registry. `trace.py` reads it for REQ-GAP, EMPTY-CAP
  and the dormancy join; `req-mutation.sh` derives mutation scope from the capabilities a
  requirement is `covered_by`.
- `scripts/check-trailer.sh` — the ids become legal trailer targets. Eight merged commits currently
  point at REQ-PTT-01 for want of a better id (#1402's adjudication; corrected set in #1410).
- CLAUDE.md's acceptance table already carries the property in **three rows**, with tests.

## Prior art

- **No existing requirement states it.** Swept every statement for `key|PTT|transmitter`: the
  matches are REQ-PTT-01 ("PTT assert/release within 50 ms"), REQ-PHY-05 ("Transmitter release must
  occur within 50 ms of the last transmitted sample"), REQ-PHY-07/08 (which *backends* must exist),
  REQ-FX-06 (airtime-bounded bursts within the radio watchdog), REQ-CTL-02 (TX-keying fails closed
  for an unauthenticated client). **All are timing, backend inventory, or auth gating. None says a
  transmission must be keyed at all, or that no path may leave it keyed.**
- Note PTT-01 and PHY-05 are *both* 50 ms clauses — the control-path and audio-path halves, a split
  CLAUDE.md states explicitly. PTT-01 is therefore already narrow; this is not a case of an id being
  deliberately broad.
- The tests exist and run: `ptt_keys_every_transmit` (ardop 3, kiss 3), `ptt_keys_every_daemon_transmit`
  (2), `abnormal_exit_release` (1), `shared_ptt` (23) — **32 tests, all listed under the gate's
  `--no-default-features`**, so `enforced` is feasible rather than aspirational.

## Twins

- **REQ-PHY-05 is the twin to keep distinct.** It is deferred (#1112) because its audio half needs
  the rig. This proposal must not absorb it, or a deferred-and-honest gap becomes silently "covered".
- **REQ-CTL-02** already says TX-keying fails closed for an unauthenticated client — an adjacent
  *refusal* property. The new requirement is about keying when transmission is legitimate.
- **`openpulse-mesh`** had its audio route *removed* rather than guarded, precisely because it had
  no keying discipline and no station-ID timer. That is this property asserted by deletion.

## Prompt

Test this rather than confirm it. Two of my last three issue framings were too strong and were
corrected by looking at structure; assume this one is too until it survives.

### A. One requirement or two?

The eight commits cover two different failure modes:

1. **An emission is not keyed** → dead RF. Wasteful, not dangerous.
2. **An exit path leaves the transmitter keyed** → a stuck transmitter. Unsafe, and a §97.221
   problem on an unattended station.

`iterative-delivery`'s requirement-quality rule says *atomic — one testable assertion; compound
requirements hide gaps; split them*, and the acceptance table already treats them as separate rows.
So I lean **two**. Argue me out of it if one requirement with two clauses is how this registry
actually behaves elsewhere — I did not survey that.

### B. The capability question, which is the real problem

The emission paths span **five capabilities in five crates**: CAP-39 (ardop `bridge.rs`), CAP-40
(kiss), CAP-55 (daemon `server.rs`), CAP-47 (repeater), CAP-59 (`shared_ptt.rs`). Options:

1. `covered_by: [CAP-39, CAP-40, CAP-47, CAP-55, CAP-59]` — honest about where the code is, but
   gives the requirement a mutation scope spanning five crates, and #1405 has just shown what a
   scope wider than its bound tests produces.
2. `covered_by: [CAP-59]` alone — the property's *seam* is `SharedPtt`, and `cross-cutting-seams`
   says a cross-cutting concern belongs at the single shared seam. Narrow and checkable, but it
   would be false: the front-ends are where the property is actually violated, and four of the
   eight commits touched no `openpulse-radio` file.
3. **A new capability** — "PTT keying discipline on every emission path" — owning the seam plus the
   emission sites, satisfying the new requirement(s). Most honest, one more capability, and it
   overlaps files that CAP-39/40/47/55 already own (multi-ownership rises).

I lean (3) but I am not confident. Which of these does the registry's grain actually favour?

### C. Does registering it change any verdict today?

If `enforced`, `req-mutation.sh --all-enforced` gains it, and its scope under option (1) or (3)
includes `daemon/server.rs` (218 mutants) and `ardop/bridge.rs`. Given #1279 concluded the scheduled
job is not viable, that cost is currently theoretical — but the enforced set is also the thing
#1405 measures. **Does adding this make anything worse before it makes anything better?**

### D. Is this a requirement at all?

The sceptical reading: "every emission keys the transmitter" is an *implementation invariant* of
having a PTT at all, not a product requirement, and the right home is the acceptance table where it
already lives. I do not believe that — an unkeyed emission is externally observable as silence on
the air, and a stuck key is observable as a jammed channel — but the argument deserves a hearing
before the registry grows by two.

## Verdict

Reviewed 2026-09-19. **Add ONE requirement (keying-at-all), CORRECT one that already exists
(REQ-PTT-01), create NO new capability.**

**My central factual claim was false for half the property, and the falsity is a registry defect.**
I swept the yaml `statement:` fields; the **ratified prose** at `requirements.md:398-402` makes
REQ-PTT-01 the RAII release-on-scope-exit requirement. `f4c10467` (#872) created it that way;
`daa1676e` (#1098) wrote a traceability-matrix row paraphrasing it as "PTT assert/release within
50 ms" and citing the wrong test; `1da27abd` (#1117) imported that row into the yaml as the
statement. The importer was deleted (#1223), so the yaml became source of truth carrying the
paraphrase. The code agrees with the prose (`shared_ptt.rs:221,413,869`). Nothing checks a yaml
statement against its prose. **So the stuck-key half was registered all along.**

**Also live: a #1405 instance.** REQ-PTT-01 was `covered_by: [CAP-74]`, which does not own
`shared_ptt.rs` — the file its acceptance test lives in. Re-pointed to CAP-59.

**D — is it a requirement? ADOPT, with the argument replaced.** The survey I skipped says the
registry is *not* consistently externally-specified behaviour: every post-bright-line entry is
invariant-shaped (REQ-DCD-01 names the seam, REQ-SEC-13 names the clippy gate, REQ-RX-02/03 name the
mechanism). So the "implementation invariant, wrong home" objection has no footing. But my
"externally observable" argument is worthless — *every* defect is observable as silence. What
survives is the **hole between two existing ids**: PHY-07/08 say which backends must exist; nothing
said the configured one is used.

**A — two ids, but one is PTT-01 corrected, and NOT on atomicity.** Compound statements are this
registry's norm (CTL-02, SEC-14, CTL-04, DISC-04, QRM-01). My atomicity argument was imported from
`iterative-delivery` rather than drawn from this repo. The split stands on provenance, evidence tier
and mechanism instead.

**B — option (2), CAP-59. Options (1) and (3) rejected.** A new "keying discipline" capability owning
`bridge.rs` recreates precisely what #1399's maintainer decision measured and rejected, and rebuilds
the CAP-68 shape. CAP-59 is the registry's own precedent — the author twice added these tests to
`CAP-59.tests` — and mirrors REQ-SEC-13/CAP-77: the capability owns the seam, source scans hold the
sites.

**C — `enforced`, and the question was partly moot.** A new id *cannot* be `baseline`
(`NOT-GRANDFATHERED`, `trace.py:682`). My feasibility claim was overstated in the other direction:
the 32 tests pass, but none carried a `// VERIFIES:` — enforced is feasible *after* adding bindings,
which this change does.

**Corrections to the issue text:** ten commits carry `Implements: REQ-PTT-01`, not eight; "PTT-01 is
the only PTT requirement" is false; and CLAUDE.md's 50 ms row names REQ-PHY-05 only, so the
"control-path and audio-path halves" pairing was mine.

**Not done here, recorded instead:** CAP-59 mixes PTT and CAT and shares `rigctld.rs` with CAP-74 —
a curation item. And `68887bd5` (OTA send reports `PttFault` rather than `Delivered`) belongs to
neither id; it is about delivery reporting.
