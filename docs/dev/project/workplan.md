---
project: openpulsehf
doc: docs/dev/project/workplan.md
status: draft
last_updated: 2026-10-05
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
| Create the GitHub milestones `M1 wire freeze` … `M4 release` and move the `milestone:*` stand-in labels onto them (not creatable from the cloud session's tools — maintainer, or a local session with `gh`). The labels `post-release`, `post-release-rerank`, `milestone:M2` and `milestone:after-v0.17` exist since 2026-10-02 (created by the triage) | 3, 6, 20 | **done 2026-10-04**: milestones `M1 v0.17 wire format`, `M2 release path`, `M3 on air (2 m)`, `M4 release 1 (v0.17.0)`, `after v0.17, before 1.0` (#1–#5); M2 holds 9 issues, after-v0.17 2; the `milestone:*` stand-in labels are gone |
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
| ~~#1062: replacement preamble~~ — moved into Release 1 (decision 22), see M2 | moved |
| Re-record the replay corpus against the 1.0 format; un-ignore the four replay rows (#1351) | parked |
| Repeat the 2 m campaign on the 1.0 format | parked |
| A twin-daemon test of the leaking NACK budget (#1456 review round 3, finding 6): four failed ladder bursts key three NACKs on the production path; needs deterministic failing bursts in the bridge | parked |
| Sender-side silence between fragments in one keying, as belt-and-braces to the #1461 receiver fix (review finding 8; about 6 % airtime; the gap needed depends on the receiver's read size and detector hold) | parked |
| Onset-ranking review (2026-10-04) parked items: K re-measured on a re-recorded on-air frame and with a steady tone at fc ± baud/4 stronger than the frame (finding 3's falsifier; the notch normally removes it first); the rank counter in the daemon's diagnostics rather than instruments only (finding 4); templates for BPSK31/63/100 so a station on a slow `[modem] mode` gets the fix too (finding 5) | parked |
| QPSK250-D at a longer preamble (from #1062): F0 measured its noise ceiling halving at 64 symbols, and F1 showed a longer aperiodic preamble decodes more on BPSK250; measure decode parity with the F1 harness and receive cost with the probe before proposing a second wire break | parked |
| Sync spread through the frame (pilots or a midamble) for the single-carrier modes (from #1062 F2): the fade-null tail — a preamble inside a fade while the FEC recovers the frame — bounds every preamble threshold, whatever the sequence | parked |
| A decoded-header check for #1460's evidence rule (from #1062 F3): a 62-symbol template matches foreign same-baud data at up to ~0.5 over a long over, so correlation alone cannot refuse it | parked |
| The onset ranking on the DDC correlation arm (from #1062 review finding 2): templates over 2 048 samples get no ranking today, which excludes BPSK31/63/100 and any long future preamble | parked |

### M2 — Release path (target 2026-10-28)

| Item | Status |
|---|---|
| ~~Compression: unpack failure + REQ-CMP-03~~ — moved to M1 (decision 17) | moved |
| ARDOP receive and host path: #1315 (ARQ ACK has no in-stream acquisition), #1385 (host frames > 255 B, no segmentation) | #1315 done 2026-10-02 (the ARDOP adaptive ISS and CLI `transmit_arq` listen through the OTA ACK listen's held-stream scan, 9 s window; `arq_ack_listen_holds_the_stream`); #1385 done 2026-10-02 (host blocks split into frame-sized chunks at the data port; each chunk is an ordinary frame and the host stream needs no reassembly; `host_block_larger_than_a_frame`) |
| Ladder behaviour that A2 scores: #1456 (NACK streak has no time decay), #1460 (a burst with no preamble evidence keys a NACK), #1446 (linksim: BPSK31 after MFSK16 fails 19/19 — diagnose: ladder bug or sim bug) | #1456 done 2026-10-02: a failed burst needs 0.5 s of post-lead audio (capped at half a frame) to count, and an ACK on air is not evidence; measured idle flicker is 0.1–0.15 s, the counted pieces of a failing frame 4.2 s and up (`design/reply-window-evidence.md` v3, two review rounds; `idle_flicker_is_not_evidence`). #1460 narrowed: a foreign over longer than 0.5 s still counts; the preamble discriminator is #1062. #1446 **not reproducible** (closed) |
| The daemon's NACK budget mutes the receiver after spaced idle flickers (found by the #1456 review): `consecutive_ota_nack` counts every keyed NACK and resets only on a decode, so after three flickers a failed first frame gets no NACK and the ISS abandons after 2 silent windows. HARQ also retains flicker LLRs before the evidence guards (SL7+) | mute done 2026-10-02 (the budget leaks one per 10 min of listening; `nack_budget.rs`); HARQ retention done 2026-10-03: a burst's LLRs are kept only once it clears the evidence guards. On 10 min of the recorded 250 Hz idle at SL7 and SL9, 240 non-evidence bursts were retained before and none after (`a_burst_that_is_not_evidence_retains_no_harq_llrs`) |
| Data larger than one frame through the daemon: #1461. Measured 2026-10-02: a 4-fragment file in one keying delivered one fragment and stalled. Fixed: `decode_burst_frames` and the OTA fallback continue past each decoded frame (`design/multi-frame-burst-decode.md`, reviewed) | done 2026-10-02 (`twin_multi_fragment_file`, OTA off and on; `burst_carries_several_frames`) |
| File-transfer receiver never asks for a missing fragment (found by the #1461 review): the sender's selective-repeat arm (`BlockAck { complete: false }`) is unreachable, because the daemon receiver only sends `complete: true` and has no fragment-gap timer. One faded fragment on HF fails the transfer at the 120 s block stall | done 2026-10-02 — also the lost ack, the lost NACK and the stall timers that did not scale with airtime (`design/filexfer-selective-repeat.md`, mandatory review; `filexfer_lossy_link`) |
| OTA phase 2 decodes an off-frequency fallback frame as a ladder frame at SL1 (found 2026-10-02): the fallback mode rides phase 2 as an `Sl1` candidate, so a non-ladder frame recovered there moves the controller and is ACKed, the #1123 failure mode for off-frequency bursts. It also yields the first frame only | done 2026-10-03: a phase-2 fallback decode returns early as non-ladder traffic, with no ACK, no controller update, and the keying's following frames (`off_frequency_fallback_is_not_ladder`) |
| PTT and station ID on the core path: #1257 (no leader delay before the first sample — done in its PR, default 0 until a rig is measured), #1334 (a flush timeout un-arms the station-ID timer — done in its PR), #1367 (200 ms sleep before PTT drop) | #1257, #1334 done; #1367 done 2026-10-02 in code (release waits for the device's reported drain plus 10 ms, capped at the old 200 ms; zero or no report → 200 ms; `design/drain-from-device-delay.md`, mandatory review); the tail and release checks run at the hardware-loopback stage and G1 |
| Key-to-audio gap (found 2026-10-02 reading the IC-9700 SDR captures for #1257): every keyed path keys first and then modulates and opens the output inside the guard, so the rig carries dead air for the synthesis time before the first sample — about 1.3 s per frame on the 2026-07-30 run (clicks at −1.3 s, reading UNCHECKED; `tests/captures/README.md`). It is airtime on every turn and an uncontrolled implicit leader. Moving synthesis before the key changes when PTT asserts → design review first (decision 8). Measure the gap on the 2 m pair before deciding whether it blocks. **Synthesis ruled out 2026-10-05** (`tx_synthesis_cost`, x86, release): ≤ 12 ms for every rung through SL11 and both ACKs, 19–26 ms at SL12–14, so even 4× slower on a Pi it is under 0.1 s. What remains between the key and the first sample is the device path: `open_output` (with a configured device name it enumerates every output device), stream build and start-up. The station probe `key_to_audio_by_stage` (openpulse-audio, `cpal-backend`) times those stages; it needs a real device | open — station probe pending (maintainer TODO), then the design |
| Held-out tests swept 2026-10-02 (after #1480 broke `ota_channel_adaptation` unseen): every ignored test in the files #1480 touched was run. All passed except two. `ofdm_scfdma_bakeoff` is a multi-minute measurement that hit a 25 min timeout, so it has no verdict. `daemon_vs_cli_on_real_captures` fails with "the daemon arm entered the acquisition chain (174 settles)", identically before #1489, on the pre-#1148 replay corpus parked for re-recording (#1351). No gate runs ignored tests, so `slow-tests.sh` before a release is the only net | recorded; corpus row stays with #1351 |
| #1304 (noise floor on OFDM52's wide band) — diagnose whether it breaks DCD on SL7–14 | done 2026-10-03: does not, since #1452's per-bin floor and burst hold. ~13 s of back-to-back OFDM52 frames on both wide-filter idles is one whole burst 8/8; with the hold disabled 8/8 split or end early (#1304's mechanism, reproduced). A single 255 B frame is too short to show it either way (`dcd_wideband_long_frame`) |
| #1421 dependency update, security-relevant ones only | done 2026-10-02: `h2` 0.4.19 (RUSTSEC-2026-0258) and `rustls` 0.23.45 (RUSTSEC-2026-0285), both reached by `openpulse-cli` through `reqwest`; CI actions on their Node 24 majors, pinned by SHA. Left: `lru` and `event-listener` (unsound, no fixed release; only the post-release `openpulse-panel`/`openpulse-tui` reach them) and three yanked transitive crates |
| Daemon receive cost on coded frames — **diagnosed 2026-10-02**: the #1123 uncoded fallback runs an exhaustive decode-driven onset scan (~128–400 full-frame decodes) on every coded ladder burst before the coded scan finds it. Release build, x86: 0.85–1.72 s per frame decode, of which ~1 s is that scan (SL2 0.77 s without it). Fix designed and reviewed (`docs/dev/design/fallback-onset-ranking.md`); **Pi numbers 2026-10-04** (`receive_cost_scaling`, release, `PROBE_READ=4096`, rpi53/rpi51): SL6 3.2/3.3 s, SL5 5.2/5.5 s, SL4 5.6/5.8 s, SL3 5.9/6.1 s, SL2 7.0/7.3 s per decode, about 4× x86. Against the 9 s ACK window: with the 0.52 s FSK4 ACK, SL2 leaves ≈1.2 s for the flush delay, PTT and key-up; with the ≈5 s MFSK16 ACK (sent when the IRS recommends SL1) no rung fits. **Blocks Release 1.** **Fixed 2026-10-04:** the fallback tries the top K = 4 preamble-ranked onsets, and its exhaustive scan runs only after the coded scan fails; x86 at 4 096-sample reads SL6 2.17 → 0.19 s, SL2 4.61 → 1.84 s per decode | done; Pi re-measure pending |
| **Profiles** (decision 18): two profiles, `fast` (today's `hpx_hf`) and `robust` (the same ladder capped at SL6, ≤ 500 Hz); the other ten deleted; no aliases. Design: `docs/dev/design/session-profiles.md` (reviewed; six fixes folded in) | done in the implementation PR |
| ARDOP `ARQBW` sizes modes from a stale hand-kept table (`bandplan::occupied_bandwidth_hz`): OFDM52 3200 Hz vs the plugin's 2031 Hz; MFSK16, QPSK250-D and OFDM52-* missing, so `ARQBW 2000` caps `fast` at SL5. Size from the plugin instead (found by the profile design). A correct table still stops `ARQBW 2000` below OFDM52 (2031 Hz); the loss is QPSK250-D everywhere and SL6–SL14 at ≥ 2032 Hz | done 2026-10-02 — `arq_max_tx_level_for_bandwidth` sizes from the plugin; a cap below every rung now answers the lowest rung, not "uncapped" (`arqbw_sizes_from_the_plugin`) |
| On-air tooling (found by the re-baseline): `run-onair-twin-ota.sh` enables `observability.audit_mode` so `OtaRateDecision` events are retained for A2; `onair-bundle-evidence.sh` CAT-reads each rig's filter width and frequency trim and records `notch_enabled`/`agc_enabled`/`cessb_enabled` | done 2026-10-02 (audit mode on, evidence collected per station, 2 m default) |
| Release check: `every_profile_rung_decodes_at_its_floor_with_its_fec`, `hpx_hf_rungs_survive_fade`, `mfsk16_arq_subfloor`, `goodput_gate`, benchmark, the ARDOP suites — at one commit, with the known-red rows disclosed (notch #1457 per decision 10) | done 2026-10-03 at `5809597f` (tree identical to the gated `beba762d`): `GATE: PASS beba762d… clean`, which ran `every_profile_rung_decodes_at_its_floor_with_its_fec`, `hpx_hf_rungs_survive_fade` (3/3), `mfsk16_arq_subfloor` (8/8), `goodput_gate`, `openpulse-ardop` (9 lib + 24 integration); benchmark gate `true`; `slow-tests.sh`: OTA CAP-33 3/3, spectral and total-power decode counts pass, notch REQ-QRM-01 2/3 — `the_notch_rescues_a_decode_that_fails_without_it` fails (decodes without the notch at 0.3), the disclosed #1457 row. Built with `CARGO_INCREMENTAL=0` and no debuginfo to fit the cloud session's disk; test behaviour is unaffected. Re-run before the tag |
| **#1062 replacement preamble** (decision 22): PN, N ≥ 63, chipped at each rung's own baud, on the BPSK rungs (SL2–SL5); QPSK keeps its designed sequence. Design `docs/dev/design/pn-preamble.md` (mandatory review, wire format), then the BPSK250 parity falsifier, then the remaining rungs, the synthetic fixtures for the #1021/#1045/#1049 classes, and #1460's evidence rule on top. Gates M3: the campaign runs on this format | design reviewed 2026-10-05; **F0: QPSK stays out** (64-symbol 500 Hz ceiling 0.334 > 0.23); **F1: PASS**, PN-63 non-inferior on all five columns and better on four (e.g. +0.033 [+0.010, +0.056] at the SL5 floor); F1c x86 SL2 1.79 → 3.08 s, Pi pending; **F2: FAIL** under its rule (decodable p01 0.327 vs 1.2 × worst interferer 0.304 = 0.364), though PN-63 cuts the worst interferer 0.981 → 0.304 — accepted at a 1.07 margin, threshold ≈ 0.315 (decision 23); F3: failed for every polynomial (misframed rule), keep x⁶ + x + 1 (decision 24); **F4: PASS**, AWGN 20 dB goodput 328 vs control 331 bps (bound 310), fade avg level 9.8 on both; continuing at F5 |

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
| 21 | 2026-10-02 | **#1456 and #1460 stay in M2**: the parked decay design is to be redesigned against its review's falsifier, not moved to post-release | maintainer |
| 22 | 2026-10-05 | **#1062 moves into Release 1 and gates M3. Amends decision 16.** The alternating preamble is the one root of #1460's remainder (a foreign over cannot be refused), the onset ranking's and the veto's BPSK250-only coverage, and #1049's onset placement; v0.17.0 is not cut, so the break costs one campaign and one corpus re-record instead of two. Sequenced design → mandatory review → the BPSK250 parity falsifier before any other mode. #1460 stays open until the evidence rule lands on the new preamble | maintainer |
| 23 | 2026-10-06 | **#1062 F2: accept BPSK250's PN-63 threshold at a 1.07 margin. Amends F2's pre-registered 1.2.** PN-63 at ρ ≈ 0.315 refuses every measured interferer (worst 0.304) and keeps the 3 dB decodable p01 (0.327); the shipped `--++` at 0.40 passes the tone (0.699) and the DSB pair (0.981) and rejects more decodable frames (p05 0.490). Strictly better than today on both sides; the 1.2 was not met. The sequence continues at F3 | maintainer |
| 24 | 2026-10-06 | **#1062 F3: keep x⁶ + x + 1.** F3 failed for all six degree-6 polynomials (whole-frame worst 0.52–0.65): the worst is chance correlation with whitened payload, set by frame length, so the rule was misframed and no N = 63 sequence meets it. Differences between polynomials are within payload-to-payload noise; x⁶ + x + 1 is already in production (pilot). Continue at F4 | maintainer |

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
