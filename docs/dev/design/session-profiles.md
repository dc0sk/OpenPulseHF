---
project: openpulsehf
doc: docs/dev/design/session-profiles.md
status: review
last_updated: 2026-10-01
---

# Session profiles for Release 1: `fast` and `robust`

Work plan decision 18 (maintainer, 2026-10-01). Milestone M2.

## Problem

`SessionProfile::PROFILE_NAMES` lists eleven profiles (`profile.rs:128`). Release 1 ships one ladder,
`hpx_hf`. The other ten either run waveforms that are not on the release ladder (`hpx_pilot*`,
`hpx_wideband*`, `hpx_narrowband`), or are earlier ladders that the fade-aware re-seat of `hpx_hf`
superseded (`hpx500`, `hpx_modcod`, `hpx_ofdm_hf`). Two of them are still defaults:
- `[ardop] adaptive_profile` defaults to `hpx500` (`openpulse-config/src/lib.rs:778`), and an empty
  value falls back to it too (`openpulse-ardop/src/main.rs:115`). That ladder's coherent
  `QPSK250`/`QPSK500` rungs decode ~0 % on `moderate_f1` (#923).

The operator needs two choices, not eleven:
- **fast**: performance and bandwidth under good conditions;
- **robust**: reliability under poor conditions or with limited gear.

## Decision

| Name | Ladder | Rungs | Occupied BW | For |
|---|---|---|---|---|
| `fast` | today's `hpx_hf`, unchanged | SL1–SL14 (MFSK16 … OFDM52-64QAM + LDPC r≈8/9) | up to ≈2031 Hz (OFDM52) | good conditions, a 2.4 kHz SSB filter, a linear PA |
| `robust` | the **same** ladder, capped at **SL6** | SL1–SL6 (MFSK16, BPSK31/63/100/250, QPSK250-D), all coded | ≤ 500 Hz | poor conditions, narrow filters, small or non-linear PAs (single-carrier, so no OFDM peak-to-average stress) |

- **One ladder, two caps.** `robust` is not a second rung table. `SessionProfile` gains a local-only
  field, `max_level: Option<SpeedLevel>`. It is **excluded from `fingerprint()`**, like the SNR floors
  and `nack_threshold`. Both profiles therefore advertise the same ladder fingerprint in the handshake,
  so a `fast` station and a `robust` station keep adaptive OTA instead of falling back to fixed mode
  (`record_verified_peer`, `openpulse-daemon/src/lib.rs:2166`).
- **The cap is a separate field, never an operator bound.** Both existing caps *overwrite*:
  `OtaRateController::set_level_bounds` replaces `max_level` (`ota_rate.rs:201-203`), and it is called
  whenever `ota_min_level` or `ota_max_level` is set (`server.rs:255-257`) and by the
  `OtaSetLevelBounds` control command with `None` for an empty field (`openpulse-daemon/src/lib.rs:2934`).
  `RateAdaptationPolicy::set_max_tx_level` replaces `max_tx_level` (`rate_policy.rs:59-64`) on every
  ARQBW change (`bridge.rs:300-303`). Seeding the profile cap into either field would let any of those
  calls silently un-cap `robust`. So:
  - `OtaRateController` gains `profile_cap`, set once from the profile in `new`; `hi()` is
    `min(profile_cap, max_level, top mapped level)`. A robust *receiver* then never recommends above
    SL6 (`rx_candidates` and `next_mapped` are bounded by `hi()`, `:240`, `:353`), and a robust
    *sender* clamps any peer recommendation through `adopt_recommendation` (`:300-306`).
  - `RateAdaptationPolicy` gains the same `profile_cap`; the effective ceiling is
    `min(profile_cap, max_tx_level)`, and `defined_modes()` lists only levels at or below the cap, so
    the ARQBW mapping cannot pick a level above it.
  - **Tests:** setting `ota_min_level` alone, sending `OtaSetLevelBounds` with an empty max, and an
    ARQBW change to 3000 each leave a `robust` session at or below SL6. Each test must fail with the
    cap seeded into the old field.
- **At the cap, the decision is `Hold`.** Today SL6's ceiling (11 dB) makes `on_rx_frame` report
  `ClimbOnSnr` every frame when SL6 is the top (`ota_rate.rs:504-511`), so the A2 `OtaRateDecision`
  audit would show a phantom climb on every `robust` frame. When `next_mapped(confirmed) == confirmed`,
  the decision is reported as `Hold`, with a test.
- **ARDOP floors at SL2.** The ARDOP TNC registers no `mfsk16` plugin (`openpulse-ardop/src/main.rs:99-106`)
  and has no K=3 robust ACK, yet both profiles map SL1 = MFSK16. Today's `hpx500` had no SL1, so the
  new default would fire the `main.rs:124-134` warning on every start, and NACK exhaustion at SL2 would
  land on a dead rung (`rate.rs:334-336`, `:394-396`). The ARDOP path therefore runs either profile with
  a minimum level of SL2 (`profile_floor`, which mirrors the cap). The warning goes, and a test shows that NACK
  exhaustion at SL2 stays at SL2. Registering MFSK16 in the TNC is the alternative, but without the
  K=3 ACK the rung would still fail at deep fade; that is post-release work.
- **Names on the wire.** The handshake's `profile_name` string (`handshake.rs:246`, capped by
  `caps::PROFILE_NAME`) carries `fast` or `robust`. Only the value changes; the byte layout does not.
  It is informational: the compatibility decision uses the fingerprint alone.
- **Defaults.** `[modem] profile = "fast"`. `[ardop] adaptive_profile = "fast"`, so that the host's
  `ARQBW` decides the width. An empty name is an error, not a silent fallback.
- **No aliases** (maintainer). `by_name` accepts `fast` and `robust` only. An unknown name fails with
  the list of valid names. A pre-1.0 config naming `hpx_hf` fails loudly.
- **Deleted:** the other ten profiles (`hpx500`, `hpx_modcod`, `hpx_pilot`, `hpx_pilot_rrc`,
  `hpx_pilot_fast`, `hpx_pilot_fast_rrc`, `hpx_ofdm_hf`, `hpx_wideband`, `hpx_wideband_hd`,
  `hpx_narrowband`), with `SCFDMA_QAM_HF_ENTRY_POLICY` if nothing else uses it.
  - Tests that only exercised a deleted profile go.
  - Tests that used one as a handy ladder are retargeted to `fast`/`robust`, or to a test-local
    `SessionProfile` built for the purpose.
  - The plugins stay; git keeps the history.
- **Migration items the deletion touches** (from the review's inventory):
  - the `roadmap_profile_table` test (acceptance row 43), which asserts that `roadmap.md`'s profile table
    equals `PROFILE_NAMES`; the table in `roadmap.md` is updated with it;
  - the REQ-QRM-01 notch apparatus, which runs on `hpx_wideband` by measured choice
    (`openpulse-linksim/src/lib.rs:1122-1142`); it gets a test-local `SessionProfile` with the same
    rungs, so the slow suite's numbers stay comparable;
  - testmatrix use-cases `adaptive_hpx500`, `adaptive_wideband` and `adaptive_ofdm_hf`
    (`apps/openpulse-testmatrix`, `matrix.rs:27-30`, `runners/mod.rs:57-60`) and their
    `comparison.json` keys;
  - the handshake tests that pin the literal `"hpx_hf"` (`handshake_integration.rs:338,543`,
    `handshake_kat.rs:41`) and the spec example (`protocol-wire-spec.md:321`). The KAT's signed bytes
    change with the string, so its vector is regenerated;
  - `ota_rate_lockstep.rs:25`, `twin_daemon_bridge.rs:172`, `ota_ack_capture_stream.rs:186`, both ARDOP
    tests, `mode_advisor.rs:151-165`, `openpulse-cli/src/commands/daemon.rs:543`,
    `openpulse-daemon/src/protocol.rs:664`;
  - scripts `run-onair-twin-ota.sh:38`, `demo-hwloop-panel.sh:88`, `run-twin-station-audio.sh:24,84`,
    `docs/config/onair-twin-ota.example.sh:56`, and `openpulse-config/src/lib.rs:706,778,1078`.

  The benchmark (a required CI check) and `requirements.yaml` name no profile (review item 6, with
  positive controls).
- **Docs.** Living docs are updated: `mode-fec-ladder.md`, `cli-guide.md`, the config examples and the
  on-air scripts. Historical records (the traceability ledger, reviews, research) keep the old names.

## Found while designing (same milestone, separate PR)

The ARDOP `ARQBW` cap maps Hz to a level through a hand-kept table,
`openpulse_qsy::bandplan::occupied_bandwidth_hz` (`bandplan.rs:295`). That table is a **twin** of
the plugins' own `ModemPlugin::occupied_bandwidth_hz` (e.g. `plugins/ofdm/src/lib.rs:175`), and the
two disagree:
- `OFDM52` is 3200 Hz in the table and 2031.25 Hz in the plugin, which its own test checks.
- `MFSK16`, `QPSK250-D` and every `OFDM52-*` variant are missing from the table, and
  `max_speed_level_for_bandwidth` drops a mode it cannot size.

So no `ARQBW` value reaches QPSK250-D (SL6), and with `fast` on ARDOP an `ARQBW` of 2032 Hz or more,
which should reach SL14, also stops at SL5 (BPSK250). `ARQBW 2000` correctly stops below OFDM52
(2031 Hz > 2000) once QPSK250-D is sized. The fix is to size modes from the registered plugin (the trait method exists; the engine does not
expose it yet), not from the table.
It is filed as an M2 item.

**Wire.** `profile_name` is used only for logging in production (`openpulse-daemon/src/lib.rs:2184`).
The fingerprint of `fast` equals today's `hpx_hf`, so builds from before and after the rename still
interoperate. The v0.17 declaration covers layout and `WIRE_VERSION`, so the string's value is not a
wire change. Its length fits `caps::PROFILE_NAME` (24).

**Unchecked.** Tolerance of oscillator drift is not claimed for either profile. No measured drift
figure exists for the single-carrier rungs, and OFDM has its own CFO estimator.

## Consumer

- `[modem] profile` / `ota_profile` → `server.rs:234` → `SessionProfile::by_name` →
  `OtaRateController`;
- `[ardop] adaptive_profile` → `openpulse-ardop/src/main.rs:113` → `start_adaptive_session`;
- CLI `--profile` (`openpulse-cli/src/cli.rs:403`);
- the daemon control command `StartOtaSession { profile }` (`openpulse-daemon/src/protocol.rs`);
- the handshake `profile_name` and `profile_fingerprint` fields (`handshake.rs:246-247`, `:414-415`).

Found by `grep -rn "by_name\|profile_name" --include=*.rs crates`.

## Prior art

- `docs/dev/design/ladder-versioning.md`: the fingerprint, and why local policy is excluded from it.
  The cap reuses that rule.
- `OtaRateController::set_level_bounds` (operator bounds), `RateAdaptationPolicy::set_max_tx_level`
  (ARQBW).
- ARDOP's own `ARQBW` model: width as the operator's knob.

Found by `grep -rn "max_level\|set_max_tx_level" --include=*.rs crates`.

## Twins

- `bandplan::occupied_bandwidth_hz` vs `ModemPlugin::occupied_bandwidth_hz`; see "Found while
  designing".
- The ARDOP default and the empty-name fallback, both `hpx500`; both are removed here.

Found by `grep -rn "\"hpx500\"" --include=*.rs crates`, and
`grep -rn "fn occupied_bandwidth_hz" --include=*.rs`, which matched the trait (`plugin.rs`), all ten
plugins, the bandplan, and four test stubs (one in `engine.rs`, three in `openpulse-modem/tests`).
