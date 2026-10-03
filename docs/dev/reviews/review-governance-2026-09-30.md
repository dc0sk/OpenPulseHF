---
project: openpulsehf
doc: docs/dev/reviews/review-governance-2026-09-30.md
status: resolved
last_updated: 2026-09-30
---

# Adversarial review — governance and focus review (2026-09-30)

One round by Fable (read-only, no files edited), prompted for falsification, on the draft of
`docs/dev/reviews/governance-review-2026-09-30.md` at commit `3eae9641` of branch
`claude/affectionate-brahmagupta-sdn8qg`. Every correction is folded into that doc and listed in its
*Second opinion* section. Two new findings were checked in the code before being used:
`compression.rs:149` (tag `2` maps to `Zstd(ZSTD_DICT_ID)` unconditionally) and
`release-1.0-criteria.md` *What this package does not buy* (the version byte is whitened).

## Consumer

- The maintainer, deciding the release scope and the #1062 route; no code consumes this.
  The review changes no behaviour.

## Prior art

- `docs/dev/project/release-1.0-criteria.md`, *Status and sequencing — snapshot 2026-08-18*, which
  asked "are we going in circles?" six weeks earlier; this review re-asks it against a narrower
  release goal and cites it throughout.
- `docs/dev/reviews/pre-1x-completeness-audit-2026-07-18.md` (completeness, not governance).

Found by: `ls docs/dev/reviews` and `grep -n "going in circles" docs/dev/project/*.md`.

## Twins

- UNCHECKED — a governance review has no sibling code path. The nearest twin is the 2026-08-18
  snapshot above, which this review does not edit.

## Prompt

Sent verbatim, apart from line wrapping:

> You are the adversarial reviewer ("Fable") for the OpenPulseHF repo at /home/user/OpenPulseHF
> (Rust HF data modem, solo maintainer + Claude Code). Read-only: do NOT edit files, do NOT commit.
>
> Review the draft at docs/dev/reviews/governance-review-2026-09-30.md. The maintainer asked for a
> review of project governance: where focus is lost, deadlock/looping/over-optimisation/"ghosts", and
> a re-prioritised path to a first release = "basic modem: full mode ladder, error correction and
> compression; everything else waits".
>
> Your job is to FALSIFY, not agree. For each numbered finding and each recommendation:
> 1. Check the factual claims against the repo (git log on origin/main, CLAUDE.md,
>    docs/dev/project/release-1.0-criteria.md, docs/dev/onair-execution-plan.md,
>    docs/mode-fec-ladder.md, crates/openpulse-daemon/src/server.rs around line 1132,
>    crates/openpulse-core/src/compression.rs, crates/openpulse-config/src/lib.rs ~221, handshake
>    code for any compression negotiation). Report any claim that is wrong, overstated, or unproven,
>    with the evidence.
> 2. Specifically test: (a) is compression truly not negotiated anywhere in the handshake (search
>    CONREQ/CONACK, capability fields, #1166)? Is the unpack-failure-passes-raw-bytes path actually
>    reachable after CRC validation, and how severe is it really? (b) Is the "deadlock" real, or does
>    the repo contain a decision/plan that already breaks it (e.g. a #1062 decision, a G0 status
>    update, an on-air plan after 2026-07-30)? (c) Is option (B) "ship current preamble as v1, later
>    preamble = new mode name" technically sound given the whitening/preamble/wire-format notes in
>    CLAUDE.md and release-1.0-criteria.md? (d) Is the claimed hpx_hf plugin set right (which plugins
>    does SessionProfile::hpx_hf actually use; is fsk4 the ACK; are pilot/scfdma/64qam/psk8 truly
>    unused by hpx_hf)? (e) Is "ghosts" fair, e.g. #1463 — was it measured on air or derived? Check
>    the issue text if accessible in repo docs/ledger, otherwise say UNCHECKED. (f) Are the September
>    repeater/KISS/ARDOP fixes actually transmit-safety defects that SHOULD have been done before a
>    release (i.e. is calling them focus drift unfair)?
> 3. Flag any recommendation that could cause harm (e.g. slimming CLAUDE.md losing load-bearing
>    rules, dropping the full gate per PR, turning off notch by default) and say what must be kept.
> 4. Say what the draft MISSED that matters most for getting a first release out.
>
> Be concise: a numbered list of corrections (claim → verdict → evidence file:line or command), then
> missed items, then an overall verdict. Keep under ~900 words.

## Verdict

Condensed from the returned report; the verdicts and evidence are Fable's.

**Corrections**

1. Compression not negotiated — **true, but it was a recorded decision**: the fields were deleted
   in the #1147 wire break (#1166, PR #1189) because nothing consumed them. `requirements.yaml`
   still marks REQ-CMP-03 ratified and covered. The fix is to rewrite the requirement.
2. Failed unpack passed on as raw bytes (`server.rs:1132`) — **true, severity overstated**: the
   bytes already passed FEC + CRC-16, so only version skew or a hostile peer reaches it.
   **Missed and worse:** `unpack` maps tag `2` to `Zstd(ZSTD_DICT_ID)` unconditionally, so the
   dictionary ID is never on the wire or checked; a retrained dictionary silently breaks
   mixed-version sessions.
3. "#1062 has no chosen design" — **true**. "No stopping rule" — **partly wrong**:
   `release-1.0-criteria.md` records why #1062 stays on the critical path. Its weakest link: it
   shows the current preamble is *deployed*, not that it *fails on air*; no such on-air failure is
   recorded.
4. Option (B) as written — **technically unsound**: the `Frame` version byte and the ACK are
   whitened, so a preamble change is never reached by a version check. A "new mode name" works only
   with dual-receive, which is #1062's own staged path.
5. "A1 passed on 2 m" — **overstated**: A1 is defined over HF and is "partly" done (one direction).
   2 m line-of-sight does not fade, so A2 cannot be scored there. Whether #1081's attribution
   reaches the evidence bundle is UNCHECKED.
6. `hpx_hf` plugin set — **correct** (`profile.rs:378-392`); name MFSK16's K=3 return channel too.
7. #1463's "~1/7" is **derived, fairly framed** — but #1463 says `hpx_hf` is untouched (SL6 is
   differential), so it is a ghost *for this release* specifically.
8. September repeater/KISS/ARDOP work as drift — **unfair as written**: at least 11 of 21 commits
   are transmit-safety or §97 fixes. The drift is receive-side work in crates the release does not
   ship (#1297, #1308, #1310).
9. "One commit in five" on tooling — **unmeasured**: state 9–21 %.
10. "Most commits went to…" — **vacuous**: say "no commit since 2026-07-30 produced on-air
    evidence".
11. Sizes, dates, notch default, ledger length, 2026-07-30 bundle — **verified**. Issue counts
    UNCHECKED by the reviewer (they came from the GitHub API in the drafting session).

**Recommendations that could cause harm**

- Slimming `CLAUDE.md`: keep the acceptance table's requirement → test commands somewhere
  machine-readable; keep verification mechanics 1–6, the RX-seam checklist, the review-scope rule,
  and "rebuild BOTH ends".
- A daily gate re-admits #1074's failure mode unless the pre-push hook *tests* reverse dependents.
- Notch off by default: decision 3 says runtime-*optional*; #1457 failed its negative control (the
  instrument broke). Do not change a default to make a red row go away.
- Making Fable optional reverses a 2026-08-02 maintainer rule; change it where it was set.

**Missed**

1. A2 scoring and #1081 attribution belong in the on-air step's exit condition.
2. G0 may be more than a purchase (the FT-991A receive path fails offline too).
3. The release check must list the known-red/ignored rows (#1457, #1351), or it is green by
   selection.
4. The compression dictionary ID.
5. `onair-execution-plan.md` was last updated 2026-07-23 and needs re-baselining.

**Overall:** the diagnosis (deadlock between #1062, the campaign and G0; review producing work
faster than one person closes it; scope about 4× the stated release) is supported by the repo.
Three load-bearing claims were wrong or overstated (option B, A1 on 2 m, safety fixes as drift);
with those fixed and the missed items added, publishable.

**Disposition:** all corrections and missed items were applied in commit `c1b26bf`.
