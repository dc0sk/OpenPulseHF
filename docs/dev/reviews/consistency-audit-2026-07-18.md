---
doc: docs/dev/reviews/consistency-audit-2026-07-18.md
date: 2026-07-18
status: resolved
scope: docs, code, code-comments and tests — claim consistency + backlog/status doc consolidation
---

> **RESOLVED 2026-07-18** by PRs #954 (Type C/LZHUF residue sweep), #955 (acceptance-table repair and
> the two TX-only gates behind it), #956 (status-doc consolidation), #957 (six more vacuously-passing
> gates made real) and #958 (the tail: stale comments, wrong profile tables, dead type/test names).
> **All findings are addressed.** The highest-value one was not in the top ten: four doc-comments and
> two docs still documented a CE-SSB conclusion that had been *reversed* — the code excludes dense
> OFDM-HOM and SC-FDMA, and the test they cited to lock the old claim now asserts the opposite. Method: 7 parallel finders,
> one adversarial verifier per finding (refute-by-default), synthesis on a separate model.
> **51 findings, 51 survived verification, 0 refuted** — a 0% refutation rate is unusual and worth
> reading with care; for a *consistency* audit the claims are near-mechanically checkable ("this doc
> says X, the code says Y"), unlike the speculative mechanism claims in a bug hunt. The top findings
> were independently re-verified by hand before any fix was made.

# OpenPulseHF Documentation & Consistency Audit — Final Report

## (a) Executive summary

**The documentation is broadly truthful about capability, but poorly maintained about status.** Every finding that survived adversarial verification turned out to be a **documentation-only defect** — no finding altered a severity/confidence rating without also downgrading it, and in every single case the *code* was doing the right, safe thing (rejecting Type C cleanly, gating CE-SSB correctly, computing calibrated LLRs correctly). The pattern is not "the docs lie about what the system does" — it is "the docs lag behind what the system does," almost always in one direction: **completed/removed/fixed work that the status trackers still describe as pending, shipped, or broken.**

There is one real, structural problem: **LZHUF/Type C compression was deleted in PR #948 as a capability the project never actually had, and that correction did not propagate.** It is the single biggest cluster in this audit — touching README.md (three separate places, including a specific, false "Winlink Type C wire-compatible" claim), `docs/osi-layer-map.md`, `docs/dev/project/traceability-matrix.md`, `CLAUDE.md`'s own historical block, `docs/dev/project/changelog.md`'s dependency claim, `Cargo.toml`'s leftover manifest entry, and a stale doc-comment on the one function whose contract changed the most. This is the closest thing in the audit to a live "misleading claim," because README explicitly asserts wire-compatibility with a real external system (Winlink) that the project's own release notes say never existed.

Beyond that, the drift is the ordinary kind: a `backlog.md` that still lists two fully-shipped subsystems as "open work" and never updated seven completed items out of twelve; a `CLAUDE.md` that carries a 300-line completed execution plan verbatim; a hardening-audit report still marked `status: living` the day after its entire finding list closed; a few acceptance-table rows that cite tests that don't exist or won't parse as written; and a handful of test files whose *names* promise more than their *assertions* establish (the repo's own named failure mode, caught again here — most seriously in `bpsk_hardening.rs`, which is cited as the acceptance test for "BPSK loopback correctness" and never calls `receive()`).

None of this reached the "code is unsafe" bar. Given the base rate of this kind of audit, that is a genuinely good result — but the repo's own traceability discipline is exactly the reason to close these out; a false "shipped"/"removed"/"gated" claim compounds every time a future agent trusts it.

---

## (b) Doc consolidation plan

| Document | Disposition | Reason |
|---|---|---|
| `docs/dev/project/traceability.md` | **Keep — the single per-change ledger.** No changes needed; it was correct in every case checked. | Newest-first ledger with real pass/fail counts; the audit repeatedly found it was the *one* artifact that stayed accurate. |
| `docs/dev/project/traceability-matrix.md` | **Keep — the single capability/CAP-ID status source.** Fix CAP-41's design cell (LZHUF). | Correct for CAP-67/CAP-68 (backlog contradicted it, not the other way round); one stale cell (CAP-41). |
| `docs/dev/project/roadmap.md` | **Keep — the single phase-history source.** Fix the FF-15 heading typo (:1493 "A–F" → "A–G") and the RF-6 "Still open" stale items (:1758-1760). | Otherwise current (`last_updated: 2026-07-17`) and matches code; CLAUDE.md and backlog.md should point here instead of duplicating it. |
| `docs/dev/project/backlog.md` | **Keep as the living open-items register, but prune hard.** Delete items 1–7 (all shipped, already duplicated under "Recently completed"); delete/replace items 10–11 with one-line pointers to CAP-67/CAP-68; add one entry each for FF-15 Phase H and FF-16 Phase F (currently absent entirely). | It is the contract-designated home for deferred work (`docs/dev/project.md:18`) but is 58%+ dead weight and misses the two items that actually belong there. |
| `CLAUDE.md` | **Keep as the agent contract, but cut the "Execute Phase 1…" / Group 1-3 / Phase 2-5.7 blocks and the "Recently shipped (PRs #316-#321)" lists** down to a link to `roadmap.md`. Fix the 3 malformed acceptance-table commands, the LZHUF Phase 5.2 stale bullets, and the MFSK16 "ACK/ladder deferred" line. | ~45% of the file is a completed execution plan duplicating `roadmap.md`; its acceptance table is otherwise the strongest artifact in the repo and should stay authoritative — just corrected. |
| `docs/dev/reviews/winlink-stack-audit-2026-07-17.md` | **Flip `status: living` → `status: resolved`**, annotate each of the 10 findings with its fixing PR, per the convention already used by `2026-07-15-protocol-bridge-audit.md` and `2026-07-15-rx-decode-audit.md`. | Every finding closed (#943, #945–#951); the doc is the newest review artifact and the only one left at `living` with a fully-closed backlog. |
| `docs/dev/archive/backlog-fec-improvements.md`, `docs/dev/archive/backlog-waveforms.md` | **Archive-dir hygiene**: fix stale pre-move `doc:` paths; move their `docs/dev/README.md` index rows out of "Planning and release governance" into "Historical and review artifacts". Leave `status: living` (the repo's frontmatter validator rejects any other value). | Frozen research (2026-05-09) correctly labeled "Archived" in-body, but misfiled in the README's planning index next to the live backlog. |
| `docs/dev/onair-status.md` | **Keep as the single on-air status hub** (it already is — linked from 5+ docs). Fix the dead `branch:` frontmatter field, delete/strike the stale "Next steps: Step 1" (its own blocker is declared resolved 30 lines above), and fold in `backlog.md`'s duplicate 5-step on-air checklist. | Already the best-linked on-air doc; just needs its own internal self-contradiction and dead pointer cleaned up. |
| `docs/on-air_testplan.md`, `docs/regulatory-compliance-checklist.md` | **Stay separate.** | Operator-facing procedure/checklists, not status — correctly out of scope for consolidation. |
| `docs/dev/vara-parity-execution-board.md` | **Archive.** Items 1–8 are complete by its own footer; its only residue (on-air validation campaign) is already tracked in `onair-status.md`/`backlog.md`. | No live content left; a status board with nothing left to track should not stay `living`. |
| `README.md`, `docs/features.md` | **Stay separate** (user-facing front door / capability reference) but must be corrected in place: LZHUF row/mentions, `hpx_hf` SL range, `hpx_wideband_hd` table, `HPX2300`→`hpx_wideband` rename, `ArqSession` references. | These are what an operator actually reads; they should not become a second source of truth for ladder data — link `docs/mode-fec-ladder.md` (already gated) instead of re-deriving tables. |
| `docs/mode-fec-ladder.md` | **Keep as the single source for hpx_hf rung detail** (already gated by `ladder_doc_matches_profile`). Fix one stale prose number (:286, "30 dB" → "20 dB"). | Only doc with an automated consistency gate against `profile.rs`; everything else should defer to it, not duplicate it. |

---

## (c) Ranked top findings

1. **[confirmed, medium→high blast radius] LZHUF/Type-C removal (PR #948) never fully propagated** — README.md asserts "Winlink Type C wire-compatible" (:190), "Gzip and LZHUF compression" (:262, :406) three sections below its own banner (:42) retracting that exact claim as "a capability we never had"; the same drift recurs in `docs/osi-layer-map.md:20,45`, `traceability-matrix.md:233` (CAP-41), `CLAUDE.md:415-421` (test names that don't exist), `changelog.md:30` (dependency claim vs. `Cargo.toml:111` still declaring `oxiarc-lzhuf`), and `crates/openpulse-b2f/src/session.rs:297` (doc comment). **Fix:** one sweep — strike the row/mentions from README, `osi-layer-map.md`, correct CAP-41's design cell, delete the two dead test names from CLAUDE.md, delete the orphaned `Cargo.toml:111` line, fix the `session.rs:297` doc comment. Template already exists at `docs/features.md:518`.

2. **[confirmed, high] `bpsk_hardening.rs` — CLAUDE.md's acceptance test for "BPSK loopback correctness" never calls `receive()`.** 4 of 18 tests assert a literal `true`; two more discard an `is_ok() || is_err()` tautology; two have no assertions at all. It sits on line 1 of the acceptance table and is cited by `traceability-matrix.md:204` as CAP-12's evidence. **Fix:** re-point the acceptance row and CAP-12 at `psk31_longframe_acquisition.rs`/`bpsk_snr_tracks_a_fade.rs` (which do decode), which already exist — or add a real decode+compare to this file.

3. **[confirmed, high] Three CLAUDE.md acceptance-table commands are not runnable as written** — `cargo test -p openpulse-core --lib session_key ack::tests` (:525), `-p openpulse-radio --no-default-features cm108 gpio` (:521), `-p openpulse-mesh --test mesh_loopback impersonated_origin_rejected_at_relay authenticated_relay_forwarding` (:522) all fail with "unexpected argument" — cargo takes one positional TESTNAME. Gates E7 (ACK auth), REQ-PTT-02/03, and E3 (relay origin auth). **Fix:** move the extra filters after `--`, matching the working convention already used two rows below at :523/:524.

4. **[confirmed, medium→high] `docs/dev/project/backlog.md` designates two fully-shipped subsystems (Observability REQ-OBS-01..03; Control-channel security REQ-SEC-CTL-01..05) as "Open work items"**, one with an "open decision" (TLS-PSK) that was actively reversed. Compounded by 7 of 12 numbered items being duplicate shipped entries, and by FF-15 Phase H / FF-16 Phase F (the two *actually* open items) being entirely absent. **Fix:** per the consolidation table above — prune, point at CAP-67/CAP-68, add the two missing deferred entries.

5. **[confirmed, medium] `docs/dev/reviews/winlink-stack-audit-2026-07-17.md` is still `status: living`**, states "one real, live, unfixed DoS" and lists findings 2–10 as an open hardening backlog, the day after issue #942 closed and all ten items shipped (#943, #945–#951) — including a self-contradiction between its own lede (fixed) and body (unfixed). **Fix:** flip to `status: resolved`, annotate each row with its fixing PR.

6. **[confirmed, medium] The Winlink hardening arc (8 PRs, issue #942 closed) added zero rows to CLAUDE.md's acceptance table**, while every comparable prior security audit (relay E3, handshake freshness, SAR poison, ACK auth E7) did get rows — a real deviation from the repo's own convention, on the crate that talks to `cms.winlink.org:8772`. **Fix:** add 3–4 rows naming the real tests (`b2f_integration.rs`, `iss_failure_paths.rs`, `timeout_hardening.rs`, `cmd_hardening.rs`).

7. **[confirmed, medium] `hpx_hf` ladder documentation is stale in three independent places**: README's SL range (SL2–SL17 vs. real SL1–SL14, `README.md:242`), `profile.rs`'s own in-file comment table and prose (floors off by up to 10 dB from the executable `snr_floors`, `profile.rs:381-388,504-506`), and `mode-fec-ladder.md:286`'s prose ("30 dB" vs. its own correct table's "20 dB"). The `ladder_doc_matches_profile` gate only checks the `.md` table, so the comment and README drifted anyway. **Fix:** regenerate all three from `snr_floors`/`snr_ceilings`; consider gating the in-file comment too since that's what maintainers actually edit against.

8. **[confirmed, medium] `docs/features.md`'s per-profile tables are two profile revisions stale**: "HPX2300" (`:392-398`) names a constructor (`hpx2300()`) that no longer exists (renamed `hpx_wideband`, PR unknown/roadmap:509), and "HPX Wideband HD" (`:404-410`) lists SL12–SL14 as 64QAM500/1000/2000-RRC when the real `hpx_wideband_hd()` is SL9–SL15 SCFDMA26/52 → 64QAM2000-RRC (PR #320). **Fix:** regenerate both tables from `profile.rs`, or delete them and link `mode-fec-ladder.md`.

9. **[confirmed, low→medium] CLAUDE.md carries ~300 lines (45% of the file) of a fully-completed "Execute Phase 1 tasks in this order" plan** plus two "shipped (PRs #187-#321)" lists frozen ~630 PRs behind HEAD, duplicating the maintained `roadmap.md`. One heading ("Phase 2 — ✅ Partial") self-contradicts its own body (2.5/2.6/2.7 are all "✅ Done"). **Fix:** cut to Active tracks + Deferred + a roadmap.md link, per the consolidation table.

10. **[confirmed, medium] `qpsk_hardening.rs` — CLAUDE.md's "QPSK loopback correctness" gate is also TX-only** (`grep -c receive` = 0); its 42-scenario "matrix" test's pass condition compares its own loop counter against itself. Mitigated somewhat: real QPSK decode *is* covered elsewhere (`qpsk500_acquisition.rs`, `qpsk_differential_fading.rs`), just not by the file the table names. **Fix:** add a decode-and-compare step to the fixture, or retitle the acceptance row to point at a file that actually round-trips.

11. **[confirmed, medium] `CLAUDE.md`'s MFSK16 crate-map entry says "ACK/ladder deferred (PR-C/D)"** while MFSK16 is SL1 of the shipped `hpx_hf` ladder and its ACK path has a shipped, gated K=3 union-decoded return channel (`mfsk16_arq_subfloor.rs`) — the crate map contradicts the acceptance table three sections below it. **Fix:** replace with the shipped state and the two gate names.

12. **[confirmed, medium] Two doc-comments describe CE-SSB and dead-name a rejected test.** Four sites (linksim x3, `openpulse-config/src/lib.rs:433-435`) claim CE-SSB benefits "OFDM QPSK/8PSK" and SC-FDMA; the actual gate (`engine.rs:701-716`) excludes both entirely (measured 12/12→0/12 collapse). Separately, `docs/features.md:172-173` and `roadmap.md:844-845` cite a test, `cessb_benefits_hold_on_ofdm_hom`, that was renamed/reversed to `cessb_benefits_hold_on_low_order_ofdm_hom` and now asserts the *opposite* claim. **Fix:** align all sites with `engine.rs:668-700`'s own (correct) doc block.

13. **[confirmed, low] A cluster of test files whose names claim more than their assertions establish**: `repeater_integration.rs:200` (`relay_empty_buffer_returns_none` accepts any outcome), `tx_limiter.rs:15` (`limiter_bounds_peak_amplitude` never inspects an amplitude — the *only* engine-path coverage of FF-7's limiter), `fec_comparison.rs:171` (`fec_decision_gate` has zero assertions, prints a decision instead), `generic_cat_integration.rs:33` et al. (four `*_sends_correct_bytes` tests that never inspect `MockTransport.write_log`), and the PTT "≤50ms" acceptance row backed only by a `NoOpPtt` timer that cannot fail. **Fix:** per-file — drain the loopback backend / write_log and assert the named property, or `#[ignore]` + rename per the `scfdma_ce_sweep.rs` precedent already in the repo.

14. **[confirmed, low] Misc smaller drift**, each isolated and cheap: `docs/dev/design/js8-discovery-rendezvous-plan.md:570-576` cites 5 test targets that don't exist (mitigated — doc self-labels "plan only", equivalent coverage shipped under other names); `docs/features.md:506` cites a fabricated test name for a capability that *is* actually tested under two other names; README/osi-layer-map name a deleted `ArqSession` type (successor: `harq.rs`+`rate_policy.rs`); CLAUDE.md's crate map omits `apps/openpulse-twinview` and `tools/openpulse-dict-trainer`; `docs/features.md` omits the live Zstd+HPX-dictionary compression path entirely; `roadmap.md:1493`'s FF-15 heading says "A–F shipped" while its own body (4 lines below) and `CLAUDE.md`/`requirements.md` say "A–G"; `plugin.rs:150-153`'s LLR-calibration contract still lists 64QAM/BPSK/QPSK as uncalibrated after all three were calibrated; `engine.rs:3931-3933`'s doc-comment summary for `receive_with_llr_combining` describes a weighting stage PR #686 deleted (corrected 8 lines below in the same comment); `docs/dev/project/roadmap.md:1758-1760` still lists the ≤191B RsStrong lever and the SC/OFDM SNR-scale split as "still open," both closed by #941/#944.

---

## (d) Full detail by area

### doc-consolidation

- **[confirmed, medium]** `docs/dev/project/backlog.md:57-115` lists Observability (REQ-OBS-01..03) and Control-channel security (REQ-SEC-CTL-01..05) under "Open work items" though `traceability-matrix.md:259-308` (CAP-67/68) records all slices shipped, including a reversed design decision (TLS-PSK→Noise) the backlog still poses as "to confirm before coding" (`:106-108`).
- **[confirmed, medium]** `docs/dev/reviews/winlink-stack-audit-2026-07-17.md` — `status: living`, `:26` "one real, live, unfixed DoS," `:30` calls the rest "a reasonable hardening backlog," `:45` still recommends fixing/deleting the LZHUF path — all ten findings closed via #943/#945-#951, issue #942 CLOSED. Self-contradicts its own lede at `:12-13`.
- **[confirmed, low]** README.md:190/:262/:406, `docs/features.md:518` (correct), `docs/osi-layer-map.md:20/:45`, `crates/openpulse-core/src/plugin.rs` (n/a) — LZHUF simultaneously "removed in PR #948" and "Winlink Type C wire-compatible" across five living docs; `CLAUDE.md:393,415-421` self-contradicts (Phase 5.2 block: has a removal note at :417 but stale test names at :421 and a stale dependency claim at :416).
- **[confirmed, low]** `docs/dev/project/backlog.md:139-251` — 7 of 12 numbered "open" items (1-7) are `✅ Already shipped`/`✅ Shipped (PR #33x)`, 6 of them duplicated verbatim under "Recently completed (summary)" 100 lines below.
- **[confirmed, low]** `docs/dev/project/backlog.md:10` — FF-series enumeration stops at FF-13; FF-15 Phase H and FF-16 Phase F (the two genuinely open deferred items) have no backlog entry at all, tracked instead in CLAUDE.md/roadmap.md/changelog.md/two design plans.
- **[confirmed, low]** On-air validation tracked as an open checklist in five separate living docs (`backlog.md:211-240`, `onair-status.md` — dead `branch:` frontmatter field pointing at a deleted branch, plus a "Next steps: Step 1" contradicting its own "now-resolved" note four lines above — `vara-parity-execution-board.md`, `on-air_testplan.md`, `regulatory-compliance-checklist.md`). `onair-status.md` is already the hub (linked from 5+ docs); the dead frontmatter and self-contradiction are the real defect.
- **[confirmed, low]** `docs/dev/archive/backlog-fec-improvements.md:3-5` and `docs/dev/archive/backlog-waveforms.md:3-4` — stale pre-move `doc:` paths (both targets deleted two directory moves ago) and `status: living` on frozen research; misfiled in `docs/dev/README.md:15-17`'s "Planning and release governance" section instead of "Historical and review artifacts" (`:79`). Same pattern found repo-wide across the whole `docs/dev/archive/` directory, not unique to these two files.
- **[confirmed, low]** `CLAUDE.md:163-473` — a ~310-line completed "Execute Phase 1 tasks in this order" plan (every item `✅ Done`/`✅ Complete`), two resolved-decision blocks still headed as blocking ("`PttController` trait location (blocks Phase 1.5)" whose body says "Resolved and implemented"), and shipped-PR lists frozen at #321 while HEAD is #953 (though the LZHUF entries within them *were* actively kept current via #948 annotations — the list is maintained, just badly titled).

### claims-vs-code

- **[confirmed, medium]** `README.md:242` gives `hpx_hf` as SL2–SL17; the profile (`crates/openpulse-core/src/profile.rs:436-476`) is SL1–SL14 (omitting SL1/MFSK16, advertising three nonexistent top rungs). `docs/mode-fec-ladder.md:172` and `roadmap.md:1799` are correct. `profile.rs`'s own doc comments (`:350,353,355,435`) independently repeat the stale SL2–SL19/SL15-SL17 framing.
- **[confirmed, low]** `docs/features.md:392-398` documents an "HPX2300" profile whose constructor no longer exists (`by_name`/`PROFILE_NAMES` at `profile.rs:128-163` have no `hpx2300` entry — renamed to `hpx_wideband`, `roadmap.md:509`). Legitimate as a *bandwidth-class* term elsewhere (`hpx-waveform-design.md:162`, `regulatory.md:226`) — only the "adaptive profile" framing in features.md is wrong.
- **[confirmed, medium]** `docs/features.md:404-410` "HPX Wideband HD" table lists SL12-14 = 64QAM500/1000/2000-RRC; real `hpx_wideband_hd()` (`profile.rs:754-792`) is SL9-15 = SCFDMA26-{8PSK,16QAM,32QAM}→SCFDMA52-{16QAM,32QAM,64QAM}→64QAM2000-RRC. `README.md:251` and `mode-fec-ladder.md:178` are correct — the drift is local to features.md.
- **[confirmed, medium]** LZHUF claimed live in `docs/osi-layer-map.md:20,45`, `README.md:262,406,190`, `docs/features.md:661` (contradicts its own :518/:530 twelve lines away); code (`compress.rs:5`, `session.rs:235,321`) confirms full removal. `Cargo.toml:111` still declares the unconsumed `oxiarc-lzhuf` dependency.
- **[confirmed, low]** `docs/features.md:506` cites `sar_encode_fragment_reassemble_decode_roundtrip`, which does not exist anywhere in the repo — the real tests are `pq_conreq_serialized_size_fits_in_sar_capacity` and `sar_roundtrip_of_pq_conreq` (`pq_handshake_integration.rs:396,421`). The underlying capability *is* tested; only the citation is fabricated.
- **[confirmed, low]** `README.md:393` and `docs/osi-layer-map.md:47` name `ArqSession`/`arq_session` module, which does not exist (`crates/openpulse-modem/src/lib.rs` has no such module — logic lives in `harq.rs`+`rate_policy.rs`, as CLAUDE.md:140 itself already notes). README also has two more instances at `:176,:199` naming the deleted *file path* directly.
- **[confirmed, low]** `docs/features.md:514-518` omits the live `Zstd(u32)` + HPX-dictionary compression algorithm entirely (`compression.rs:7,26-27,93-111,128`); `README.md:391` correctly lists it. Section heading "### Session-layer LZ4" should become "### Session-layer compression."
- **[confirmed, low]** `CLAUDE.md`'s crate map omits two real workspace members: `apps/openpulse-twinview` (`Cargo.toml:36`, documented in `README.md:426`) and `tools/openpulse-dict-trainer` (`Cargo.toml:44`, documented in `docs/openpulse-manual.md:1131`) — the only 2 of 42 members missing from an otherwise-complete map.

### acceptance-table-vs-tests

- **[confirmed, medium]** The 8-PR Winlink hardening arc (#943, #945-#951; issue #942 CLOSED) added zero rows to CLAUDE.md's acceptance table (`:479-528`), unlike every comparable prior security fix (relay E3 `:522`, handshake freshness `:523`, SAR poison `:524`, ACK auth E7 `:525`) and unlike the immediately-preceding PR #944 (whose ledger entry explicitly notes "CLAUDE.md acceptance row").
- **[confirmed, medium]** Three acceptance rows (`CLAUDE.md:521,522,525`) do not run as written — cargo rejects a second bare positional test filter; must move extra filters after `--`, exactly as the working rows at `:523,:524` already do.
- **[confirmed, low]** `CLAUDE.md:501`'s "Every `hpx_hf` rung decodes on a … fade" claim is backed by `hpx_hf_rungs_survive_fade.rs:108`, which explicitly skips MFSK16 (`:113`) and all three LDPC-high-rate top rungs (`:119`) — 4 of 14 rungs — with a loose `checked >= 6` backstop (`:133`) that could let more silently drop out. The test's own doc comment (`:105`) is honest about the scope; only the acceptance-table wording overclaims.
- **[confirmed, low]** `CLAUDE.md:421` still cites `lzhuf_round_trip`/`lzhuf_bad_input_error`, deleted by PR #948; `:419` describes a deleted `decompress_lzhuf` cap.
- **[confirmed, low]** Two acceptance rows (`CLAUDE.md:487,513`) are phrased as unfulfilled TODOs — "(add test in `fec.rs`)" / "(add timing test in `noop.rs`)" — for tests that shipped (`fec.rs:1209`, `noop.rs:37`); contradicts the very next line's own rule ("Do not mark a task done if its test does not exist"). Three other rows (`:496,509,515`) share the same unfiltered-command shape but are not phrased as TODOs.

### code-comments-vs-code

- **[confirmed, medium]** CE-SSB doc-comments in `apps/openpulse-linksim/{lib.rs:246-247,gui.rs:1163-1164,main.rs:153}` and `crates/openpulse-config/src/lib.rs:433-435` claim CE-SSB benefits "OFDM QPSK/8PSK" and SC-FDMA; the real gate (`engine.rs:701-716`) excludes both entirely (only OFDM16/OFDM52 qualify) — the engine's own doc (`:668-700`) and the linksim's own regression test (`cessb_ab.rs:104-105`) state the correct rule three files away.
- **[confirmed, medium]** `docs/features.md:172-173` and `docs/dev/project/roadmap.md:840-845` cite a nonexistent test (`cessb_benefits_hold_on_ofdm_hom`) to lock a claim (CE-SSB helps dense OFDM-HOM) that was measured and reversed; the real test, `cessb_benefits_hold_on_low_order_ofdm_hom` (`cessb_power_evm.rs:221`), asserts the opposite, and `engine.rs:684-687` names the earlier claim as an error outright.
- **[confirmed, low]** `crates/openpulse-gateway/src/main.rs:330-334` — a doc comment describing a cooperative full round-trip is attached to the hostile-CMS-rejects-everything test three lines below it; the test it actually describes (`gateway_round_trip`, `:419`) has no doc comment at all.
- **[confirmed, low]** `crates/openpulse-b2f/src/session.rs:295-297` — `receive_data`'s doc still claims it "selects decompressor based on … proposal type (D=Gzip, C=LZHUF)"; the code unconditionally rejects Type C (`:317-322`). The crate's own module doc (`compress.rs:5`) already states the truth.
- **[confirmed, low]** `crates/openpulse-core/src/plugin.rs:150-153` still names 64QAM/BPSK/QPSK as noise-blind/uncalibrated after PR #687 calibrated all three (`64qam/demodulate.rs:578-581`, `qpsk/demodulate.rs:685,716`, `bpsk/demodulate.rs:376`); echoed at `fec.rs:544-546`; a self-contradicting stale parenthetical also survives inside 64QAM itself (`64qam/demodulate.rs:167`).
- **[confirmed, low]** `crates/openpulse-modem/src/engine.rs:3931-3933` — `receive_with_llr_combining`'s one-line summary claims "weight the resulting soft LLRs by inverse-noise-variance," which the same comment block retracts 8 lines later (PR #686 removed the weighting; the only combine is a plain MAP sum, `combine_llrs_map`).
- **[confirmed, low]** `CLAUDE.md:551-553`'s "one-line doc comment / no multi-paragraph docstrings" convention is contradicted by 49 doc blocks of 12+ lines in production crates (largest: `engine.rs:668`, 33 lines; `plugin.rs:129`, 32 lines) — deliberate, valuable rationale blocks the written rule doesn't actually govern, and which is where several of the stale claims above accumulated.

### cross-doc-contradictions

- **[confirmed, high]** README's compression/crate/protocol tables (`:190,262,406`) advertise LZHUF/Type C as shipped and wire-compatible, contradicting README's own v0.15.0 banner 148 lines earlier (`:42`) which retracts it as a capability the project never had.
- **[confirmed, medium]** `profile.rs:381-388`'s in-file comment ladder table disagrees with the executable `snr_floors` 130 lines below it by up to 10 dB per rung; `mode-fec-ladder.md:243` (table, correct) vs. `:286` (prose, stale "30 dB" vs. code's 20 dB) — same drift, third location.
- **[confirmed, medium]** README's adaptive-profiles table (`:242`, SL2–SL17) vs. the gated `roadmap.md:1799` (SL1–SL14) and code — a row previously fixed once already (per `roadmap.md:1027`, PR #838) and now drifted a second time.
- **[confirmed, medium]** `docs/features.md:408`'s "HPX Wideband HD" table (64QAM500/1000, SL12-14) vs. `profile.rs:754-766` and the correct `roadmap.md:1802`/`README.md:251` — an entire SCFDMA waveform family invisible in features.md.
- **[confirmed, low]** `docs/dev/project/changelog.md:30` claims `oxiarc-lzhuf` dependency removed; `Cargo.toml:111` still declares it (though it is absent from `Cargo.lock` and the SBOM, so the build-graph claim is true in substance, just not literally).
- **[confirmed, low]** `traceability-matrix.md:233` (CAP-41) still credits `compress_lzhuf_winlink` with "matching the Winlink Type C convention for real-CMS interop" — deleted function; the implementation cell (pointing at `compress.rs`, gzip-only) is fine, only the design cell is stale.
- **[confirmed, low]** `docs/osi-layer-map.md:20,45` places LZHUF at the presentation layer as a live codec and omits the live Zstd path.
- **[confirmed, low]** `roadmap.md:1493`'s FF-15 heading says "A–F shipped"; its own body 4 lines below (`:1497`) and `:1581` (Phase G complete) say "A–G," matching `requirements.md:246` and the design plan's frontmatter. Self-corrected within the same section — low impact. (Duplicate of the status-drift entry below; same root cause, single fix.)

### status-drift

- **[confirmed, high]** README still advertises LZHUF/Type C as shipped and "Winlink Type C wire-compatible" (`:190,262,406`) — the exact claim the v0.15.0 release banner (`:42`, same file) retracts as never having been true.
- **[confirmed, medium]** `CLAUDE.md:415-421` — Phase 5.2 ("LZHUF codec") still headed `✅ Done (PR #98)` with no removal annotation on the heading itself (only an inline sub-bullet at `:417`); still asserts the retracted `oxiarc-lzhuf` dependency and cites two tests (`lzhuf_round_trip`, `lzhuf_bad_input_error`) that don't exist. `roadmap.md:214` has the correct `✅ Done (PR #98) → ❌ REMOVED (PR #948)` heading form.
- **[confirmed, low]** `docs/dev/project/changelog.md:28-30` — same `oxiarc-lzhuf` dependency claim as above; `Cargo.toml:111` still declares it, though `Cargo.lock`/SBOM confirm it is out of the actual build graph.
- **[confirmed, medium]** `roadmap.md:1758-1760` — RF-6 arc's "Still open" list names two items (SC/OFDM SNR-scale split; RsStrong ≤191B lever) both closed by PR #941 (`free_rs_strengthening`, `fec.rs:150`) and PR #944 (`snr_scale_boundary.rs`) — inviting exactly the "unify the scales" regression `snr_scale_boundary.rs` exists to forbid.
- **[confirmed, medium]** `CLAUDE.md:116` — MFSK16 crate-map entry says "ACK/ladder deferred (PR-C/D)"; MFSK16 is SL1 of the shipped `hpx_hf` ladder (`profile.rs:440`) with a shipped, gated K=3-union ACK path (`mfsk16_arq_subfloor.rs`), and the deferral is recorded as explicitly RESOLVED elsewhere in the repo (`robust-narrowband-measurement.md:184`, `roadmap.md:2146`).
- **[confirmed, low]** `docs/dev/reviews/winlink-stack-audit-2026-07-17.md` — `status: living` the day after all 10 findings closed and issue #942 closed; internally self-contradicts its own lede.
- **[confirmed, low]** `CLAUDE.md:135`'s "Recently shipped (PRs #316–#321)" is ~630 PRs stale; largely mitigated because the genuinely-recent arcs (fade ladder, evidence climb, RsStrong, Winlink hardening) are covered elsewhere in CLAUDE.md (sharp edges, acceptance rows) or in `roadmap.md`, which this section already points to.
- **[confirmed, low]** `roadmap.md:1493` FF-15 heading "A–F shipped" vs. its own body ("A–G," 4 lines below) — duplicate of the cross-doc-contradictions entry above; fix once.

### test-integrity

- **[confirmed, high]** `crates/openpulse-modem/tests/bpsk_hardening.rs` — CLAUDE.md's "BPSK loopback correctness" gate never calls `receive()`; 4 tests assert `true`, 2 discard `is_ok()||is_err()`, 2 have no assertions. Real BPSK decode coverage exists elsewhere (`psk31_longframe_acquisition.rs`, `bpsk_snr_tracks_a_fade.rs`) but under different, uncited names.
- **[confirmed, medium]** `crates/openpulse-modem/tests/qpsk_hardening.rs` — CLAUDE.md's "QPSK loopback correctness" gate is also TX-only (`grep -c receive` = 0); its 42-scenario "matrix" test compares a loop counter to itself. Mitigated: real QPSK decode is covered by other, uncited files.
- **[confirmed, low]** `docs/dev/design/js8-discovery-rendezvous-plan.md:570-576` cites 5 test targets (`reference_vectors`, `js8_loopback`, `llr_reliability`, `hint_collision`, `js8_discovery_twin`) that don't exist under those names. Mitigated: the doc self-labels "plan only, nothing implemented," and equivalent coverage shipped under different names (`snr_sweep.rs`, `beacon_loopback.rs`, `rendezvous_end_to_end.rs`). JS8's `demodulate_soft` genuinely uses the trait's uncalibrated default (not in the HARQ-combining path), so the missing `llr_reliability` gate is real but lower-stakes than claimed.
- **[confirmed, low]** CLAUDE.md's "PTT assert/release ≤ 50 ms" row is backed by a timing test on `NoOpPtt` (`noop.rs:36`), which sets a bool and can never fail; two more equally-vacuous timing tests exist for `VoxPtt`/GPIO mocks (not caught by the original finding). The actual requirement (`requirements.md:40`) is about TX-pipeline turnaround, which nothing in the crate measures.
- **[confirmed, low]** `crates/openpulse-radio/tests/generic_cat_integration.rs` — 5 tests named `*_sends_correct_bytes`/`*_bcd_be` never inspect `MockTransport.write_log` (declared for exactly this purpose, `generic_cat.rs:43`); two have no assertion at all. Partially mitigated: byte-exact template-expansion IS unit-tested in `rig_definition.rs:239-269` — the real residual gap is that nothing confirms the per-rig `.toml` files still match those tested literals.
- **[confirmed, medium]** Three misc name/assertion mismatches: `repeater_integration.rs:200` (`relay_empty_buffer_returns_none` accepts any outcome, self-admitted in a comment), `tx_limiter.rs:15` (`limiter_bounds_peak_amplitude` never inspects amplitude — the *only* engine-path test of FF-7's limiter wiring), `iq_output.rs:15` (`bpsk_iq_output_lengths_match` never compares a length — mitigated, a sibling test in the same file does check it correctly).
- **[confirmed, low]** `crates/openpulse-core/tests/fec_comparison.rs:171` — `fec_decision_gate` has zero assertions (prints "ACCEPTED"/"REJECTED" and returns either way); its own doc comment concedes this. Contrasts with the repo's own precedent (`scfdma_ce_sweep.rs:4-8`) of banner-labeling such files as measurement harnesses rather than gates."
  },
  "workflowProgress": [
    {
      "type": "workflow_phase",
      "index": 1,
      "title": "Find"
    },
    {
      "type": "workflow_phase",
      "index": 2,
      "title": "Verify"
    },
    {
      "type": "workflow_phase",
      "index": 3,
      "title": "Synthesize"
    },
    {
      "type": "workflow_agent",
      "index": 1,
      "label": "find:doc-consolidation",
      "phaseIndex": 1,
      "phaseTitle": "Find",
      "agentId": "a3972d8c0385ae458",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366437995,
      "queuedAt": 1784366434168,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366676876,
      "tokens": 135898,
      "toolCalls": 22,
      "durationMs": 238881,
      "resultPreview": "{"findings":[{"title":"backlog.md still lists items 10 (Observability) and 11 (Control-channel security) as \"Open work items\" — the traceability matrix says both shipped","area":"doc-consolidation","file":"docs/dev/project/backlog.md:18","severity":"high","confidence":"high","evidence":"backlog.md:18-20 heads a section `## Open work items` / \"Ordered by priority. Items marked **[deferred]** hav…"
    },
    {
      "type": "workflow_agent",
      "index": 2,
      "label": "find:claims-vs-code",
      "phaseIndex": 1,
      "phaseTitle": "Find",
      "agentId": "a49999b148d9d782e",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366438944,
      "queuedAt": 1784366434168,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366740198,
      "tokens": 103454,
      "toolCalls": 40,
      "durationMs": 301253,
      "resultPreview": "{"findings":[{"title":"README's profile table gives hpx_hf the wrong SL range (SL2–SL17); the profile is SL1–SL14","area":"claims-vs-code","file":"README.md:242","severity":"high","confidence":"high","evidence":"README.md:242 — \"| `hpx_hf` | SL2–SL17 | SL2 | OFDM52-64QAM | Primary HF (full ≤2700 Hz span) |\". The actual profile in `crates/openpulse-core/src/profile.rs:359` (`pub fn hpx_hf()`) pop…"
    },
    {
      "type": "workflow_agent",
      "index": 3,
      "label": "find:acceptance-table-vs-tests",
      "phaseIndex": 1,
      "phaseTitle": "Find",
      "agentId": "a0630afbce3ce4db5",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366439034,
      "queuedAt": 1784366434169,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366693576,
      "tokens": 100757,
      "toolCalls": 32,
      "durationMs": 254542,
      "resultPreview": "{"findings":[{"title":"The entire Winlink hardening arc (8 PRs, issue #942 CLOSED) shipped with ZERO acceptance-table rows","area":"acceptance-table-vs-tests","file":"CLAUDE.md:479-528","severity":"high","confidence":"high","evidence":"`sed -n '479,529p' CLAUDE.md | grep -i \"b2f|winlink|gateway|driver\"` returns NOTHING — the acceptance table contains no row for openpulse-b2f, openpulse-b2f-drive…"
    },
    {
      "type": "workflow_agent",
      "index": 4,
      "label": "find:code-comments-vs-code",
      "phaseIndex": 1,
      "phaseTitle": "Find",
      "agentId": "af9723f1821ae70f1",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366438716,
      "queuedAt": 1784366434169,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366845040,
      "tokens": 121967,
      "toolCalls": 40,
      "durationMs": 406324,
      "resultPreview": "{"findings":[{"title":"CE-SSB comments across four crates still advertise 8PSK/SC-FDMA support that `cessb_benefits` explicitly excludes","area":"code-comments-vs-code","file":"apps/openpulse-linksim/src/lib.rs:247","severity":"high","confidence":"high","evidence":"`apps/openpulse-linksim/src/lib.rs:246-247`: \"CE-SSB TX envelope conditioning (default on, matching the engine). Only acts on the / m…"
    },
    {
      "type": "workflow_agent",
      "index": 5,
      "label": "find:cross-doc-contradictions",
      "phaseIndex": 1,
      "phaseTitle": "Find",
      "agentId": "aeacfba0c72d45a94",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366439214,
      "queuedAt": 1784366434169,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366817649,
      "tokens": 103321,
      "toolCalls": 31,
      "durationMs": 378435,
      "resultPreview": "{"findings":[{"title":"README's compression/crate/protocol tables still advertise LZHUF Type C as shipped and \"Winlink Type C wire-compatible\" — contradicted by README's own release paragraph 148 lines earlier","area":"cross-doc-contradictions","file":"README.md:190","severity":"critical","confidence":"high","evidence":"README.md:190 — `| **LZHUF / LH5** | B2F wire (Type C) | Both | `oxiarc-lzhu…"
    },
    {
      "type": "workflow_agent",
      "index": 6,
      "label": "find:status-drift",
      "phaseIndex": 1,
      "phaseTitle": "Find",
      "agentId": "a455ef5d36770194b",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366438571,
      "queuedAt": 1784366434169,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366666514,
      "tokens": 103137,
      "toolCalls": 32,
      "durationMs": 227943,
      "resultPreview": "{"findings":[{"title":"README still advertises the LZHUF/Type C codec as shipped and \"Winlink Type C wire-compatible\" — the exact claim v0.15.0 deleted as impossible, contradicting README's own release banner","area":"status-drift","file":"README.md:190","severity":"high","confidence":"high","evidence":"README.md:190 — `| **LZHUF / LH5** | B2F wire (Type C) | Both | `oxiarc-lzhuf`; 4-byte LE pre…"
    },
    {
      "type": "workflow_agent",
      "index": 7,
      "label": "find:test-integrity",
      "phaseIndex": 1,
      "phaseTitle": "Find",
      "agentId": "aafd0c1ba771bfd87",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366439163,
      "queuedAt": 1784366434169,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366795749,
      "tokens": 130021,
      "toolCalls": 37,
      "durationMs": 356586,
      "resultPreview": "{"findings":[{"title":"`bpsk_hardening` — CLAUDE.md's acceptance test for \"BPSK loopback correctness\" never demodulates anything, and four of its tests assert a literal `true`","area":"test-integrity","file":"crates/openpulse-modem/tests/bpsk_hardening.rs:49","severity":"critical","confidence":"high","evidence":"`grep -c receive crates/openpulse-modem/tests/bpsk_hardening.rs` → **0**. The file n…"
    },
    {
      "type": "workflow_agent",
      "index": 8,
      "label": "verify:README still advertises the LZHUF/Type C",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "a7f14cdadecdad640",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366672922,
      "queuedAt": 1784366669399,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366717391,
      "tokens": 63444,
      "toolCalls": 4,
      "durationMs": 44469,
      "resultPreview": "{"verdict":"confirmed","severity":"high","note":"CONFIRMED — all four cited README lines say exactly what the finding claims, and the contradiction is intra-file.\n\nRead: README.md:183-190 (the live \"Compression types\" table) still carries `| **LZHUF / LH5** | B2F wire (Type C) | Both | `oxiarc-lzhuf`; 4-byte LE prefix; Winlink Type C wire-compatible |`. README.md:262 lists \"Gzip and LZHUF com…"
    },
    {
      "type": "workflow_agent",
      "index": 9,
      "label": "verify:CLAUDE.md Phase 5.2 is still marked "✅ D",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "a1bffc55fa062a350",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366672981,
      "queuedAt": 1784366669400,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "Read CLAUDE.md:388-425 verbatim and docs/dev/project/roadma…",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366712393,
      "tokens": 62863,
      "toolCalls": 4,
      "durationMs": 39412,
      "resultPreview": "{"note":"Read CLAUDE.md:388-425 verbatim and docs/dev/project/roadmap.md:210-222; ran `grep -rnE \"lzhuf_round_trip|lzhuf_bad_input_error|fn compress_lzhuf|fn decompress_lzhuf|oxiarc\" crates/ plugins/ apps/ Cargo.lock` → ZERO hits, and `grep -c lzhuf crates/openpulse-b2f/src/compress.rs` → 0. The cited lines say exactly what the finding claims.\n\nCONFIRMED, with one correction to the finding's f…"
    },
    {
      "type": "workflow_agent",
      "index": 10,
      "label": "verify:The changelog says the `oxiarc-lzhuf` de",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "abd439e9bf5b4df27",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366678042,
      "queuedAt": 1784366669400,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366838326,
      "tokens": 69075,
      "toolCalls": 15,
      "durationMs": 160284,
      "resultPreview": "{"verdict":"confirmed","severity":"low","note":"CONFIRMED as to fact, DOWNGRADED medium->low because the stated impact is empirically false.\n\nWhat I read/ran:\n- Cargo.toml:111 — `oxiarc-lzhuf       = \"0.2.7\"` is present in [workspace.dependencies]. Verified verbatim.\n- Enumerated all 61 [workspace.dependencies] keys and tested each against every member manifest (`find . -name Cargo.toml -not…"
    },
    {
      "type": "workflow_agent",
      "index": 11,
      "label": "verify:The RF-6 roadmap arc's "Still open" list",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "a3225795e7b2f97ca",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366673678,
      "queuedAt": 1784366669400,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "{}",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366742079,
      "tokens": 67522,
      "toolCalls": 8,
      "durationMs": 68401,
      "resultPreview": "{"parameter":"{}","verdict":"confirmed","severity":"medium","note":"Read docs/dev/project/roadmap.md:1700-1790 (RF-6 arc section), its frontmatter (lines 1-6), crates/openpulse-core/src/fec.rs:140-160, and docs/dev/project/traceability.md; ran greps for #941/#944/RsStrong/snr_scale in roadmap.md and for free_rs_strengthening across crates/ and plugins/.\n\nCONFIRMED. The cited text at roadmap.md:1…"
    },
    {
      "type": "workflow_agent",
      "index": 12,
      "label": "verify:CLAUDE.md's MFSK16 crate-map entry says",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "a61a4e5cac6bb663b",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366673493,
      "queuedAt": 1784366669400,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366749410,
      "tokens": 70276,
      "toolCalls": 8,
      "durationMs": 75916,
      "resultPreview": "{"verdict":"confirmed","severity":"medium","note":"Read CLAUDE.md:116 verbatim — it does end \"Broadcast-first; ACK/ladder deferred (PR-C/D)\". Refuted the \"legitimately-frozen record\" defense: CLAUDE.md is the live authoritative agent contract, and the specific PR-C deferral it cites is marked RESOLVED elsewhere in the repo.\n\nEvidence, all read directly:\n(1) crates/openpulse-core/src/profile…"
    },
    {
      "type": "workflow_agent",
      "index": 13,
      "label": "verify:The Winlink audit doc (status: living) s",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "a78c0475dafc9aafc",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366673381,
      "queuedAt": 1784366669401,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366758299,
      "tokens": 76539,
      "toolCalls": 11,
      "durationMs": 84918,
      "resultPreview": "{"verdict":"confirmed","severity":"low","note":"Read docs/dev/reviews/winlink-stack-audit-2026-07-17.md directly. All cited text exists (line numbers off by ~4 from the finding's citations: \"one real, live, unfixed DoS\" is :26 not :30; \"Verdict: fix the proposal-Vec bug ... before anything else touches this stack\" is :30 not :34; finding 8's \"or delete and correct the shipped claim in docs\" …"
    },
    {
      "type": "workflow_agent",
      "index": 14,
      "label": "verify:CLAUDE.md's "Recently shipped" section t",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "ac62deee098b9a654",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366673591,
      "queuedAt": 1784366669401,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "{}",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366739667,
      "tokens": 71857,
      "toolCalls": 6,
      "durationMs": 66075,
      "resultPreview": "{"parameter":"{}","verdict":"confirmed","severity":"low","note":"Read CLAUDE.md:126-160 in full, grepped CLAUDE.md for #93x/#94x/#95x, checked `git log --oneline -12`, and read docs/dev/project/changelog.md:1-40 + grepped docs/dev/project/roadmap.md.\n\nCONFIRMED as stated on the literal facts. CLAUDE.md:135 `**Recently shipped (PRs #316–#321)**:` is the newest-labeled status block (followed by \"…"
    },
    {
      "type": "workflow_agent",
      "index": 15,
      "label": "verify:Roadmap's FF-15 heading says "A–F shippe",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "aa45ac72ff3f52d39",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366673842,
      "queuedAt": 1784366669401,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366712355,
      "tokens": 63940,
      "toolCalls": 5,
      "durationMs": 38513,
      "resultPreview": "{"verdict":"confirmed","severity":"low","note":"Read all four cited locations. roadmap.md:1493 reads verbatim `### FF-15 — JS8-based station discovery and rendezvous *(A–F shipped: ...; only H on-air remains)*`. The contradiction is INTERNAL to the same section, not just cross-doc: roadmap.md:1497, four lines below, says \"Phases A–G implemented, only H on-air remains\", and roadmap.md:1581 has an…"
    },
    {
      "type": "workflow_agent",
      "index": 16,
      "label": "verify:backlog.md still lists items 10 (Observa",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "a7bac4d9d1159f6b8",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366681562,
      "queuedAt": 1784366679230,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "CONFIRMED against source. backlog.md:1-6 is `status: living…",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366740995,
      "tokens": 81842,
      "toolCalls": 6,
      "durationMs": 59432,
      "resultPreview": "{"note":"CONFIRMED against source. backlog.md:1-6 is `status: living` (last_updated 2026-07-08), so the frozen-historical-record exemption does not apply. backlog.md:18 heads `## Open work items`; :57-82 carries item 10 (Observability, REQ-OBS-01..03) with a forward-looking scope list and a present-tense \"traces vanish on restart (no on-disk log)\"; :84-115 carries item 11 (Control-channel securi…"
    },
    {
      "type": "workflow_agent",
      "index": 17,
      "label": "verify:The Winlink audit doc is still `status:",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "ab8e0b86e4f92038d",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366698137,
      "queuedAt": 1784366679230,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366785487,
      "tokens": 70181,
      "toolCalls": 9,
      "durationMs": 87349,
      "resultPreview": "{"verdict":"confirmed","severity":"medium","note":"Read docs/dev/reviews/winlink-stack-audit-2026-07-17.md in full header+table, all four sibling audit docs' frontmatter, docs/dev/project/traceability.md fix entries, `gh issue view 942`, and grepped the actual source.\n\nCONFIRMED, with one detail the finding missed and a severity downgrade.\n\nSubstantiated: frontmatter is `status: living` / `las…"
    },
    {
      "type": "workflow_agent",
      "index": 18,
      "label": "verify:LZHUF/Type C is simultaneously "removed",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "a2c8ed666438e199e",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366717231,
      "queuedAt": 1784366679230,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366785673,
      "tokens": 70164,
      "toolCalls": 4,
      "durationMs": 68442,
      "resultPreview": "{"verdict":"confirmed","severity":"medium","note":"Verified by grepping the tree and reading every cited line. Code deletion is real: no compress_lzhuf* symbol exists anywhere under crates/ (compress.rs:5 says \"Type C (LZHUF) is not supported... was removed\"; session.rs:235/:321 answer Reject). The stale claims are real and verbatim: README.md:190 still asserts \"Winlink Type C wire-compatible\"…"
    },
    {
      "type": "workflow_agent",
      "index": 19,
      "label": "verify:Seven of backlog.md's twelve numbered en",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "a1064fd80e7a42438",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366717868,
      "queuedAt": 1784366679231,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366764837,
      "tokens": 67949,
      "toolCalls": 4,
      "durationMs": 46969,
      "resultPreview": "{"verdict":"confirmed","severity":"low","note":"Read docs/dev/project/backlog.md in full (261 lines) plus git history. All cited line numbers verify exactly: `## Open work items` (:18) is followed by items 8,9,10,11,12 (:22,:39,:57,:84,:117) and THEN items 1-7 (:139,:148,:160,:171,:180,:189,:197), each of which is self-labelled `✅ Already shipped` or `✅ Shipped (PR #33x)`. Line :144 does read \"No…"
    },
    {
      "type": "workflow_agent",
      "index": 20,
      "label": "verify:The two genuinely-open deferred phases (",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "a99bc21362769113b",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366721672,
      "queuedAt": 1784366679231,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366770824,
      "tokens": 65232,
      "toolCalls": 6,
      "durationMs": 49152,
      "resultPreview": "{"verdict":"confirmed","severity":"low","note":"Read /home/dc0sk/git/OpenPulseHF/docs/dev/project/backlog.md in full (261 lines), /home/dc0sk/git/OpenPulseHF/docs/dev/project.md, and grepped roadmap.md.\n\nThe core factual claim holds:\n- docs/dev/project.md:18 designates backlog.md as \"Parked / deferred work items not yet scheduled\" — so it is the contract-designated home for deferred work.\n- …"
    },
    {
      "type": "workflow_agent",
      "index": 21,
      "label": "verify:On-air validation is tracked as an open",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "ad95d20877e09f054",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366745006,
      "queuedAt": 1784366679231,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366820982,
      "tokens": 77328,
      "toolCalls": 7,
      "durationMs": 75976,
      "resultPreview": "{"verdict":"confirmed","severity":"low","note":"Read all five cited docs in full or at the cited lines, plus `git branch -a`, a `grep -rn \"onair-status\" docs`, a `grep -rn \"^branch:\" docs`, and a run of scripts/validate-doc-frontmatter.sh.\n\nCONFIRMED sub-claims:\n- docs/dev/onair-status.md:4 `branch: fix/afc-settling-receive-with-timeout` — that branch does not exist (git branch -a: main, gh…"
    },
    {
      "type": "workflow_agent",
      "index": 22,
      "label": "verify:Both archived backlogs declare `status:",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "a988c9e55ca131fd1",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366744739,
      "queuedAt": 1784366679231,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366815604,
      "tokens": 68795,
      "toolCalls": 8,
      "durationMs": 70864,
      "resultPreview": "{"verdict":"confirmed","severity":"low","note":"FACTS — all verified verbatim, every cited line is exactly as quoted:\n- docs/dev/archive/backlog-fec-improvements.md:3-5 = `doc: docs/backlog-fec-improvements.md` / `status: living` / `last_updated: 2026-05-09`; :10 = \"**Frozen research summary (2026-05-09).** Archived; see docs/dev/project/backlog.md for live open work.\"\n- docs/dev/archive/backl…"
    },
    {
      "type": "workflow_agent",
      "index": 23,
      "label": "verify:CLAUDE.md carries a full duplicate execu",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "a1f59cf72dc7971f7",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366745466,
      "queuedAt": 1784366679231,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "Read CLAUDE.md in full (684 lines) plus roadmap.md frontmat…",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366794511,
      "tokens": 67566,
      "toolCalls": 6,
      "durationMs": 49045,
      "resultPreview": "{"note":"Read CLAUDE.md in full (684 lines) plus roadmap.md frontmatter. The substance holds, though the finding's line numbers are off by ~43.\n\nCONFIRMED:\n- CLAUDE.md:163 — \"Execute Phase 1 tasks in this order. Tasks within the same group are independent and may be parallelised.\" is a live imperative followed by ~310 lines (:165–:473, 45% of the file) in which every entry is complete. `grep`…"
    },
    {
      "type": "workflow_agent",
      "index": 24,
      "label": "verify:The entire Winlink hardening arc (8 PRs,",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "a962111c1870235b1",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366748310,
      "queuedAt": 1784366695880,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366820269,
      "tokens": 77242,
      "toolCalls": 8,
      "durationMs": 71959,
      "resultPreview": "{"verdict":"confirmed","severity":"medium","note":"Read CLAUDE.md:475-528 in full (all 48 acceptance rows) — the cited line range is accurate and there is genuinely NO row for openpulse-b2f, -b2f-driver, or -gateway; the grep for b2f/winlink/gateway hits only the crate map (:84-86), PR-history lists (:146,:149) and the Phase 4.4/5.1/5.2/5.5 Done entries (:388-453), never inside the table.\n\nVerif…"
    },
    {
      "type": "workflow_agent",
      "index": 25,
      "label": "verify:Three acceptance commands do not run as",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "a46364ef8f67daff9",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366755120,
      "queuedAt": 1784366695880,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366851728,
      "tokens": 65400,
      "toolCalls": 8,
      "durationMs": 96608,
      "resultPreview": "{"verdict":"confirmed","severity":"medium","note":"Read CLAUDE.md:515-530 and ran all three commands verbatim. Every element of the claim checks out.\n\nCITED LINES MATCH: CLAUDE.md:521 (`cargo test -p openpulse-radio --no-default-features cm108 gpio`), :522 (`--test mesh_loopback impersonated_origin_rejected_at_relay authenticated_relay_forwarding`), :525 (`--lib session_key ack::tests`) contain …"
    },
    {
      "type": "workflow_agent",
      "index": 26,
      "label": "verify:"Every hpx_hf rung decodes on a fade" —",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "a475ba8ebb1866369",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366762933,
      "queuedAt": 1784366695880,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366827659,
      "tokens": 69293,
      "toolCalls": 6,
      "durationMs": 64726,
      "resultPreview": "{"verdict":"confirmed","severity":"low","note":"Verified against source. CLAUDE.md:501 row text is verbatim as quoted (\"Every `hpx_hf` rung decodes on a Watterson `moderate_f1` fade...\"). crates/openpulse-modem/tests/hpx_hf_rungs_survive_fade.rs:113 skips `mode == \"MFSK16\"` and :119 skips `fec_for(level) == LdpcHighRate`; backstop at :133 is `assert!(checked >= 6)`. profile.rs:116-125 (`define…"
    },
    {
      "type": "workflow_agent",
      "index": 27,
      "label": "verify:CLAUDE.md still lists two LZHUF integrat",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "a6d2860e3d2766525",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366769107,
      "queuedAt": 1784366695880,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366845360,
      "tokens": 68471,
      "toolCalls": 8,
      "durationMs": 76252,
      "resultPreview": "{"verdict":"confirmed","severity":"low","note":"CONFIRMED on the core claim, severity downgraded medium -> low.\n\nWhat I read/ran:\n1. `sed -n '380,430p' CLAUDE.md` — line 421 reads verbatim: \"- Integration tests: `lzhuf_round_trip`, `lzhuf_bad_input_error`\". Line 419 reads \"`decompress_lzhuf`: caps `orig_len` at 16 MiB to prevent OOM from malformed frames\".\n2. `grep -rn \"lzhuf_round_trip\\…"
    },
    {
      "type": "workflow_agent",
      "index": 28,
      "label": "verify:Two acceptance rows still read as unfulf",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "af5947efad4236391",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366775469,
      "queuedAt": 1784366695881,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366833869,
      "tokens": 66706,
      "toolCalls": 7,
      "durationMs": 58399,
      "resultPreview": "{"verdict":"confirmed","severity":"low","note":"Read CLAUDE.md:478-530 (the full acceptance table plus the rule line that follows it), crates/openpulse-core/src/fec.rs:1205-1245, and crates/openpulse-radio/src/noop.rs:28-52.\n\nCORE CLAIM CONFIRMED. CLAUDE.md:487 and :513 read verbatim as quoted, and both are phrased as imperative TODOs (\"(add test in `fec.rs`)\", \"(add timing test in `noop.rs`)…"
    },
    {
      "type": "workflow_agent",
      "index": 29,
      "label": "verify:README's profile table gives hpx_hf the",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "a9f3466dc614c1550",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366794434,
      "queuedAt": 1784366742606,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366855295,
      "tokens": 72074,
      "toolCalls": 9,
      "durationMs": 60861,
      "resultPreview": "{"verdict":"confirmed","severity":"medium","note":"CONFIRMED — the cited line says exactly what the finding claims, and the code contradicts it.\n\nWhat I read:\n- README.md:242 — `| `hpx_hf` | SL2–SL17 | SL2 | OFDM52-64QAM | Primary HF (full ≤2700 Hz span) |`. Verbatim match to the finding's quote.\n- crates/openpulse-core/src/profile.rs:436-476 — `hpx_hf()` allocates `let mut modes = [None; 21]`…"
    },
    {
      "type": "workflow_agent",
      "index": 30,
      "label": "verify:docs/features.md documents an "HPX2300"",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "a965106e3d3580f02",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366795557,
      "queuedAt": 1784366742606,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "Read docs/features.md:392 (heading "**HPX2300** (wideband, …",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366872154,
      "tokens": 73146,
      "toolCalls": 8,
      "durationMs": 76597,
      "resultPreview": "{"note":"Read docs/features.md:392 (heading \"**HPX2300** (wideband, ~2300 Hz occupied bandwidth):\" + rung table SL8 QPSK500 / SL9 QPSK1000 / SL11 8PSK1000) and docs/features.md:66 (\"QPSK1000 (used in the HPX2300 adaptive profile)\"). Verified against crates/openpulse-core/src/profile.rs:128-141 (PROFILE_NAMES — twelve names, no hpx2300) and profile.rs:146-163 (by_name — no \"hpx2300\" arm, retu…"
    },
    {
      "type": "workflow_agent",
      "index": 31,
      "label": "verify:LZHUF/Type C support is still claimed in",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "aac02153315c3fbd9",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366802614,
      "queuedAt": 1784366742606,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "CONFIRMED, severity downgraded high -> medium; scope is BRO…",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366881382,
      "tokens": 70313,
      "toolCalls": 7,
      "durationMs": 78768,
      "resultPreview": "{"note":"CONFIRMED, severity downgraded high -> medium; scope is BROADER than reported.\\n\\nCode verified: crates/openpulse-b2f/src/compress.rs:5 documents the LH5 removal; session.rs:96 hardcodes ProposalType::D in the only surviving queue fn (queue_message; queue_message_type_c is gone); session.rs:240 answers ProposalType::C => FsAnswer::Reject; session.rs:319-321 errors \"proposal type C (LZH…"
    },
    {
      "type": "workflow_agent",
      "index": 32,
      "label": "verify:features.md cites an integration test by",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "ad217d8ca15492070",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366803626,
      "queuedAt": 1784366742607,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366854874,
      "tokens": 62690,
      "toolCalls": 7,
      "durationMs": 51248,
      "resultPreview": "{"verdict":"confirmed","severity":"low","note":"Facts fully substantiated; severity downgraded high -> low.\n\nVERIFIED: (1) `grep -rn \"sar_encode_fragment_reassemble\" .` over the entire repo returns exactly one hit -- docs/features.md:506 itself, no test anywhere. (2) Ran the citation: `cargo test -p openpulse-core --no-default-features --test pq_handshake_integration sar_encode_fragment_reasse…"
    },
    {
      "type": "workflow_agent",
      "index": 33,
      "label": "verify:features.md's Wideband-HD rung table is",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "acc7532126d87809f",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366820861,
      "queuedAt": 1784366742607,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366871397,
      "tokens": 65322,
      "toolCalls": 8,
      "durationMs": 50536,
      "resultPreview": "{"verdict":"confirmed","severity":"medium","note":"CONFIRMED on the substance, with a small line-number correction and a severity downgrade.\n\nWhat I read:\n- /home/dc0sk/git/OpenPulseHF/docs/features.md:404-410 — the actual location of the table (the finding cited :413/:413-419, which is a few lines off; the header \"**HPX Wideband HD** (full SSB passband, up to 2700 Hz; 64QAM)\" is line 404 and…"
    },
    {
      "type": "workflow_agent",
      "index": 34,
      "label": "verify:README and osi-layer-map name an `ArqSes",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "a437d24749d92d85e",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366822124,
      "queuedAt": 1784366742607,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366864026,
      "tokens": 64092,
      "toolCalls": 4,
      "durationMs": 41902,
      "resultPreview": "{"verdict":"confirmed","severity":"low","note":"Verified by reading the cited lines and re-running the grep (the finding's own grep had failed under zsh with \"no matches found: --include=*.rs\", so its evidence was accidentally vacuous — but the claim holds anyway).\n\nCONFIRMED: `grep -rn \"ArqSession\\|arq_session\" crates apps plugins` returns zero hits. `crates/openpulse-modem/src/lib.rs:6-15…"
    },
    {
      "type": "workflow_agent",
      "index": 35,
      "label": "verify:features.md's compression section omits",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "a408a70f502b5bd52",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366826289,
      "queuedAt": 1784366742607,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366867685,
      "tokens": 64306,
      "toolCalls": 5,
      "durationMs": 41396,
      "resultPreview": "{"verdict":"confirmed","severity":"low","note":"Read docs/features.md:510-528 (Data Compression chapter): the algorithm table at 516-518 lists only LZ4, Gzip, and struck-through LZHUF LH5; heading 520 is \"### Session-layer LZ4\"; line 522 describes compress_if_smaller. Ran `grep -ni zstd docs/features.md` -> ZERO matches in the whole file, so the omission is total. Against code: crates/openpulse-…"
    },
    {
      "type": "workflow_agent",
      "index": 36,
      "label": "verify:CLAUDE.md's crate map omits two workspac",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "afc541b5660a0d020",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366826552,
      "queuedAt": 1784366742607,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366862721,
      "tokens": 64444,
      "toolCalls": 5,
      "durationMs": 36169,
      "resultPreview": "{"verdict":"confirmed","severity":"low","note":"Verified every element of the claim directly.\n\n1. Workspace members (root `Cargo.toml`): the cited lines are exact — `Cargo.toml:36` is `\"apps/openpulse-twinview\"` and `Cargo.toml:44` is `\"tools/openpulse-dict-trainer\"`. Both directories exist on disk (`apps/openpulse-twinview/`, `tools/openpulse-dict-trainer/`).\n\n2. CLAUDE.md omission: `grep…"
    },
    {
      "type": "workflow_agent",
      "index": 37,
      "label": "verify:`bpsk_hardening` — CLAUDE.md's acceptanc",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "ac7f35eec8f7045fc",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366832934,
      "queuedAt": 1784366799850,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366871824,
      "tokens": 65995,
      "toolCalls": 4,
      "durationMs": 38890,
      "resultPreview": "{"verdict":"confirmed","severity":"high","note":"Read all 255 lines of crates/openpulse-modem/tests/bpsk_hardening.rs and grepped the three citing docs. Every specific claim verifies: grep -c receive → 0 across 18 tests; the fixture (:17-44) has no RX method at all and never drains LoopbackBackend; the header (:3-6) advertises an SNR sweep, multipath profiles and error recovery, none of which exis…"
    },
    {
      "type": "workflow_agent",
      "index": 38,
      "label": "verify:`qpsk_hardening` — CLAUDE.md's "QPSK loo",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "aa82bd922853b3ab9",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366838980,
      "queuedAt": 1784366799850,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366901760,
      "tokens": 66787,
      "toolCalls": 5,
      "durationMs": 62780,
      "resultPreview": "{"verdict":"confirmed","severity":"medium","note":"Read crates/openpulse-modem/tests/qpsk_hardening.rs in full (196 lines) plus CLAUDE.md:482 and docs/dev/project/traceability-matrix.md:205.\n\nEvery cited line is accurate. `grep -c receive` on the file returns 0 — there is no receive/demodulate call anywhere; the only two non-transmit tests (:128, :169) call plugin.modulate() and compare samples.…"
    },
    {
      "type": "workflow_agent",
      "index": 39,
      "label": "verify:JS8 plan §10.5 acceptance table names fo",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "a7e2b266421df84d9",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366847644,
      "queuedAt": 1784366799850,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "VERIFIED the literal claim; REFUTED the framing and severit…",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366920548,
      "tokens": 70233,
      "toolCalls": 7,
      "durationMs": 72903,
      "resultPreview": "{"note":"VERIFIED the literal claim; REFUTED the framing and severity.\n\nWhat I read:\n- docs/dev/design/js8-discovery-rendezvous-plan.md:565-577 (§10.5 table) and :583-600 (§11 phase table) — the five cited targets are there verbatim.\n- `ls plugins/js8/tests/ crates/openpulse-discovery/tests/ crates/openpulse-daemon/tests/` → js8: snr_estimate.rs, snr_sweep.rs; discovery: beacon_loopback.rs, re…"
    },
    {
      "type": "workflow_agent",
      "index": 40,
      "label": "verify:The "PTT assert/release ≤ 50 ms" accepta",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "a1abdeb6ec0cb2a72",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366850819,
      "queuedAt": 1784366799850,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366920593,
      "tokens": 66191,
      "toolCalls": 8,
      "durationMs": 69774,
      "resultPreview": "{"verdict":"confirmed","severity":"low","note":"Read CLAUDE.md:513, crates/openpulse-radio/src/{noop.rs,vox.rs,gpio.rs}, docs/dev/requirements.md:40, and ran `cargo test -p openpulse-radio --no-default-features`.\n\nCONFIRMED core claim: (a) CLAUDE.md:513 still carries the leftover instruction \"(add timing test in `noop.rs`)\" as though the test were absent; (b) the gate is vacuous — noop.rs:16-2…"
    },
    {
      "type": "workflow_agent",
      "index": 41,
      "label": "verify:Four `generic_cat` tests named `*_sends_",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "a94db3a9f9b3e38ab",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366850119,
      "queuedAt": 1784366799850,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366918729,
      "tokens": 69953,
      "toolCalls": 8,
      "durationMs": 68610,
      "resultPreview": "{"verdict":"confirmed","severity":"low","note":"Core claim verified by reading crates/openpulse-radio/tests/generic_cat_integration.rs in full plus src/generic_cat.rs and src/rig_definition.rs.\n\nCONFIRMED: grep for `write_log` in the integration test file returns zero hits; the symbol exists only at generic_cat.rs:43 (declaration, doc-commented \"inspectable by tests\"), :51, :59. `icom_ic7300_p…"
    },
    {
      "type": "workflow_agent",
      "index": 42,
      "label": "verify:Three tests whose names assert a specifi",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "ace09a9f69ccc9994",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366860717,
      "queuedAt": 1784366799850,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "Read all three cited sites verbatim; all three bodies are a…",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366904147,
      "tokens": 66416,
      "toolCalls": 7,
      "durationMs": 43430,
      "resultPreview": "{"note":"Read all three cited sites verbatim; all three bodies are as described. (1) tx_limiter.rs:13-29 — doc comment \"the audio written to the backend stays within threshold\", body sets set_tx_limiter_threshold(0.5), transmits, receives, drop(rx); no amplitude inspected. Confirmed this is the ONLY engine-path coverage: grep for tx_limiter|tanh_limit|limiter_threshold shows the bounding propert…"
    },
    {
      "type": "workflow_agent",
      "index": 43,
      "label": "verify:`fec_decision_gate` is named a gate and",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "a7685d077c45e963c",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366869339,
      "queuedAt": 1784366799850,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366921477,
      "tokens": 65654,
      "toolCalls": 4,
      "durationMs": 52138,
      "resultPreview": "{"verdict":"confirmed","severity":"low","note":"Read crates/openpulse-core/tests/fec_comparison.rs in full and ran `cargo test -p openpulse-core --no-default-features --test fec_comparison -- --nocapture`.\n\nMECHANICAL CLAIMS ALL VERIFIED:\n- fec_decision_gate() at :171 has ZERO assertions across its 48-line body; terminal statement is the if/else println! branch at :207-218 (both arms print and …"
    },
    {
      "type": "workflow_agent",
      "index": 44,
      "label": "verify:README's compression/crate/protocol tabl",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "a37152a8b4b9f32ac",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366864081,
      "queuedAt": 1784366819828,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366906742,
      "tokens": 63971,
      "toolCalls": 7,
      "durationMs": 42660,
      "resultPreview": "{"verdict":"confirmed","severity":"high","note":"Verified all cited lines verbatim. README.md:190 is a live present-tense \"Compression types\" table row: `| **LZHUF / LH5** | B2F wire (Type C) | Both | `oxiarc-lzhuf`; 4-byte LE prefix; Winlink Type C wire-compatible |` — \"Direction: Both\" asserts encode and decode. README.md:262 (\"Gzip and LZHUF compression\") and README.md:406 (\"Gzip (Type D…"
    },
    {
      "type": "workflow_agent",
      "index": 45,
      "label": "verify:`hpx_hf`'s SNR-floor column in the profi",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "abe206f4a4e7bba67",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366867223,
      "queuedAt": 1784366819828,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366907954,
      "tokens": 67509,
      "toolCalls": 5,
      "durationMs": 40731,
      "resultPreview": "{"verdict":"confirmed","severity":"medium","note":"All cited file:line locations verified verbatim. profile.rs:381-388 comment table gives SL7-SL14 floors 10/14/16/17/22/23/24/30; the executable snr_floors at profile.rs:515-522 are 9/10/12/14/16/18/19/20 — all eight OFDM rungs disagree, SL14 by 10 dB. The snr_ceilings block at :532-539 is self-consistent with the CODE floors (floor(SL7)=9 ... floo…"
    },
    {
      "type": "workflow_agent",
      "index": 46,
      "label": "verify:README's adaptive-profiles table gives `",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "a546ea4588df295e7",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366872386,
      "queuedAt": 1784366819828,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366919862,
      "tokens": 65866,
      "toolCalls": 6,
      "durationMs": 47476,
      "resultPreview": "{"verdict":"confirmed","severity":"medium","note":"Confirmed against all three cited sources. README.md:242 reads verbatim `| `hpx_hf` | SL2–SL17 | SL2 | OFDM52-64QAM | Primary HF (full ≤2700 Hz span) |`; docs/dev/project/roadmap.md:1799 reads `| `hpx_hf` | SL1–SL14 | SL2 | OFDM52-64QAM |`. Code settles it: crates/openpulse-core/src/profile.rs `hpx_hf()` assigns modes[Sl1]=\"MFSK16\" (fec_modes[Sl…"
    },
    {
      "type": "workflow_agent",
      "index": 47,
      "label": "verify:features.md's "HPX Wideband HD" table li",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "a56c9eb45134ad801",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366873293,
      "queuedAt": 1784366819828,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366926548,
      "tokens": 65979,
      "toolCalls": 7,
      "durationMs": 53255,
      "resultPreview": "{"verdict":"confirmed","severity":"medium","note":"Read docs/features.md:404-410 (verbatim match to the finding: SL12=64QAM500, SL13=64QAM1000, SL14=64QAM2000-RRC under \"HPX Wideband HD\") against crates/openpulse-core/src/profile.rs:754-766, which sets SL9=SCFDMA26-8PSK, SL10=SCFDMA26-16QAM, SL11=SCFDMA26-32QAM, SL12=SCFDMA52-16QAM, SL13=SCFDMA52-32QAM, SL14=SCFDMA52-64QAM, SL15=64QAM2000-RRC. G…"
    },
    {
      "type": "workflow_agent",
      "index": 48,
      "label": "verify:changelog.md claims the `oxiarc-lzhuf` d",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "ac7d6eab9be2a790e",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366876411,
      "queuedAt": 1784366819828,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366919640,
      "tokens": 63519,
      "toolCalls": 5,
      "durationMs": 43228,
      "resultPreview": "{"verdict":"confirmed","severity":"low","note":"Headline claim verified. Cargo.toml:111 still reads `oxiarc-lzhuf       = \"0.2.7\"` inside the `[workspace.dependencies]` block (sits between `tar = \"0.4\"` at :110 and `sha2 = \"0.10\"` at :112). docs/dev/project/changelog.md:28-30 does say the codec is removed \"along with the `oxiarc-lzhuf` dependency\". A repo-wide grep over *.toml/*.lock/*.rs/…"
    },
    {
      "type": "workflow_agent",
      "index": 49,
      "label": "verify:traceability-matrix CAP-41 still records",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "ade6ef7538f488b5d",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366876462,
      "queuedAt": 1784366819829,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "Read traceability-matrix.md:233 (CAP-41 row verbatim), its …",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366922068,
      "tokens": 70533,
      "toolCalls": 5,
      "durationMs": 45606,
      "resultPreview": "{"note":"Read traceability-matrix.md:233 (CAP-41 row verbatim), its frontmatter (status: living, self-described as \"the living, end-to-end traceability matrix\" — so the frozen-record exemption does not apply), changelog.md:29 + the PR #193-#195 entry (which explicitly retracts the claim: \"REMOVED (PR #948) — 'matching Winlink Type C convention' was an unverified claim\"), and b2f_integration.rs…"
    },
    {
      "type": "workflow_agent",
      "index": 50,
      "label": "verify:osi-layer-map places LZHUF at the presen",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "a715d1d244a5357b1",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366876964,
      "queuedAt": 1784366819829,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "Verified by reading docs/osi-layer-map.md:20 and :45 (both …",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366933425,
      "tokens": 67464,
      "toolCalls": 6,
      "durationMs": 56461,
      "resultPreview": "{"note":"Verified by reading docs/osi-layer-map.md:20 and :45 (both quote verbatim as claimed), crates/openpulse-b2f/src/compress.rs:1-12, crates/openpulse-core/src/compression.rs:1-50, plus greps for LZHUF/Zstd across crates/apps/plugins and git log on both paths.\n\nCONFIRMED. The L6 row is wrong in both directions: (a) LZHUF was deleted in PR #948 (c506101) — compress.rs:5 says \"Type C (LZHUF)…"
    },
    {
      "type": "workflow_agent",
      "index": 51,
      "label": "verify:FF-15 phase accounting: the roadmap sect",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "ae3a8994d12e6f492",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366886545,
      "queuedAt": 1784366819829,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366943036,
      "tokens": 65275,
      "toolCalls": 8,
      "durationMs": 56491,
      "resultPreview": "{"verdict":"confirmed","severity":"low","note":"Read roadmap.md:1493, :1497, :1581; CLAUDE.md:88, :129, :677; js8-discovery-rendezvous-plan.md:4, :172; and grepped the CLI for the Phase-G surface.\n\nCONFIRMED (narrowly): roadmap.md:1493's heading parenthetical literally reads \"*(A–F shipped: full RX + beacon TX + rendezvous → HPX handoff; only H on-air remains)*\", which is wrong. Its own body a…"
    },
    {
      "type": "workflow_agent",
      "index": 52,
      "label": "verify:CE-SSB comments across four crates still",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "a8f8035ea8d4b2e37",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366906532,
      "queuedAt": 1784366846993,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366953361,
      "tokens": 66907,
      "toolCalls": 5,
      "durationMs": 46828,
      "resultPreview": "{"verdict":"confirmed","severity":"medium","note":"Verified every cited line. engine.rs:701-716 gate is exactly as the finding states: `if !m.starts_with(\"OFDM\") { return false; }` followed by exclusion of 8PSK/16QAM/32QAM/64QAM/32APSK -> only OFDM16/OFDM52 qualify. All four contradicting comments read verbatim as quoted: linksim lib.rs:246-247 and gui.rs:1163-1164 (\"OFDM QPSK/8PSK\"), linksim …"
    },
    {
      "type": "workflow_agent",
      "index": 53,
      "label": "verify:Shipped docs cite `cessb_benefits_hold_o",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "a90df8861c69ac223",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366911359,
      "queuedAt": 1784366846993,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366976188,
      "tokens": 67001,
      "toolCalls": 8,
      "durationMs": 64829,
      "resultPreview": "{"verdict":"confirmed","severity":"medium","note":"CONFIRMED on the evidence, with severity downgraded high->medium.\n\nVerified by reading: docs/features.md:172-173 (quote matches exactly; frontmatter says status: living, last_updated 2026-06-24 -> not a frozen record); docs/dev/project/roadmap.md:840-845 (quote matches, incl. \"`cessb_benefits` enabling all OFDM*/SCFDMA* is therefore correct as-…"
    },
    {
      "type": "workflow_agent",
      "index": 54,
      "label": "verify:Gateway test doc comment is attached to",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "ae86261aaa14e1ff3",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366913107,
      "queuedAt": 1784366846993,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366951686,
      "tokens": 63551,
      "toolCalls": 3,
      "durationMs": 38579,
      "resultPreview": "{"verdict":"confirmed","severity":"low","note":"Verified by reading crates/openpulse-gateway/src/main.rs:331-359 and :419-420, plus a grep of all ///, #[test] and fn lines in the file. The claim is accurate on every point: (1) line 331 reads \"/// Full round-trip: gateway sends one message, mock CMS sends one reply.\" and is attached, via lines 332-333 and the #[test] at 334, to fn iss_send_report…"
    },
    {
      "type": "workflow_agent",
      "index": 55,
      "label": "verify:`B2fSession::receive_data` doc still say",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "ab14f61cd29dc4428",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366913049,
      "queuedAt": 1784366846993,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366946412,
      "tokens": 61441,
      "toolCalls": 4,
      "durationMs": 33363,
      "resultPreview": "{"verdict":"confirmed","severity":"low","note":"Read crates/openpulse-b2f/src/session.rs:280-333 and crates/openpulse-b2f/src/compress.rs:1-20, plus grepped ProposalType::C across the crate. The finding is accurate verbatim: session.rs:297 documents \"Selects decompressor based on the accepted proposal type (D=Gzip, C=LZHUF)\" while session.rs:315-324 has no selection — D calls decompress_gzip, C …"
    },
    {
      "type": "workflow_agent",
      "index": 56,
      "label": "verify:`ModulationPlugin::demodulate_soft`'s LL",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "a85d1bc4d893b394e",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366927375,
      "queuedAt": 1784366846993,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366986060,
      "tokens": 67452,
      "toolCalls": 6,
      "durationMs": 58685,
      "resultPreview": "{"verdict":"confirmed","severity":"low","note":"Read plugin.rs:129-160, fec.rs:529-556, and all three named plugins. The claim is factually correct at every cited line. plugin.rs:151-153 states 64QAM passes noise_var=1.0 and BPSK/QPSK emit raw correlations; all three now calibrate: 64qam/demodulate.rs:578-581 uses preamble_noise_var(...).unwrap_or_else(estimate_decision_noise_var) into symbol_llrs…"
    },
    {
      "type": "workflow_agent",
      "index": 57,
      "label": "verify:`receive_with_llr_combining`'s one-line",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "a5d04ad0e959c143c",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366934060,
      "queuedAt": 1784366846993,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366968609,
      "tokens": 65312,
      "toolCalls": 5,
      "durationMs": 34549,
      "resultPreview": "{"verdict":"confirmed","severity":"low","note":"Read crates/openpulse-modem/src/engine.rs:3931-3999 (the doc block + `receive_with_llr_combining` body), :4001-4065 (`combine_and_decode_llrs`, where the actual combine lives), and crates/openpulse-core/src/fec.rs:591-603 (`combine_llrs_map`).\n\nThe cited text is exactly as quoted. engine.rs:3931-3933 reads \"/// Receive via SNR-weighted LLR combini…"
    },
    {
      "type": "workflow_agent",
      "index": 58,
      "label": "verify:CLAUDE.md's "one-line doc comment / no m",
      "phaseIndex": 2,
      "phaseTitle": "Verify",
      "agentId": "ab635524c8f494d42",
      "model": "claude-opus-4-8[1m]",
      "state": "done",
      "startedAt": 1784366930644,
      "queuedAt": 1784366846993,
      "attempt": 1,
      "lastToolName": "StructuredOutput",
      "lastToolSummary": "confirmed",
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784366961113,
      "tokens": 63215,
      "toolCalls": 4,
      "durationMs": 30468,
      "resultPreview": "{"verdict":"confirmed","severity":"low","note":"Read CLAUDE.md and re-ran the scan independently.\n\nWHAT I VERIFIED:\n1. The convention text exists verbatim — but NOT at the cited CLAUDE.md:410. `grep -n` puts it at **CLAUDE.md:551-553** (\"- All public types and functions get a one-line doc comment / - No multi-paragraph docstrings / - No comments explaining what the code does; only comments exp…"
    },
    {
      "type": "workflow_agent",
      "index": 59,
      "label": "synthesize",
      "phaseIndex": 3,
      "phaseTitle": "Synthesize",
      "agentId": "a508fbb09d3284c0d",
      "model": "claude-sonnet-5",
      "state": "done",
      "startedAt": 1784366997013,
      "queuedAt": 1784366993937,
      "attempt": 1,
      "promptPreview": "Repo: OpenPulseHF — a Rust plugin-based HF software modem (amateur radio digital data over HF).
Read CLAUDE.md FIRST: it is the authoritative agent contract and carries the crate map, the
acceptance-criteria table (requirement <-> acceptance test), the "Current phase and execution order"
status lists, a large "Known sharp edges" section, and a "DSP acquisition & carrier-recovery playbook".
Other k…",
      "lastProgressAt": 1784367183365,
      "tokens": 156016,
      "toolCalls": 0,
      "durationMs": 186352,
      "resultPreview": "# OpenPulseHF Documentation & Consistency Audit — Final Report
