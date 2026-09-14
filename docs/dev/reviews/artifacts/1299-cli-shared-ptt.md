---
project: openpulsehf
doc: docs/dev/reviews/artifacts/1299-cli-shared-ptt.md
status: review
last_updated: 2026-09-14
---

# Decision record — moving the CLI's three keying sites onto `SharedPtt` (#1299b)

## Consumer

Who keys in production, by `file:line`:

- `crates/openpulse-cli/src/commands/transmit.rs:18` / `:22` — `openpulse transmit`, raw
  `assert_ptt`/`release_ptt` around `engine.transmit_with_fec_mode`.
- `crates/openpulse-cli/src/commands/calibrate.rs:201` / `:202` — `calibrate ptt`, which **measures**
  the assert→release latency against `PTT_TARGET_MS = 50` (`calibrate.rs:150`).
- `crates/openpulse-cli/src/commands/calibrate.rs:328` / `:343` — `calibrate drive` (`run_drive`).
- The funnel they must join: `openpulse_radio::SharedPtt::key_as` (`shared_ptt.rs:116`), already used
  by the daemon (`lib.rs:1807` `keyed_transmit`), ARDOP (`bridge.rs:501`), KISS (`bridge.rs:131`) and
  the repeater (`repeater/lib.rs:341`).
- The controller builder that must gain `+ Send`: `crates/openpulse-cli/src/radio.rs:5-9`, returning
  `Box<dyn PttController>` today.

## Prior art

- `crates/openpulse-kiss/src/lib.rs:77` — `bridge.ptt.spawn_watchdog(None)`, the same migration
  already done for KISS under this issue's first half, gated by an acceptance-table row.
- `crates/openpulse-daemon/src/lib.rs:1807` `keyed_transmit` — the RAII-guard shape to copy,
  including its "no `block_in_place` in here" constraint.
- `crates/openpulse-daemon/tests/ptt_keys_every_daemon_transmit.rs:140-210` — the source-scan shape
  (`production_prefix` + `blank_spans`, validated against a planted call) for banning a raw keying
  call, rather than an allowlist that rots.
- `crates/openpulse-repeater/tests/abnormal_exit_release.rs` — the unwind-release shape.
- **Not reused, deliberately:** `ptt_builder::build_ptt` (`openpulse-radio/src/ptt_builder.rs:3`).
  #1258 names the CLI as its drifted eighth arm, but `generic` is CLI-only and absent there, `"none"`
  is `Ok(None)` vs the daemon adapter's `Some(no_ptt())` (`server.rs:2300`), and
  `ptt_wiring_integration.rs` pins `"CM108"`/`"GPIO"` strings from the CLI's own `.context(...)`
  (`radio.rs:25,35`). Deduping the builders is a separate change.

## Twins

- **The daemon**, which shares `DEFAULT_PTT_MAX` (`server.rs:703`) — checked and **NOT affected**,
  see the Verdict's scoping correction.
- **ARDOP's manual `PTT TRUE`** and `SharedPtt::hw_assert`/`arm` (`shared_ptt.rs:223,241`) — paths
  that bypass `key_as` entirely. In scope for #1257 (does a manual key get a leader?), not here.
- `calibrate ptt` is its own twin-of-sorts: it is an **instrument measuring the thing being changed**,
  so it must be pinned as unaffected rather than merely still passing.

## Prompt

Two rounds with Fable (2026-09-14), then a maintainer decision on the one question neither round
could settle from the tree. Full text: `1257-leader-delay-fable-review.md` and
`1257-followup-fable-review.md`.

Round 2 was opened because round 1's verdict called the CLI "one caller" and I found three sites —
and because I believed one of them was a long tuning carrier that a watchdog would cut off.

## Verdict

**Round 2 refuted my own premise, which is the more useful half.** `calibrate.rs:328` is **not** a
tuning carrier. `run_drive` keys, transmits **three** uncoded OFDM52 frames of a 255-byte payload,
sleeps 150 ms, polls ALC 4×80 ms, releases — about **3 s** per iteration, re-keyed each iteration.
I asserted "deliberate long key" from the function's name and its `assert_ptt` without reading it.
Verified by reading `calibrate.rs:310-345`. There is no `tune` command anywhere in the CLI or daemon.

So there is **no legitimate exception**: all three sites move onto `SharedPtt`.

### The real hazard, and my correction to the review's scoping

A single **legitimate** frame can exceed the watchdog. From the tree's own numbers: an SL2 BPSK31+Rs
frame is 66.6 s, or 131.8 s at two RS blocks (`engine.rs:2109`), and `Concatenated`/`SoftConcatenated`
cost **3.55×** raw against `Rs`'s 1.77× (`engine.rs:234-236`) — so BPSK31 + Concatenated at 255 B is
of order **265 s** against `DEFAULT_PTT_MAX = 180 s` (`shared_ptt.rs:27`). A migrated CLI would
force-release such a frame ~85 s early while the engine kept writing audio into an unkeyed rig.

**The review said "the daemon shares this hazard". It does not, and the distinction matters because
it decides whether this is a live defect or one the migration would introduce.** The daemon's send
takes its FEC from `engine.ota_tx_fec()` (`server.rs:1961-1963`) — the ladder's per-rung FEC — and
every profile pairs BPSK31/BPSK63 with `Rs` only (`profile.rs:381-383,447`). `Concatenated` is not
reachable from daemon config at all: `grep -rn concatenated crates/openpulse-daemon/` is empty, and
the same filter matches `crates/openpulse-cli/src/main.rs:41` where `parse_fec` does accept it. So
the daemon's worst case is ~132 s, inside the deadline. **CLI-only, and introduced by the migration
rather than pre-existing.**

### The ~265 s figure was right by accident — measured after the design was settled

Both the review and this artifact derived "of order 265 s" from `max_frame_samples ×
fec_slice_factor` (3.55× for `Concatenated`). **That is not the mechanism**, and the first version of
the premise test failed because of it: at a 200 B payload BPSK31+Concatenated is **134 s**, well
inside the deadline, and the guard would have been unreachable.

Measured with the new predictor over every BPSK rung × every `FecMode` (probe run and deleted
2026-09-14), the real cause is the **RS block boundary**: 223 B + `Frame::WIRE_OVERHEAD` needs a
SECOND 255-byte block, which doubles the airtime. BPSK31 worst cases:

| FEC | max airtime | at |
|---|---|---|
| `Rs` / `RsStrong` / `RsInterleaved` | 131.8 s | 223 B |
| `LdpcHighRate` | 111.9 s | 250 B |
| `Ldpc` | **197.9 s** | 250 B |
| `Concatenated` | **264.7 s** | 223 B |
| `SoftConcatenated` | **265.0 s** | 223 B |
| `Turbo` | **296.2 s** | 250 B |

So the guard is reachable for four combinations, not one, and the worst case is Turbo at 296 s — not
Concatenated. No rung above BPSK31 exceeds the deadline at any payload. The same 191/223 B boundary
is already documented in `CLAUDE.md` under "RsStrong is free ONLY ≤191 B"; this is that boundary
showing up on the airtime axis instead of the goodput one.

**The lesson is the one this repo keeps paying for**: a number that is numerically right can still be
derived from the wrong mechanism, and the wrong mechanism is what gets reused. Had I taken 265 s on
trust and written the test at 200 B, the guard would have shipped unreachable.

### Maintainer decisions (2026-09-14)

1. **Refuse before keying.** The CLI derives the frame's airtime bound from mode + FEC + payload
   length *before* it keys, and refuses the emission with an actionable error when the bound exceeds
   `max_duration()`. Nothing is half-transmitted and the watchdog keeps one meaning. This needs a
   **public** airtime helper — `fec_slice_factor` is private (`engine.rs:247`) — and note
   `burst_cap_samples` is the wrong source: it is ×4-clamped (`engine.rs:2089-2101`) and would refuse
   a legal BPSK31+Rs frame at 131.8 s. This is a production consumer, not an instrument, so a public
   API is the right answer here rather than the usual "a probe needing private access is a unit test".
2. **`DEFAULT_PTT_MAX`'s citation is corrected.** Its doc comment attributes 180 s to "Part 97
   duty-cycle guidance"; no Part 97 provision setting a 180 s continuous-transmission limit could be
   found (§97.119 is a 10-minute **ID interval**, a different thing). Re-documented as a chosen
   engineering bound with its real rationale, regulatory attribution dropped. A false regulatory
   citation on a transmit-safety constant is exactly the claim that gets quoted back as fact.

### Scope: two PRs, A before B

**A = this change (#1299b).** All three sites onto `SharedPtt`; `radio.rs:9` gains `+ Send` (every
production implementor is `Send` by field inspection — the compile is the proof); the
refuse-before-keying guard; the `DEFAULT_PTT_MAX` doc correction.

**B = #1257's leader delay in `key_as`**, merged after. Split rather than combined: B still has open
inputs A does not (no config field exists for a leader, and the CLI loads no config at all, so it
needs a flag; and the manual-key paths that bypass `key_as` need a stated answer). The hazard of a
knob the CLI does not honour is prevented by merge **order**, not by co-location, and a revert of B
must not take A with it.

### What stops the fourth hand-rolled keying path

A **flat construct ban**, not an allowlist: a source scan asserting no `assert_ptt`/`release_ptt`/
`hw_assert`/`hw_release` in the production prefix of any file under `crates/openpulse-cli/src`,
validated against a planted call. An allowlist is what rots into the fourth path — and since round 2
established there is no exception, the ban can be unconditional.

### Refused as scope creep, filed instead

- `run_drive` swallows `engine.transmit` errors (`calibrate.rs:331`, `let _ =`) and registers only
  `OfdmPlugin` (`:318`), so `--mode BPSK31` transmits nothing three times and reports the ALC of an
  unmodulated keyed rig as a measurement. Pre-existing; filed, not fixed under a keying PR.
- The `ptt_builder` dedupe (#1258 leftover), and a `Send` supertrait on the public trait (20 impls).
- The daemon-side deadline policy question, which does not arise there today.
