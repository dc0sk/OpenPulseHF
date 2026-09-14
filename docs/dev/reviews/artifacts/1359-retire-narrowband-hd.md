---
project: openpulsehf
doc: docs/dev/reviews/artifacts/1359-retire-narrowband-hd.md
status: review
last_updated: 2026-09-14
---

# Decision record — retiring `hpx_narrowband_hd` and unadvertising five modes (#1359)

A maintainer decision, taken twice: once on the issue as filed, and again after investigation
changed its scope.

## Prompt

**First ask.** #1359 reported five modes advertised in `PluginInfo::supported_modes` that cannot be
modulated at 8 kHz — `8PSK2000`, `8PSK9600`, `8PSK9600-RRC`, `QPSK9600`, `QPSK9600-RRC` — each
refusing with `ModemError::Configuration`. Three options were put with their costs: stop advertising
them, make them reachable, or mark them dormant and teach the listing surfaces to filter.
**Decision: stop advertising them.**

**Second ask, because implementing the first turned up a fact it did not contemplate.** Two of the
five are not strays: `QPSK9600-RRC` (SL8) and `8PSK9600-RRC` (SL9) are the *entire*
`hpx_narrowband_hd` profile, which is selectable by name (`profile.rs`, `PROFILE_NAMES`), offered by
the panel's own list, and documented in the config template, the CLI guide, the ladder doc, features
and the book. Unadvertising the modes alone would leave a selectable profile referencing modes no
plugin supports. Four options were put — retire the profile, refuse it at startup, do only the three
safe modes, or make 48 kHz reachable. **Decision: retire the profile too.**

## Verdict

Recording what came back, in one place, because the two decisions above are split across the Prompt
section and a third answer arrived after implementation began.

1. **Stop advertising the five modes** (`8PSK2000`, `8PSK9600`, `8PSK9600-RRC`, `QPSK9600`,
   `QPSK9600-RRC`) — maintainer, on the issue as filed. Not "make them reachable", not "filter them
   at the listing surfaces".
2. **Retire `hpx_narrowband_hd` as well** — maintainer, after the profile entanglement surfaced. Not
   "refuse it at startup", not "do only the three safe modes".
3. **Narrowed against decision 2's own wording, deliberately** — see the section below. The decision
   said "remove … its two modes together"; the modes carry working 48 kHz loopback tests, so the DSP
   and its tests are KEPT (retired dormant) and only the advertisement is removed. Full deletion
   stays a follow-up, because that direction is reversible and the other is not.

**A later adversarial review (Fable, 2026-09-14) found this change incomplete twice, after the first
gate run**, and both findings are folded in above rather than deferred:

- The **testmatrix excusal lists** (`KNOWN_LIMITATION_MODES`, `WIDEBAND_POST_V1_MODES`) held exactly
  these five names — a fourth registry mirror the sweep missed. `GATE: FAIL`, one test.
- **Six living-doc lines still asserted the old status**, two of which the testmatrix fix itself had
  just falsified. Found by a literal-name grep, not by recall; the census is pasted into the commit.

Two lines the review flagged were checked and are **not** defects — `architecture.md:84` and
`hpx-waveform-design.md:21` claim "RRC-superseded" and make no registration claim. Confirmed is not
correct in either direction.

## Verified, not assumed

The premise came from an earlier review; I checked it before acting, because the whole decision rests
on it:

- **No production caller constructs `AudioConfig` with a non-default rate.** The only hits in the
  daemon, CLI, ARDOP and KISS are a test mock.
- **`sample_rate` is absent from the TOML schema entirely** (`grep sample_rate crates/openpulse-config/src/lib.rs` → nothing), so an operator cannot change it.

So the engine is 8 kHz always, and selecting `hpx_narrowband_hd` produced a station whose every
transmit failed at modulate — on a profile the docs offered.

## Where the decision was narrowed against its own wording

The chosen option said "remove … and its two modes together" and reasoned "nothing loses a
capability it actually had". Implementing it surfaced that the modes **do** have a capability: both
carry working 48 kHz loopback tests (`qpsk9600_loopback_48k`, `qpsk9600_rrc_loopback_48k`, and the
psk8 equivalents) that round-trip at fc = 12 kHz. They are functioning DSP the engine cannot reach,
not broken code.

This repo's standing rule is that unreachable code is **retired dormant, not deleted** — kept
compiled and tested, with a tag carrying an issue reference and a retention rationale. The rationale
exists and is concrete: `docs/dev/project/backlog.md` item 12 Phase 1 (sample-rate generalization)
and `wide-channel-extension.md` item 1.6 both name these waveforms as what that work unblocks.

**So the decision was implemented as: unadvertise the five, retire the profile, keep the DSP and its
48 kHz tests.** That satisfies the intent — nothing selectable can now fail at modulate — without
deleting tested capability. If full deletion was meant, it is a small follow-up; the reverse would
not have been.

## What the existing machinery caught

Each of these failed rather than passing quietly, which is the argument for the ratchets:

- **`roadmap_profile_table_matches_profiles`** failed on removing the profile, naming the drifted
  doc table.
- **`channel_loopback.rs` pinned a count of 2** "rungs that cannot modulate at 8 kHz" — exactly this
  profile's two. Now pinned at **0**, so a future unmodulatable rung fails instead of being counted
  as expected.
- **`soft_demod_conformance`'s `UNDRIVABLE_AT_8K`** went from five entries to empty, and the sweep
  still passes — which is what confirms all five are genuinely unadvertised and nothing else refuses.
- Three separate name lists had their own copies: `PROFILE_NAMES`, the panel's `PROFILES`, and
  `profile_modes_resolve.rs`.

## Consumer

- `SessionProfile::by_name` (`profile.rs`) — the resolution point for `[modem] profile`.
- `SessionProfile::PROFILE_NAMES` — what the CLI lists.
- `apps/openpulse-panel/src/ui.rs::PROFILES` — what the operator panel offers.
- `PluginRegistry::get(mode)` — dispatch, which is why `supported_modes` is the advertisement that
  matters rather than a label.

## Prior art

- The repo's own "retire dormant, not deleted" rule, and its warning against parking code behind a
  default-off feature flag — which is why the DSP keeps its ordinary tests rather than moving behind
  a `cfg`.
- `UNDRIVABLE_AT_8K` (added in #1360) already pinned these five by name; this change empties it
  rather than deleting it, so the shape stays available for the next mode that starts refusing.

## Twins

- **`hpx_wideband_hd`** is the obvious sibling — also an "HD" profile. Checked: its rungs are
  64QAM2000-RRC and friends, which modulate at 8 kHz, so it is reachable and stays.
- **`8PSK2000` (plain)** was the third unadvertised mode and is not a profile question at all: its
  own source comment already said it "is not viable for engine / on-air decode", so advertising it
  offered operators a mode its author had documented as unusable.
- Not a twin: the `-RRC` 2000-baud modes, which are the operational ones the ladders use.
