---
project: openpulsehf
doc: docs/dev/design/session-profiles.md
status: draft
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
| `robust` | the **same** ladder, capped at **SL6** | SL1–SL6 (MFSK16, BPSK31/63/100/250, QPSK250-D), all coded | ≤ 500 Hz | poor conditions, narrow filters, small or non-linear PAs (no OFDM PAPR), drifting oscillators |

- **One ladder, two caps.** `robust` is not a second rung table. `SessionProfile` gains a local-only
  field, `max_level: Option<SpeedLevel>`. It is **excluded from `fingerprint()`**, like the SNR floors
  and `nack_threshold`. Both profiles therefore advertise the same ladder fingerprint in the handshake,
  so a `fast` station and a `robust` station keep adaptive OTA instead of falling back to fixed mode
  (`record_verified_peer`, `openpulse-daemon/src/lib.rs:2166`).
- **The cap is enforced on both rate paths.**
  - On the receiver-led OTA path (daemon), `OtaRateController::new` seeds `max_level` from the profile
    (`ota_rate.rs:175`). A robust *receiver* never recommends above SL6. A robust *sender* clamps any
    peer recommendation through `adopt_recommendation` (`:300`).
  - On the sender-led path (ARDOP, `RateAdaptationPolicy`), `start_session` applies the profile cap the
    way `set_max_tx_level` does (`rate_policy.rs:59`).
  - An operator or host cap (`ota_max_level`, ARDOP `ARQBW`) combines with the profile cap as the
    **lower** of the two; it never raises it.
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

So with `fast` on ARDOP, `ARQBW 2000` caps at SL5 (BPSK250) instead of SL11, and no `ARQBW` value
reaches QPSK250-D. The fix is to size modes from the registered plugin (the trait method exists; the engine does not
expose it yet), not from the table.
It is filed as an M2 item.

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
