---
project: openpulsehf
doc: docs/dev/reviews/governance-review-2026-09-30.md
status: resolved
last_updated: 2026-10-01
---

# Governance and focus review — 2026-09-30

Written in a cloud session at the maintainer's request: review how the project is governed and
driven, find where focus is lost, and name the deadlock, looping, over-optimisation and "ghost"
risks. The maintainer set the goal for this review:

> **First release = the basic modem: the full mode ladder, error correction and compression.
> Everything else waits.**

That sentence is narrower than the 1.0 defined in `docs/dev/project/release-1.0-criteria.md`, and
most findings below follow from the gap between the two. Every number here comes from a command
listed in *Apparatus* at the end. The draft was reviewed adversarially by Fable; its corrections are
folded in and listed in *Second opinion*.

> **Decided 2026-09-30.** The maintainer went through this review item by item and took twelve
> decisions. They are recorded, with the resulting milestones, in
> [`docs/dev/project/workplan.md`](../project/workplan.md), which supersedes §4 and §5 below.

---

## 1. Findings in one screen

1. **No commit since 2026-07-30 has produced on-air evidence.** The newest evidence bundle is dated
   **2026-07-30**, two months ago, while on-air evidence is the declared hard gate. In September,
   28 of 114 commit subjects touch the modem/DSP/BPSK/DCD path and 21 touch secondary subsystems;
   all of it was validated in simulation or on replayed captures.
2. **The issue tracker is growing, and fixes are what grows it.** 65 issues are open. Of the 36
   opened since 19 Sep, 29 are still open. Recent fix PRs close one issue and file four or five
   follow-ups: #1469 filed #1463–#1467; #1462 filed #1464–#1467-class items. That is the loop.
3. **The release path is deadlocked by three decisions that depend on each other** (details in §3):
   on-air evidence is a hard gate; the campaign waits for the preamble wire break (#1062); #1062 has
   no chosen design after two months; and the campaign is also waiting for a USB isolator purchase
   (G0). Nothing in that chain has a date or a stopping rule.
4. **Process cost now exceeds what a solo project can carry.** `CLAUDE.md` is 726 lines / 144 KB and
   is loaded into every Claude session. `traceability.md` is 17 333 lines / 1.4 MB. A full gate runs
   ~2 h and the held-out suites add ~83 min. Each design decision, each conclusion and each write-up
   goes to Fable separately. Between 9 % (subjects naming `trace`) and 21 % (a broader pattern that
   over-matches) of the 290 commits since August went to the trace/gate/hook tooling.
5. **Ghosts: many open issues describe failures nobody has observed on the air.** They are found by
   probes, fixtures and reviews of reviews. Some are real. But the project has close to zero field
   hours, so there is no way to tell which ones matter, and each one is treated as a blocker.
6. **Compression, which your release explicitly includes, has gaps the matrix marks ✅.**
   - **Corrected 2026-10-01 — this bullet is wrong.** A packed zstd frame **does** carry the dictionary
     ID, in zstd's own frame header, and zstd refuses a mismatched dictionary (`Dictionary mismatch`) —
     measured in a probe; see work plan decision 17 and the 2026-10-01 ledger entry. The bullet below
     was concluded by reading `compression.rs` and was never run. The real defect was only the third
     bullet (a failed unpack delivered as the message), now fixed. Original text, kept for the record:
   - ~~**Dictionary version is not on the wire.**~~ `unpack` maps algorithm tag `2` to
     `Zstd(ZSTD_DICT_ID)` unconditionally (`compression.rs:149`). The dictionary ID field exists to
     catch version skew but is never sent or checked, so a retrained dictionary
     (`tools/openpulse-dict-trainer`) would silently break every mixed-version session. This is the
     one that matters most for a release.
   - **REQ-CMP-03 ("negotiated in the handshake") describes a mechanism that was deliberately
     removed.** The negotiation fields were deleted in the #1147 wire break (#1166, PR #1189) because
     nothing consumed them. The pack format is self-describing, so the honest fix is to rewrite the
     requirement, not to build negotiation. `requirements.yaml` still marks it ratified and covered.
   - **A packed frame that fails to decompress is passed on as raw bytes** (`server.rs:1132`,
     `unpack(..).unwrap_or(bytes)`), where REQ-CMP-05 says it is an integrity error. Low severity:
     the bytes already passed FEC and CRC, so only version skew or a hostile peer reaches it. It is
     a three-line fix.

   None of this is on anyone's list, while DCD stage 3 is. That is the focus problem in one
   example.

---

## 2. Where focus is lost

### 2.1 Scope is ~4× the stated release

The workspace has 24 crates, 10 plugins, 5 apps and ~193 000 lines of Rust. The release you
describe needs roughly this subset:

| Needed for "basic modem, ladder, FEC, compression" | Can wait |
|---|---|
| `openpulse-core`, `-modem`, `-dsp`, `-audio`, `-radio` (PTT), `-config`, `-channel` (tests) | `-mesh`, `-repeater`, `-discovery` + `js8` plugin, `-filexfer`, `-qsy` |
| Plugins used by `hpx_hf` (`profile.rs:378-392`): `mfsk16` (SL1, incl. its K=3 return channel), `bpsk` (SL2–5), `qpsk` (SL6, `-D`), `ofdm` (SL7–14), `fsk4` (ACK) | `-freedv-auth`, `-gateway`, `-b2f`, `-b2f-driver`, `-kiss` |
| `openpulse-daemon` + `openpulse-cli` (one way to run it) | `-gpu`, `-keystore`, `-linksec`, `pki-tooling`, PQ handshake |
| `apps/openpulse-linksim` (the goodput gate) | `-panel`, `-tui`, `-twinview`, `-testbench`, `-testmatrix` |
| | plugins not on `hpx_hf`: `psk8`, `64qam`, `scfdma`, `pilot` |

Open question for you: is ARDOP (Pat/Winlink clients) part of "basic"? The current 1.0 criteria
require a Winlink exchange (A3). Your sentence does not mention it. I have put it under *can wait*.

In September, 21 commits touched the repeater, KISS, ARDOP, mesh and QSY. **At least 11 of those
were transmit-safety or §97 fixes and were right to do before any release:** unkeyed emissions
(#1250, #1259), rig_b left keyed (#1260, #1324, #1325), a KISS PTT watchdog never started (#1299),
mesh's route to real audio removed (#1251), repeater station ID (#1332). The drift is the rest:
**receive-side work in crates the release does not ship** — the repeater cannot receive / give
rig_b its own card (#1297, #1308), KISS and ARDOP cannot receive on real audio (#1310).

### 2.2 The 1.0 criteria doc defines a different release

`release-1.0-criteria.md` defines 1.0 as: on-air QSO, an attributed on-air ladder, a Winlink
exchange through a real CMS, security posture, coverage tooling (from scratch), a third-party-
implementable wire spec and a docs audit. That is a good 1.0. It is not the first release you
described. Keeping it as the only target means the "basic modem" release has no written definition,
so it cannot be finished.

**Recommendation:** write a short `release-0.x-basic-modem.md` (one page), or rename the existing
doc's scope. Section 5 has a draft.

### 2.3 The acquisition / DCD thread is converging slowly

The 2026-08-18 snapshot already asked "are we going in circles?" and answered "a convergent spiral".
Six weeks on, the same thread is still the largest single consumer:

- BPSK timing lock (#1438 PR1, PR2), crossfade arms (#1428, #1429, #1363), SNR estimate on the
  early lock (#1451), oracle phase gap (#1442), m·baud/32 residuals (#1441), frames ≥1.5 symbols
  in (#1450);
- DCD: stage 1 (#1452), stage 2 (#1454), stage 3 (#1443/#1469), plus #1455, #1464–#1467.

Each step is carefully measured. The problem is not quality, it is the **stopping rule**: every one
of these is judged against simulated or replayed captures at margins (0.5 dB, 1/16 placements,
8-sample timing) that the first on-air campaign will re-rank anyway.

---

## 3. Deadlock, looping, over-optimisation, ghosts

### 3.1 Deadlock — the release chain has no terminating step

Recorded decisions (maintainer, 2026-08-03):

1. On-air evidence is a hard gate for 1.0.
2. The wire format may change freely until 1.0, "and that is the reason to maximise maturity before
   tagging". Order: wire format / maturity → on-air campaign → tag.

Plus the facts:

3. #1062 (new preamble) is on the critical path and must land before the campaign. It has been open
   since 2026-08-03. Probes f7–f13 and R6 measured the problem; the issue lists three candidates and
   none has been chosen. `release-1.0-criteria.md` (*Sequencing*) records *why* it stays on the
   critical path. Its core argument is that the preamble now matters because #1118 put the chain on
   the daemon path — that shows the current preamble is **deployed**, not that it **fails on air**.
   No on-air failure attributable to the period-4 preamble is recorded; the on-air blocker is USB
   RFI (`onair-execution-plan.md`).
4. The recorded-capture gates are `#[ignore]`d until a re-record after #1062 (#1351).
5. The campaign also waits on G0, the receive-path RFI fix, whose lead item is a galvanic USB
   isolator (a purchase), noted 2026-08-18. The isolator may not be all of G0: the FT-991A receive
   path fails offline too, and the plan says "if G3 still fails, return to G0".
6. The on-air plan itself (`onair-execution-plan.md`) was last updated 2026-07-23 and predates every
   decision above.

"Maximise maturity" has no end condition, so step 2 never finishes. Step 3 is research with no
time-box. Step 5 has blocked the campaign for six weeks. Result: the only thing that could re-rank
the backlog (field evidence) is unreachable, so the backlog grows.

**Ways out.** A preamble change after a tag is *not* free: the `Frame` version byte and the ACK are
whitened, so no version check is reached, and recovery needs dual-decode before FEC at nine
`demodulate_soft` sites plus HARQ accumulators split per keystream (`release-1.0-criteria.md`,
*What this package does not buy*). An earlier draft of this review said a later preamble could just
be a new mode name; that is only true with a dual-receive transition mode, which is #1062's own
staged path. So the choice is:

- **(A) Time-box #1062 to two weeks with a pre-chosen candidate.** Take the cheapest of the three the
  issue lists (modem73-style terminator), land it, re-record the corpus once. **Recommended.**
- **(B) Release the basic modem as 0.x on the current preamble, explicitly saying "wire format will
  change before 1.0".** That is compatible with decision 2 (the format may change until 1.0 is
  tagged) and lets the campaign start now. The cost is a second campaign after #1062, which the
  decision record calls acceptable only if no campaign has been paid for yet — so this reverses a
  recorded reasoning, and is yours to decide.

In both cases: **order the isolator this week** and **re-baseline `onair-execution-plan.md`** before
the first on-air weekend. 2 m is useful for plumbing (A1 on 2 m so far is one rig→rig decode in one
direction, plus an SDR off-air decode of the transmit chain) but cannot score A2: a line-of-sight 2 m
link does not fade, and A1 is defined over HF.

### 3.2 Looping — the fix → review → follow-up issues cycle

Pattern seen in the September PRs:

1. A defect is found by a probe or review.
2. The fix gets two or three Fable rounds, sabotage runs, a ledger entry, a review artifact and a
   multi-paragraph acceptance row in `CLAUDE.md`.
3. The review finds 3–5 adjacent edge cases; each becomes a new issue.
4. Those issues enter the same cycle.

Adversarial review is very good at finding edge cases, and it always finds more. Without a filter
("does this block the release?"), it produces work faster than one person can close it.

**Fix:** follow-ups found in review default to a `post-release` label and one line in a parking
list, not a full issue body. Only promote one to an issue when it blocks the release goal or it is
a transmit-safety defect.

### 3.3 Over-optimisation

- **The verification system verifies itself.** `gate.sh` has a self-test; `trace.py` has
  evidence-, graph- and yaml self-tests; there are ratchets on vacuous bindings, on trailer
  relevance, on dormant packages, on re-homed doc comments, on ledger order. Each was added after a
  real miss, and each is reasonable alone. Together, since August, about a fifth of commits went to
  this layer (26 subjects mention `trace` alone). For a solo project before any release, that is
  too much insurance on the process and too little on the product.
- **`CLAUDE.md` has become a lab notebook.** Rows in the acceptance table are up to ~40 lines of
  measurement history. The "Known sharp edges" section is valuable but belongs in a doc that is
  read on demand. Every Claude session pays for all 144 KB in context and tokens, which matters
  on a Max 5 plan.
- **Margins below what the field can resolve.** Examples: 15/16 vs 16/16 placements, 2 dB against
  an oracle timing phase, a ρ bound resting on a min-of-180 vs 600 seeds (#1337). These are fine
  research questions; they are not release blockers until the air says they are.

### 3.4 Ghosts

"Ghost" here means: a failure described in detail, gated, and fixed, that nobody has seen on a real
link. Signs in the tracker:

- issue titles with numbers from synthetic fixtures. #1463 ("~1/7 of on-air frames") is derived
  from a noiseless measurement and the assumption that onset phase is uniform on the air, not
  counted on air — and by its own text it **does not touch `hpx_hf`** (SL6 is differential). It is a
  ghost *for this release* and should be labelled `post-release`;
- defects in paths no station runs by default (repeater, mesh, PQ handshake, discovery);
- CLAUDE.md's own history of "confident wrong beliefs" is mostly about *simulator* conclusions that
  reality overturned (see the 2026-08-18 snapshot: "every synthetic-only conclusion in this thread
  that met reality was overturned").

The project's own lesson says the same thing: field evidence re-ranks everything. The fastest cure
for ghosts is the on-air campaign, not more probes.

---

## 4. Re-prioritised plan to a "basic modem" release

Ordered. Each step has an exit condition and a time-box.

| # | Step | Exit | Box |
|---|---|---|---|
| 0 | Finish in-flight work: merge #1468, rebase + gate + merge #1469 (handover in #1469) | both merged | 1 day |
| 1 | **Freeze scope.** Label every open issue `release` or `post-release`. Only `release` issues are worked. Non-release crates get no new features and no new issues except transmit safety | label pass done | ½ day |
| 2 | Write the one-page release definition (§5). Add the compression negotiation + REQ-CMP-05 fix to it | doc merged | ½ day |
| 3 | Decide #1062 (A or B in §3.1) | decision recorded | 1 day |
| 4 | Buy/fit the USB isolator, or move A1/A2 to 2 m | order placed / plan changed | this week |
| 5 | Fix compression: put the zstd dictionary ID on the wire and reject a mismatch; make a failed unpack of a packed frame an integrity error; rewrite REQ-CMP-03 to "self-describing, receiver always accepts, sender opt-in" | gates pass | 2–3 days |
| 6 | Ladder release check: `every_profile_rung_decodes_at_its_floor_with_its_fec`, `hpx_hf_rungs_survive_fade`, `mfsk16_arq_subfloor`, `goodput_gate`, benchmark — **and list the known-red/ignored rows** (notch #1457, the two replay-corpus rows #1351) as "accepted for 0.x" rather than leaving them out, or the check is green by selection | all green at one commit, red rows disclosed | 1 day |
| 7 | On-air: re-baseline the plan, pass G0–G3, then A1 both directions and A2 scored per `release-1.0-criteria.md` §A2 (climb used successfully, demotion, stability; both stations' logs + capture) on `hpx_hf` with compression on. First confirm #1081's attribution reaches the evidence bundle with `observability.audit_mode` on | evidence bundle | 2–4 weekends |
| 8 | Tag the basic-modem release. Only then reopen `post-release` issues, ranked by what the air showed | tag | — |

Things I would explicitly **stop** until step 8: DCD stages beyond #1443, BPSK timing refinements,
#1062 probes (after step 3), repeater/mesh/KISS/ARDOP issues without a transmit-safety angle, new
trace/gate ratchets.

**Do not change a default to make a red row go away.** The notch suite (#1457) failed its own
*negative control* — the instrument broke, not necessarily the notch. Either re-derive
`RESCUE_LADDER` (bounded work) or ship with the row disclosed as "unproven since `884d96ed`".
Decision 3 says add-ons are runtime-*optional*, not off.

---

## 5. Draft: one-page release definition

> **Release "basic modem" (v0.17 or 1.0-rc).** An operator can run `openpulse-daemon` with
> `hpx_hf` between two stations and exchange data. The ladder SL1–SL14 climbs and demotes on a real
> link, every rung is FEC-coded, and session compression is negotiated and used.
>
> **In:** hpx_hf ladder, its plugins, RS/SoftConcat/LDPC FEC as assigned, FSK4 ACK, session
> compression, signed classical handshake (already shipped), PTT backends, station ID, CLI and
> daemon.
>
> **Gates:** the workspace gate; the ladder gates in §4 step 6; on-air A1 (both directions) and A2
> (climb, demote, stability) with retained logs.
>
> **Out (ships disabled or experimental, no gate):** mesh, repeater, discovery/JS8, file transfer,
> QSY, FreeDV auth, Winlink/B2F/gateway, KISS, ARDOP, PQ handshake, GPU, panel/TUI, notch/AGC/CE-SSB
> add-ons, non-hpx_hf plugins.

---

## 6. General tips for a solo maintainer with Claude Code (Max 5)

**Budget and context**

- **Slim `CLAUDE.md` to ~200 lines.** Keep: build/test commands, a short crate map, coding
  conventions, verification mechanics 1–6, the "RX has two entry families / one seam" checklist,
  "rebuild BOTH ends", and the review-scope rule (or its replacement). Move the sharp edges and the
  DSP playbook to `docs/dev/`, linked with an explicit "read before touching acquisition". Keep the
  acceptance table's requirement → test command pairs somewhere machine-readable (one line each);
  it is the only index that is not the 1.4 MB ledger. Move its measurement histories out. This is
  likely the biggest single saving on your plan.
- **Stop growing `traceability.md` by essays.** A ledger entry should be ~5 lines: requirement, PR,
  files, test command, gate line. The long reasoning already lives in the PR.
- Use the cheapest model that works: Sonnet/Haiku for mechanical edits, renames, doc fixes and
  running gates; Opus for design; Fable only where §6 "review" says so.
- Keep sessions to one task. Long sessions re-read the same 144 KB and large files repeatedly.

**Review (Fable)**

- Keep review **mandatory** for: wire-format and trait changes, anything that keys a transmitter, and
  conclusions that become "do not re-attempt" eliminations. Those are where wrong beliefs were
  expensive.
- Make it **optional** for: bug fixes with a sabotage-verified test, write-ups, and PR bodies. One
  review round per PR, not per artifact. This reverses your 2026-08-02 rule, so change it in
  `CLAUDE.md`'s *Adversarial review* section itself, or the two files will disagree.
- Ask the reviewer one extra question every time: *"Does this block the release goal? If not, say
  so."* That turns review from an edge-case generator into a filter.

**Tests and gates**

- The full gate takes ~2 h. The post-merge gate on `main` (#1144) is detection, not prevention,
  and the pre-push hook only tests crates with changed files — #1074's failure mode (a core change
  breaking a modem test). If you move to a daily full gate, first make the hook **test** (not just
  name) reverse dependents when `openpulse-core` or `openpulse-dsp` change. Then crate-scoped tests
  per push + daily gate + gate before tag is a safe rhythm.
- Add no new ratchet or self-test until the release is tagged, unless a real regression escaped.

**Planning**

- **WIP limit of one.** One open PR on the modem at a time; no stacked PRs (#1468/#1469 show the
  rebase cost).
- **Time-box investigations.** Write the box into the issue ("2 days, then decide"). When the box
  ends, pick the best-known answer and move on.
- **A weekly 15-minute review** with three questions: did the release get closer? what is blocking
  on-air? what can I drop?
- **Get binaries to 2–3 other operators** after the basic release. Their field hours are worth more
  than any probe.

---

## Second opinion (Fable)

Fable reviewed the draft against `origin/main` at `b7737d65` with instructions to falsify. Verified:
the sizes, dates, the `hpx_hf` plugin set, the 2026-07-30 bundle, the raw-bytes path at
`server.rs:1132`. Corrected, and folded in above:

1. REQ-CMP-03's missing negotiation was a recorded decision (#1166), not an oversight; the fix is
   the requirement text. The raw-bytes path is behind FEC+CRC, so lower severity than first
   written.
2. **Missed:** the zstd dictionary ID is never on the wire (`compression.rs:149`) — the most
   release-relevant compression defect. Verified by reading the code.
3. Option (B) as first written ("a later preamble is just a new mode name") was unsound: the
   version byte is whitened, so a preamble change needs dual-receive. Rewritten.
4. "A1 passed on 2 m" was overstated; A2 cannot be scored on 2 m.
5. Calling the September repeater/KISS/ARDOP work "drift" was unfair for the ≥11 safety fixes; the
   list is now split.
6. "Notch off by default" and "daily gate" could cause harm as written; replaced.
7. **Missed:** the release check must disclose the known-red rows; A2 needs its scoring and #1081
   attribution; G0 may be more than a purchase; the on-air plan is stale.

Fable's overall verdict: the diagnosis (deadlock between #1062, the campaign and G0; review
producing work faster than one person closes it; scope about 4× the stated release) is supported by
the repo.

---

## Apparatus

Commands run on 2026-09-30 against `origin/main` at `b7737d65`:

- Commit counts per month: `git log origin/main --format=%cd --date=format:%Y-%m | sort | uniq -c`
  (Apr 121, May 381, Jun 489, Jul 472, Aug 177, Sep 114).
- Subject keywords since 2026-08-01 (290 commits): `git log --since=2026-08-01 --format=%s | grep -Eic
  <pattern>`; `trace` = 26. The broader "gate/hook/ci" count (63) over-matches (`ci` inside words), so
  the "about a fifth" figure is an estimate, not a measurement.
- Latest on-air bundle: `ls docs/dev/test-reports/on-air` → newest `bundle-2026-07-30T170837Z-…`.
- Open issues: GitHub API, `state=OPEN` → 65. Opened since 2026-09-19: 36 (29 open, 7 closed).
- Size: `wc -l CLAUDE.md docs/dev/project/traceability.md`; `du -sh` → 144 KB and 1.4 MB.
- Rust LOC: `find crates plugins apps tools pki-tooling -name '*.rs' | xargs cat | wc -l` → 192 960.
- Compression: `crates/openpulse-daemon/src/server.rs:1132` and
  `crates/openpulse-core/src/compression.rs:142-153` (`unpack` returns `None` on a decompression
  error, and the caller substitutes the raw bytes); `crates/openpulse-config/src/lib.rs:221-229`
  (per-station opt-in, no negotiation).
