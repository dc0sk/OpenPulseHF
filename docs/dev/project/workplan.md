---
project: openpulsehf
doc: docs/dev/project/workplan.md
status: draft
last_updated: 2026-10-02
---

# Work plan — Release 1 ("basic modem")

**The single curated plan.** Milestones, exit criteria, status and the decision log live here. Every
open issue belongs to exactly one milestone or to `post-release`. If this file and the tracker
disagree, fix the tracker to match this file or change this file in a PR.

Origin: the [governance review of 2026-09-30](../reviews/governance-review-2026-09-30.md) and the
maintainer's twelve decisions on it (below). `release-1.0-criteria.md` still defines **1.0**;
Release 1 is a narrower first release on the way there.

---

## How this plan is kept current

| When | What | Who |
|---|---|---|
| Every PR | Update the status line of the milestone it serves, in the same PR. A PR that serves no milestone says `post-release` in its body and needs a reason | author |
| Every new issue | Assign a milestone or `post-release` within a day, using the triage rules below. No milestone = not worked on | author |
| Weekly (15 min) | Three questions: did Release 1 get closer? what blocks on-air? what can be dropped? Record the answers as one dated line in *Weekly log* | maintainer |
| Milestone end | Check the exit criteria, move leftovers explicitly (next milestone or `post-release`), add a dated line to the decision log | maintainer |

**Triage rules** (decisions 7 and 11):

1. **Transmit safety always blocks** — but only for a feature that ships **enabled** in Release 1. A
   defect in a feature that ships disabled (repeater, mesh, relay) is `post-release`, and that feature
   must stay off by default.
2. **A simulator-only defect blocks Release 1 only if** it is on the `hpx_hf` or ARDOP path **and**
   shows up at or above the rung's declared SNR floor. Otherwise it is `post-release`.
3. **Review follow-ups default to a one-line parking entry**, not a full issue body. Promote one to
   an issue only if rule 1 or 2 applies.
4. **Re-rank after M3.** Issues parked as `post-release (re-rank)` are re-read once on-air evidence
   exists; the air decides which of them matter.

---

## Scope of Release 1 (decision 1)

**In:** `openpulse-daemon` + `openpulse-cli` and the **ARDOP TNC** (`openpulse-ardop`); the `hpx_hf`
ladder SL1–SL14 and its plugins (MFSK16 incl. its K=3 return channel, BPSK, QPSK250-D, OFDM52, FSK4
ACK); the FEC each rung is assigned; session compression; PTT backends; station ID; the signed
classical handshake.

**Out** (ships disabled or experimental, no gate): mesh, repeater, relay, discovery/JS8, file
transfer, QSY, FreeDV auth, Winlink B2F/gateway (A3), KISS, PQ handshake, GPU, panel/TUI, plugins not
on `hpx_hf` (psk8, 64qam, scfdma, pilot).

**Version number:** `v0.17.0` (decision 14); `1.0` stays reserved for the full criteria.

---

## Milestones

### M0 — Governance reset (target 2026-10-07)

| Item | Decision | Status |
|---|---|---|
| This work plan + decision log | 3 | this PR |
| Triage all open issues into milestones (maintainer approves) | 6 | **done 2026-10-02** — approved and labelled (see *Triage* below) |
| Create the GitHub milestones `M1 wire freeze` … `M4 release` and move the `milestone:*` stand-in labels onto them (not creatable from the cloud session's tools — maintainer, or a local session with `gh`). The labels `post-release`, `post-release-rerank`, `milestone:M2` and `milestone:after-v0.17` exist since 2026-10-02 (created by the triage) | 3, 6, 20 | labels done; milestones open |
| Slim `CLAUDE.md` to ~200 lines; relocate, don't delete; acceptance table → one-line-per-row index | 7 | done (#1472): 731 → ~260 lines, 144 KB → 15 KB; six sections moved verbatim to `docs/dev/` |
| Rewrite the *Adversarial review* rule in `CLAUDE.md`: mandatory for wire-format/trait changes, anything that keys a transmitter, and eliminations; one round per PR; every prompt asks "does this block Release 1?" | 8 | done (#1472) |
| Pre-push hook **tests** reverse dependents when `openpulse-core`, `-dsp` or `-modem` change; fix #1357 and #1448 on the way | 9 | done (#1473) |
| Order the galvanic USB isolator | 12 | done — isolators in place (maintainer, 2026-10-01) |
| Re-baseline `onair-execution-plan.md` (dated 2026-07-23) against these decisions, incl. the #1081 attribution check and the FT-991A offline receive failure | 12 | done in the on-air re-baseline PR; it found two tooling gaps, now M2 rows |

**Exit:** all rows done; the tracker matches this file.

### M1 — v0.17 wire format (target 2026-10-14) — **done 2026-10-01**

Narrowed by decision 16: #1062 (the preamble) moves after v0.17; decision 17 then removed the one
payload format change, so M1 closed as a bug fix plus a declaration.

| Item | Status |
|---|---|
| ~~Compression: put the zstd dictionary ID on the wire~~ — not needed (decision 17): zstd already carries the dictionary ID in its frame header and refuses a mismatch, measured | dropped |
| Compression: a packed frame that fails to unpack is an integrity error — dropped, counted, logged with zstd's reason; REQ-CMP-03 rewritten to "self-describing, not assumed of the peer" (pulled forward from M2, decision 17) | done in the unpack PR |
| Declare the v0.17 wire format in `protocol-wire-spec.md`, stating that the preamble changes before 1.0 (#1062) | done |

**Exit:** the unpack fix merged; the v0.17 format declared. M1 carries **no** wire-format change.

### After v0.17, before 1.0 (decision 16)

| Item | Status |
|---|---|
| #1062: replacement preamble — the thread's recorded direction is PN, N ≥ 63, for interferer refusal and onset placement where thresholds exist; design reviewed before implementing; fold #1171 in | parked |
| Re-record the replay corpus against the 1.0 format; un-ignore the four replay rows (#1351) | parked |
| Repeat the 2 m campaign on the 1.0 format | parked |

### M2 — Release path (target 2026-10-28)

| Item | Status |
|---|---|
| ~~Compression: unpack failure + REQ-CMP-03~~ — moved to M1 (decision 17) | moved |
| ARDOP receive and host path: #1315 (ARQ ACK has no in-stream acquisition), #1385 (host frames > 255 B, no segmentation) | open |
| Ladder behaviour that A2 scores: #1456 (NACK streak has no time decay), #1460 (a burst with no preamble evidence keys a NACK), #1446 (linksim: BPSK31 after MFSK16 fails 19/19 — diagnose: ladder bug or sim bug) | open |
| Data larger than one frame through the daemon: #1461 | open |
| PTT and station ID on the core path: #1257 (no leader delay before the first sample — done in its PR, default 0 until a rig is measured), #1334 (a flush timeout un-arms the station-ID timer — done in its PR), #1367 (200 ms sleep before PTT drop) | #1257, #1334 done; #1367 open (needs a rig measurement) |
| #1304 (noise floor on OFDM52's wide band) — diagnose whether it breaks DCD on SL7–14 | open |
| #1421 dependency update, security-relevant ones only | open |
| Daemon receive cost on coded frames — **diagnosed 2026-10-02**: the #1123 uncoded fallback runs an exhaustive decode-driven onset scan (~128–400 full-frame decodes) on every coded ladder burst before the coded scan finds it. Release build, x86: 0.85–1.72 s per frame decode, of which ~1 s is that scan (SL2 0.77 s without it). Fix designed and reviewed (`docs/dev/design/fallback-onset-ranking.md`); **station measurement pending** — run `receive_cost_scaling` on rpi51/rpi53 with `PROBE_READ` set to the daemon's read size | design done, implementation after the Pi numbers |
| **Profiles** (decision 18): two profiles, `fast` (today's `hpx_hf`) and `robust` (the same ladder capped at SL6, ≤ 500 Hz); the other ten deleted; no aliases. Design: `docs/dev/design/session-profiles.md` (reviewed; six fixes folded in) | done in the implementation PR |
| ARDOP `ARQBW` sizes modes from a stale hand-kept table (`bandplan::occupied_bandwidth_hz`): OFDM52 3200 Hz vs the plugin's 2031 Hz; MFSK16, QPSK250-D and OFDM52-* missing, so `ARQBW 2000` caps `fast` at SL5. Size from the plugin instead (found by the profile design). A correct table still stops `ARQBW 2000` below OFDM52 (2031 Hz); the loss is QPSK250-D everywhere and SL6–SL14 at ≥ 2032 Hz | open |
| On-air tooling (found by the re-baseline): `run-onair-twin-ota.sh` enables `observability.audit_mode` so `OtaRateDecision` events are retained for A2; `onair-bundle-evidence.sh` CAT-reads each rig's filter width and frequency trim and records `notch_enabled`/`agc_enabled`/`cessb_enabled` | done 2026-10-02 (audit mode on, evidence collected per station, 2 m default) |
| Release check: `every_profile_rung_decodes_at_its_floor_with_its_fec`, `hpx_hf_rungs_survive_fade`, `mfsk16_arq_subfloor`, `goodput_gate`, benchmark, the ARDOP suites — at one commit, with the known-red rows disclosed (notch #1457 per decision 10) | open |

**Exit:** all items closed or explicitly moved; release check green except the disclosed rows.

### M3 — On air (target: as the rigs allow, after M1)

| Item | Status |
|---|---|
| Test stages in order (decision 13): virtual audio → hardware loopback → **2 m** (IC-9700 ↔ FT-818, 144.640 MHz) → HF only with the release candidate (M4) | — |
| G0–G3 per the re-baselined plan on the 2 m pair (isolators fitted; G3 decides whether the RFI is gone) | open |
| A1: two-station exchange **on 2 m** in both directions, logs retained | open |
| A2 **on 2 m**: climb (new mode then decodes), demotion, stability — scored per `release-1.0-criteria.md` §A2, both stations' logs + capture, `observability.audit_mode` on. A 2 m line-of-sight link does not fade, so the climb and demotion are driven by **induced level changes** (TX power, attenuation), which §A2 allows and requires to be recorded | open |
| A4 (station ID cadence) and A5 (PTT fail-safe) checks | open |
| ARDOP: one Pat session over the air through the TNC | open |
| Re-rank every `post-release-rerank` issue against what the air showed | after A2 |

**Exit:** evidence bundles for A1, A2, A4, A5 and the ARDOP session in
`docs/dev/test-reports/on-air/`.

### M4 — Release 1 (`v0.17.0`)

| Item | Status |
|---|---|
| **UI client for the release candidate** (decision 19), with file transfer or a similar reusable feature for testing. **Last before the RC cut**: started only when everything else for RC1 is done. Design first: whether it is new or grows out of `openpulse-tui`/`openpulse-panel`, and what turning on file transfer (disabled for Release 1 so far) costs | open, last |
| Cut the release candidate `v0.17.0-rc.1`: full gate + `scripts/slow-tests.sh` at one commit, known-red rows disclosed | open |
| **HF run with the release candidate** (decision 13): repeat A1 and A2 on an HF-capable pair, logs + capture retained. Band conditions are not controllable, so an HF result is evidence the RC works there, not a characterisation; a failure attributable to propagation is recorded, not chased | open |
| Release notes (known-red rows, out-of-scope features, "wire format v1"), version bump to `v0.17.0`, tag | open |

---

**What is on the maintainer** — hardware runs, credentials, decisions — is kept in
[`maintainer-todo.md`](maintainer-todo.md), separate from this plan so it stays short and actionable.

## Decision log

| # | Date | Decision | Source |
|---|---|---|---|
| 1 | 2026-09-30 | Release 1 = core modem (`hpx_hf` ladder, FEC, compression, daemon + CLI) **plus the ARDOP TNC** | governance review §2.1, round 1 |
| 2 | 2026-09-30 | #1062: two-week time-box with a pre-chosen cheapest candidate; wire freeze before the campaign | review §3.1, round 1 |
| 3 | 2026-09-30 | One curated plan: this repo doc, mirrored by GitHub milestones; per-PR updates + weekly review | round 1 |
| 4 | 2026-09-30 | On-air bar: A1 both directions + A2 scored on **HF** before tagging | review §5, round 1 |
| 5 | 2026-09-30 | Compression: all three fixes (dictionary ID on the wire, unpack failure = integrity error, REQ-CMP-03 rewritten) | review finding 6, round 2 |
| 6 | 2026-09-30 | Triage: Claude proposes a label per issue, maintainer approves | review §3.2, round 2 |
| 7 | 2026-09-30 | `CLAUDE.md` slimmed to ~200 lines; content relocated, not deleted | review §6, round 2 |
| 8 | 2026-09-30 | Fable review mandatory only for wire/trait changes, transmitter keying and eliminations; replaces the 2026-08-02 rule once written into `CLAUDE.md` | review §6, round 2 |
| 9 | 2026-09-30 | Gate rhythm: hook tests reverse dependents; full gate daily and before every tag; post-merge gate stays | review §6, round 3 |
| 10 | 2026-09-30 | Notch (#1457): ship on as today, row disclosed as unproven since `884d96ed`; fix post-release | review §3.3, round 3 |
| 11 | 2026-09-30 | Ghost rule: a simulator-only defect blocks only on the release path and at/above the rung floor; transmit safety always blocks | review finding 5, round 3 |
| 12 | 2026-09-30 | G0: maintainer orders the isolator; Claude re-baselines the on-air plan | review §3.1, round 3 |
| 13 | 2026-10-01 | **Test stages: virtual audio → hardware loopback → 2 m → HF only with the release candidate.** Development evidence (A1, A2) is collected on 2 m: no HF band noise for others during early testing, and no conclusions drawn from band conditions nobody controls. **Amends decision 4**: the HF run moves from the M3 gate to M4, with the release candidate | maintainer |
| 14 | 2026-10-01 | Release 1 is `v0.17.0`; `1.0` stays reserved for the full criteria | maintainer (open question 1) |
| 15 | 2026-10-01 | #1438, #1443, #1452, #1454 closed — their stages merged | maintainer (open question 2) |
| 16 | 2026-10-01 | **#1062 moves after v0.17, before 1.0. Amends decision 2.** Release 1 is 0.x and the wire format may change until 1.0 (release-1.0-criteria decision 2), so the preamble does not gate v0.17. Reading the full #1062 thread showed the "cheapest candidate" (a 2-symbol terminator) fixes onset placement only, not interferer refusal, at close to the full validation cost; the recorded direction is PN with N ≥ 63, whose demod parity is unmeasured. The v0.17 campaign runs on the current preamble, and a second, cheap 2 m campaign follows #1062 before 1.0 | maintainer |
| 17 | 2026-10-01 | **Compression: no wire change for Release 1. Amends decision 5.** Measured: a packed zstd frame already carries the dictionary ID (`0x7d6f375f`) in zstd's own frame header, and a decoder holding a dictionary with another ID fails with `Dictionary mismatch`. The silent-garbage path was only the daemon delivering a failed unpack's bytes; fixing that closes both findings. The governance review's "dictionary ID is never on the wire" was wrong — read from the code, not run | maintainer |
| 18 | 2026-10-01 | **Two session profiles: `fast` and `robust`.** `fast` = today's `hpx_hf` ladder (performance and bandwidth under good conditions); `robust` = the same ladder capped at SL6, ≤ 500 Hz, single-carrier (poor conditions, limited gear). Same ladder, so the handshake fingerprint matches and a fast↔robust pair keeps adaptive OTA. The other ten profiles are deleted; no alias for the old names | maintainer |
| 19 | 2026-10-01 | **A new UI client ships with the release candidate**, supporting file transfer or a similar reusable feature for testing. It comes after everything else RC1 needs. **Amends decision 1** (Release 1 scope) | maintainer |
| 20 | 2026-10-02 | **Triage approved** as proposed and applied as labels. M2 and after-v0.17 rows carry `milestone:*` stand-in labels until the GitHub milestones are created | maintainer |

## Open questions

None open. (Version number → decision 14; the four "close?" issues → decision 15.)

## Weekly log

| Week of | Closer? | On-air blocker | Dropped |
|---|---|---|---|
| 2026-09-28 | Plan written; #1468, #1469 merged | isolator not ordered | — |
| 2026-09-28 | M0 mine done (#1472–#1474); isolators in place; test stages set (2 m, HF with the RC) | triage approval pending | — |
| 2026-09-28 | M1 done: #1062 after v0.17 (#1476); compression needs no wire change, unpack fix merged (#1477); v0.17 format declared | triage approval pending | dict-ID wire change (measured unnecessary) |
| 2026-09-28 | Profiles decided (decision 18); UI client added to M4, last (decision 19) | triage approval pending | ten profiles; the `hpx_*` names |
| 2026-09-28 | Profiles shipped (#1480); PTT leader delay (#1481) and flush-timeout count (#1482) merged; receive cost diagnosed (#1483); triage approved and labelled (decision 20) | Pi receive-cost numbers (rpi51/rpi53); dual-card drain measurement for #1367 | — |

---

## Triage (approved 2026-10-02, decision 20)

The 65-issue triage proposal of 2026-09-30 was approved by the maintainer and applied as labels on
2026-10-02. The full proposal table is in git history; this section points at the tracker instead.

| Class | Label on the issue | Count applied |
|---|---|---|
| Release path | `milestone:M2` (stand-in until the GitHub milestone exists) | 9 open (#1257 and #1334 were already closed) |
| After v0.17, before 1.0 | `milestone:after-v0.17` (stand-in) | 2 (#1062, #1171) |
| Post-release | `post-release` | 34 |
| Re-rank after M3 | `post-release-rerank` | 12 |
| Already closed | — | #1438, #1443, #1452, #1454 (decision 15); #1357, #1448 (M0, fixed by #1473) |

Queries: `label:milestone:M2`, `label:post-release-rerank`, `label:post-release` on the issue tracker.
New issues follow the triage rules above; one that serves no milestone gets `post-release`.
