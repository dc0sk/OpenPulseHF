---
project: openpulsehf
doc: docs/dev/reviews/review-v017-wire-declaration.md
status: resolved
last_updated: 2026-10-01
---

# Adversarial review — v0.17.0 wire-format declaration (M1)

One round by Fable (read-only, no files edited), prompted for falsification, on the uncommitted diff
to `docs/dev/design/protocol-wire-spec.md` and `docs/dev/project/workplan.md` on top of `28b7d103`.

## Consumer

Anyone rebuilding two stations for the M3 2 m campaign, and the M4 release check. Found by
`grep -n "wire format" docs/dev/project/workplan.md docs/dev/onair-execution-plan.md`.

## Prior art

`protocol-wire-spec.md` §3.2a's `WIRE_VERSION` freeze rule (#1191, #1204), which the declaration
reuses rather than restates. Found by `git log -S WIRE_VERSION --oneline v0.16.0..HEAD`.

## Twins

None: the spec is the only normative byte-layout document; `hpx-session-state-machine.md` and
`peer-query-relay-wire.md` cover states and the `OPHF` envelope. Found by
`grep -ln "WIRE_VERSION\|OPZ1" docs/dev/design/*.md docs/dev/*.md`.

## Prompt

Falsify, do not agree. Check against the code and report true/false with file:line: (a) `WIRE_VERSION`
is `0x01` and the §3 freeze rule is as described; (b) the preamble (#1062) is plugin-level, outside the
byte layout; (c) the §7.1 framing — zstd BE size prefix, LZ4 LE, `OPZ1`|tag|payload with tags 0/1/2,
receiver always unpacks, sender opts in; (d) the decoder ignores the enum's `dict_id`, and the zstd
header-ID claims; (e) "the byte did not move across" the 417 commits since `v0.16.0`; (f) what the
declaration omits that someone rebuilding two stations would need. Every positive grep names a
positive control. Does this block Release 1? If not, say so.

## Verdict

**Fix first (small) → all fixed.** Does not block Release 1 (documentation only).

- (a)–(d) true (`handshake_wire.rs:53`, `plugins/bpsk/src/demodulate.rs:16`, `compression.rs:61,77,114,170-174`,
  `server.rs:84,880,1133,2231`); dictionary ID confirmed from the asset bytes; the mismatch test re-run, 1 passed.
- (e) **Misleading** — `v0.16.0` has no `handshake_wire.rs`; it speaks a JSON handshake, and the byte was
  introduced by #1189. **Reworded** to "replaced, not versioned".
- (f) **Fixed:** the dictionary's sha256 added; the declaration is pinned to the `v0.17.0` tag when it is
  cut (M4), same-commit builds until then; pack scope stated (session body only); the `u32::MAX`
  encoder-failure prefix documented, with why `pack` never sends it.
- (f4) The weekly-log row "2026-09-28" for M1 work done 2026-10-01: **kept** — the column is "week of",
  and 2026-10-01 falls in that week.
