---
project: openpulsehf
doc: docs/dev/reviews/review-rig-power-lower-only.md
status: resolved
last_updated: 2026-10-14
---

# Review: on-air runners only lower a rig's power (2026-10-14)

This is the adversarial review (Fable) of commit `5ebf8785`. It was read-only and ran nothing that
could reach a radio. The change serves the station owner's three rules for the FT-818 on rpi53:
- tooling never increases a rig's power;
- the FRONT antenna stays selected;
- the mode is checked, and DIG/USER-U is set on the radio.

## Prompt

Falsify five things, with file and line:
1. Can any script, profile default or crate path still raise a rig's power? This covers every on-air
   script, the daemon/CLI/radio crates, and the helper's numeric edge cases.
2. Can anything change the FT-818's antenna, or transmit unintentionally, during setup?
3. Is `keep` honoured on every path that sets mode?
4. Are the test's assertions sound, and can the direct-send grep be bypassed?
5. Are the edited docs true and consistent?

Also: does this block Release 1?

## Verdict

**Not a Release 1 blocker** (scripts, profiles and docs only). Six should-fix findings, all taken in
the same branch before the PR.

## Findings and disposition

| # | Finding | Class | Disposition |
|---|---|---|---|
| 1 | The FT-818 profile dropped the global `TEST_MODE_RIG`, which the runner's preflight still read under `set -u`, so every run aborted before keying | should-fix (regression) | Fixed. The preflight now checks each station against its own mode and no longer reads the global. |
| 2 | Twin runner: no band guard, and its example profile defaulted to HF. The FT-818's FRONT/REAR choice is per band, so a frequency on another band changes the antenna | should-fix | Fixed. The run refuses a frequency outside `BAND_MIN_HZ`–`BAND_MAX_HZ` (default 2 m), and the example profile now uses 144.640 MHz. The daemon's `set_freq` has no band check: parked below. |
| 3 | The raw FT-991A command `w 'RG0;'` went to every station B, the FT-818 included. That is ASCII sent into a 5-byte binary CAT protocol | should-fix | Fixed. It is sent only for models in `RG0_RAW_MODELS` (default 1035). For the FT-991A the remote command is byte-identical to before; the FT-818 and IC-9700 now get `echo na`. |
| 4 | The mode was only printed or warned about. Nothing stopped a run with the FT-818 left in USB | should-fix | Fixed. With `keep`, `TEST_MODE_EXPECT_A/B` (twin runner: `*_TEST_MODE_EXPECT`) must name the reading; unset or different stops the run before keying. |
| 5 | The direct-send tripwire covered two files and one spelling | should-fix | Fixed. It scans every script for `L`, `\set_level` and raw `PC`/Icom-hex power commands, skips comment lines, and runs a positive control. A level held in a variable (`L $lvl`) is still not caught; stated in the test. |
| 6 | Stale docs: profile and guide still said "laptop / localhost", "virtual COM port", and the preflight advised raising power in the profile | should-fix | Fixed. |
| 7 | What can key during setup in the FT-818 flow: power cycle, ATU tune, the IC-9700 PTT check (side A only), `onair-dual-capture.sh check_antenna`, `calibrate ptt`. Nothing keys station B during setup | park | Power cycle and tuner are off in the profile; no change made |
| 8 | Helper edge cases (`1.`, `1e0`, `0,5`, `10`, empty) all fail safe, i.e. power is left alone. Multi-line rigctl output is untested | park | — |
| 9 | `L COMP`, `L MICGAIN` and the preflight NB/NR/SQL/VOX settings still go to the FT-818 unconditionally. They are no-ops on model 1041 today | park | Gate on model when next touched |
| 10 | Under `set -e`, a false final test made `restore_rig_state_*` return 1, which aborted cleanup before station B's restore and `rigctld` stop (pre-existing) | should-fix (taken) | Fixed: both functions end in `return 0` |
| 11 | The ledger's sabotage claims cite no run log | park | The re-run below is recorded with the commands |

**Parked:** the daemon's `set_freq` control command (`crates/openpulse-daemon/src/server.rs`) sets any
frequency without a band check, so a control client could change the FT-818's band, and with it the
antenna. It is not reached by the runners.

## Consumer (of the change reviewed)

`scripts/run-onair-twin-ota.sh` and `scripts/run-onair-ic9700-ft991a.sh`, run by the operator for the 2 m
pair (maintainer to-do, M3). They were found by
`git grep -n "RFPOWER\|TEST_MODE_RIG" -- scripts docs/config`.

## Prior art (of the change reviewed)

`scripts/lib/host-build-lock.sh` and `scripts/test-host-build-lock.sh` set the pattern: a sourced
helper plus a test that the gate runs. The ic9700 runner's 2 m band guard is the model for the twin
runner's guard.

## Twins (of the change reviewed)

`scripts/run-onair-tx500-kx3.sh` is the HF pair. It sends no power command (the tripwire scans it) and
sets mode with no `keep`; it is not this pair and is unchanged.
