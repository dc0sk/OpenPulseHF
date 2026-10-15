---
project: openpulsehf
doc: docs/dev/reviews/review-review-followups-2026-10-15.md
status: resolved
last_updated: 2026-10-15
---

# Review: follow-ups to the documentation review — lower-only keyers, ARDOP PTT (2026-10-15)

Read-only adversarial review (Fable) of the follow-ups to
`docs/dev/reviews/review-docs-revision-2026-10-15.md`. The work was presented as uncommitted changes
on top of `origin/main` `d6b7c80a`; during the review it was committed as `6f2f8356`, `397d6e78`,
`e289d5ee` and `fe8628f3` (same content plus a rebuilt `docs/openpulse-book.pdf`), so the diff
reviewed is `git diff origin/main..HEAD` (19 files). No cargo build or test was run here; the one
harness run was `bash scripts/test-rig-power.sh` (fake rig, no hardware). Sabotage checks were run
with `python3 -I` against copies of the keyers in the session scratchpad, never against the tree.

## Prompt

Falsify, do not agree. Priorities: (1) any path where `keyplay*.py` can still raise power, key a
non-FT-991A, send bytes to an FT-818 that could change its state (5-byte binary CAT framing), or
leave the rig keyed; (2) any path where the ARDOP TNC can now transmit unkeyed, or where a
previously working station (VOX, `none`, cm108 plugged in late, rigctld starting after the TNC) now
fails to start or refuses wrongly — every caller of `with_trust_relay_ptt` and the `ArdopServer`
constructors; (3) whether the `test-rig-power.sh` Python harness is sound (`raised`, the fake rig's
PC parsing, the scan regex against other spellings); (4) whether the comment and doc edits are true
against the code, including the claim that `run-onair-ic9700-ft991a.sh` enforces `TEST_MODE_EXPECT`
under `keep` and exits before keying. Also: does any finding block Release 1?

## Verdict

**No Release 1 blocker.** Nothing in the diff can raise a rig's power, key a rig that is not an
FT-991A, send a documented FT-818 opcode, or make the ARDOP TNC transmit unkeyed; the harness
catches a keyer that raises power and a keyer that skips the ID check (both sabotage runs failed as
they should). The findings are second-order: bytes sent to a rig the keyer has just refused, a scan
regex with no positive control, a comment that is false in a new way, a doc sentence that is literally
false for one runner action, and three pre-existing twins the change did not reach.

Counts: must-fix 0, should-fix 7, nit 4.

## Findings

| # | Finding | Class |
|---|---|---|
| 1 | **After refusing a rig for the wrong `ID;`, the keyer still sends 76 bytes to it.** `scripts/onair-sdr/keyplay.py:104-111` (`keyplay_meter.py:105-112`): on any `gate()` abort `main` calls `force_rx()`, which opens the port again and writes `TX0;` four times and `TX;` once per attempt, four attempts when nothing answers. Simulated against a silent port: 79 bytes in all, `ID;` then `TX0;TX0;TX0;TX0;TX;` x4. In the pre-change code the band abort exited without `force_rx` (old lines 80-83); the new code sends more to an unknown rig, not less. Mitigation measured, not assumed: the stream contains only the byte values 0x30 0x3B 0x44 0x49 0x54 0x58, and none is a documented FT-817/818 opcode (PTT 0x08/0x88, freq 0x01, mode 0x07, power 0x0F/0x8F, EEPROM write 0xBC, ...) in any of the five 5-byte alignments, so documented FT-818 state cannot change; what an FT-818 does with an undefined opcode is undocumented. Fix: when the ID check fails, nothing has been keyed and nothing should be sent — return before `force_rx`. The README sentence "they stop unless the rig answers `ID0670;`" (`scripts/onair-sdr/README.md:28`) is not true until then. | should-fix |
| 2 | **The Python power-write scan has no positive control and misses four of six spellings.** `scripts/test-rig-power.sh:63-68` scans with `'"PC(%|[0-9])'`. Probe (`printf` of six planted lines piped through the same `grep -E`): matched `b"PC005;"` and `("PC%03d;" % w)`; missed `f"PC{w:03d};"`, `'PC%03d;'` (single quotes), `"PC" + "%03d;"` and `"PC{:03d};".format(w)`. If a keyer were rewritten with any of those, `grep -rlE` finds nothing, the `for` loop runs zero times and the step passes silently. The shell scan directly above it (`test-rig-power.sh:44-48`) plants five forms and asserts the count; this one plants nothing (verification mechanic 4). The fake-rig drive is the real defence and it does catch a raise (see Consumer), so this is a gap in the second line, not the first. | should-fix |
| 3 | **`split_frame`'s rewritten doc comment is false for the frames it names.** `crates/openpulse-core/src/handshake_wire.rs:335-337` now says "the old JSON handshake has no binary magic and fails the magic check first". `git show v0.16.0:crates/openpulse-core/src/handshake.rs` lines 324-335: the v0.16.0 CONREQ is `HSCQ` + `0x01` + u32 length + JSON, and its decoder checks `b"HSCQ"` and `bytes[4] != 1`. Against today's `split_frame` (`handshake_wire.rs:268-286`) such a frame passes the magic check, passes the version check (`WIRE_VERSION` is `0x01`, line 53), and reads the top two bytes of the u32 length as a u16 of 0, so it dies in the body decode — exactly the "garbled decode error rather than version rejected" the `WIRE_VERSION` comment at lines 44-47 warns about. The "no magic" statement is true of the PQ frames only (`handshake_wire.rs:55-56`, spec section 379). The previous wording was wrong by version; this one is wrong by magic. | should-fix |
| 4 | **"Stop before keying" is not literally true of the A/B runner.** `docs/dev/onair-execution-plan.md:81-83` (and the prompt's own claim): `preflight_check` does enforce `TEST_MODE_EXPECT_A/B` under `keep` and `exit 1`s (`scripts/run-onair-ic9700-ft991a.sh:941-955, 1049-1052`; added in `7a100654` #1513, which is an ancestor of `d6b7c80a`, so the earlier review's finding 4 was already wrong when written). But under `sidea`, `setup_side_a` calls `verify_ptt_control_a` (`:1119`, `rigctl ... T 1`, one second) before `run_side_a_transmit` reaches `preflight_check` (`:1195`); under `run`, `maybe_tune_high_swr_a/b` (`:1753-1756`, keyed tuner cycle, off by default: `ALLOW_TUNER_ON_HIGH_SWR=0` at `:75`) also runs before `preflight_check` (`:1365`). The station on `keep` (side B, the FT-818) is not keyed before the check in the default configuration, and the `sidea` key is a carrier with no DATA-jack audio, so the mode hazard the check exists for is not reached; the sentence should say "before the test transmission" or name the `sidea` PTT probe. | should-fix |
| 5 | **Twin left behind: the KISS TNC still fails open on any PTT build error.** `crates/openpulse-kiss/src/main.rs:180-189`: `Err(e) => { warn!("PTT unavailable; this TNC will transmit WITHOUT keying the rig"); None }`, with the comment "decided in #1285 rather than diverging here". The ARDOP fix (`crates/openpulse-ardop/src/ptt.rs`) does not touch it, and nothing in the book or manual says the KISS TNC still has the defect. `openpulse-kiss` is not a Release 1 crate (CLAUDE.md crate map). | should-fix |
| 6 | **A not-compiled-in `gpio` backend is classed as a retryable failure, so the TNC (and the daemon) start and refuse every key forever instead of refusing to start.** `crates/openpulse-radio/src/gpio.rs:72-74` returns `PttError::Serial("GPIO PTT not compiled in ...")`, while `ptt_builder.rs:16-18` says `GpioPtt::open` returns `PttError::Config` in that case, and `ptt_from_spec` (`ptt.rs:22-34`) wraps every non-`Config` error in `RetryingPtt`. `Cm108Ptt::open`'s pin-range check (`cm108.rs:80-83`) is the same shape. Pre-existing, inherited from `openpulse-radio`, identical in the daemon (`server.rs:2411-2425`); the symptom is a `FAULT` per emission rather than a startup error, which is the failing-closed side. | should-fix |
| 7 | **No traceability entry, acceptance row or milestone status line for substantive changes.** `grep -n "keyplay\|ptt_from_spec\|lower_power_only" docs/dev/project/traceability.md docs/dev/project/acceptance-criteria.md docs/dev/project/workplan.md` finds nothing for this work (the latest `#1285` entries are from 2026-09-07). CLAUDE.md's traceability rule asks for the requirement -> design -> implementation -> tests -> run-results chain in the ledger; the commit messages carry a `Verification-objective:` line but no run results. `check-review.sh` classifies the new `crates/openpulse-ardop/src/ptt.rs` as design-class (`scripts/check-review.sh:68`), so a PR needs a `Review:` line pointing at an artifact; this file can be it. | should-fix |
| 8 | **`cap_during_tx.sh`'s `timeout 20` can kill the keyer between `TX1;` and `force_rx`.** `scripts/onair-sdr/cap_during_tx.sh:27` runs `timeout 20 python3 keyplay.py`; Python's default SIGTERM disposition terminates without running `finally`. Summed sleeps in `keyplay.py` worst case: 0.2 + 3 x 0.15 + 0.2 + 4 x 0.8 + 0.2 (gate) + 0.25 + 8.0 (`WATCHDOG_S`, line 25) + 4 x 1.18 (`force_rx`) = 17.2 s, before serial read timeouts (0.5 s per unanswered `ask`). Pre-existing; the change added the `ID;` round trip and one `PC;` read to the budget. A SIGTERM handler that raises, or a longer `timeout`, closes it. | should-fix |
| 9 | **The fixture labelled "FT-818: no reply" is an FT-991A that is mute on `ID;`.** `scripts/test-rig-power.sh:113` `Rig(rid="")` still answers `FA;`, `TX;` and `PC;` in ASCII (`:90-100`); a real FT-818 answers none of them. The keyer's later gates are therefore never exercised against a silent rig in the harness (they do hold: band reads -1 and aborts, confirmed by the simulation in finding 1). The case also does not assert that nothing follows `ID` when the ID mismatches (`:132` checks only `sent[:1]`), which is why finding 1 passes the harness. | nit |
| 10 | **A negative or zero `<max_watts>` writes a malformed `PC` command.** `keyplay.py:57` formats `"PC%03d;" % want` with no range check; `want = -1` sends `PC-01;` up to four times before the read-back fails the `w > want` gate and aborts. Harmless on a Yaesu (`?;`) but outside the CAT grammar; clamp `want` to 5..=20 at `main`. | nit |
| 11 | **The `TXSTART` line can print `999W`.** `keyplay.py:112-113` re-reads `PC;` after the gate for display only; a transient unreadable reply prints 999 W and still keys at the verified power. Cosmetic. | nit |

Checked and found sound (no finding): `lower_power_only` (`keyplay.py:50-61`) writes `PC` only when `want < now` and `now` parsed, returns the read-back, and `gate` (`:65-79`) refuses on `w > 20` or `w > want`, so a lowering that does not take, an unreadable power, and a request above the cap all abort — the harness cases `rig ignores the lowering`, `unreadable power`, `above the 20 W cap` cover them and passed. `watts()` rejects a concatenated or truncated reply (`PC0`, `TX0;PC010;`) as 999. `ID;` is the first byte on the wire (`sent[:1] == ["ID"]` asserted, `:132`). The `OPHF_RIG_ID` override cannot admit an FT-818: with the ID gate cleared, `FA;` reads nothing and the band gate aborts before any `PC` or `TX1;`. ARDOP: `main.rs:191-196` is the only production constructor call; `with_trust_and_relay`/`new` keep `NoOpPtt` but are called only from tests (`tests/ardop_integration.rs:27`, `tests/event_lag.rs:37`); no other crate constructs `ArdopServer`. `ptt_from_spec` matches the daemon arm for arm (`server.rs:2401-2425`): `none`/`""` -> `no_ptt()` keys; `vox` -> `VoxPtt`; `rigctld` down -> `RetryingPtt`, starts, refuses, reconnects; `cm108` absent -> `PttError::Serial`/`Io` -> `RetryingPtt`, so a dongle plugged in late works once it appears; only `Config` (unknown name, `rts`/`dtr` with no device, serial feature absent) refuses startup. Every TNC emission passes `keyed_transmit` (`bridge.rs:543-556`), which skips the emission and pushes `FAULT PTT assert failed` on a refused key, as the manual now says. CLI `--pki-url` 8080 matches `pki-tooling/src/main.rs:9`. `profile.rs` edits match `fec_modes` (`:314-328`: Rs on SL1-SL6, SoftConcatenated SL7-SL11, LdpcHighRate SL12-SL14) and the engine applies `free_rs_strengthening` for every front-end (`crates/openpulse-modem/src/engine.rs:2175-2178`). The `rig_b` template comment matches `server.rs:381-398` (reads `backend`, `rigctld_addr`, `serial_port`; same-address collision refused via `repeater_rig_b_config_error`). `TEST_FREQ_HZ` 144640000 is inside the 144500000-144750000 guard (`run-onair-ic9700-ft991a.sh:189-197`); the old 145650000 default would have tripped it.

## Consumer

Who calls these in production, by command:

- `keyplay.py` / `keyplay_meter.py`: `grep -rn "keyplay" scripts docs --include=*.sh --include=*.md` -> `scripts/onair-sdr/cap_during_tx.sh:27` (`ssh "$STATION" "timeout 20 python3 $KEYPLAY ..."`) and the README; no other caller.
- `ptt_from_spec`: `grep -rn "ptt_from_spec(" --include=*.rs crates` -> `crates/openpulse-ardop/src/main.rs:208` (the `openpulse-tnc` binary) plus its three unit tests in `ptt.rs`.
- `test-rig-power.sh`: `grep -rn "test-rig-power" scripts/gate.sh .github/workflows/` -> `scripts/gate.sh:227` (`run_step "rig power: lower only"`), so it runs in the workspace gate.
- Harness result, actually run: `bash scripts/test-rig-power.sh > log 2>&1; rc=0`, last line `RIG-POWER: PASS`. Sabotage runs of the extracted Python harness (`awk` of the heredoc) against scratchpad copies: keyers with the `want >= now` guard removed -> `rc=1`, `FAIL ... never raises 5 -> 20 ... raised=True`; keyers with the ID check disabled -> `rc=1`, `FAIL ... not an FT-991A ... keyed=True`. The unmodified copies -> `rc=0`.

## Prior art

- The daemon's #1285 handling, which `ptt_from_spec` mirrors: `grep -n "build_ptt_controller\|PttError::Config" crates/openpulse-daemon/src/server.rs` -> `:612-631` (startup refusal) and `:2379-2425` (the adapter).
- The lower-only rule for the shell runners: `scripts/lib/rig-power.sh` (`rig_power_lower_only`) and its tests at `scripts/test-rig-power.sh:1-56`, including the planted-form positive control at `:44-48` that the new Python scan lacks.
- The earlier review's parked items: `docs/dev/reviews/review-docs-revision-2026-10-15.md` disposition rows 4 and 7; row 4's "grep finds nothing" was wrong at the time, `git log -S'TEST_MODE_EXPECT_A' -- scripts/run-onair-ic9700-ft991a.sh` -> `7a100654 2026-10-14` and `git merge-base --is-ancestor 7a100654 d6b7c80a` -> 0.
- The old JSON frame layout the handshake comment describes: `git show v0.16.0:crates/openpulse-core/src/handshake.rs | sed -n 324,352p`.

## Twins

Sibling paths of the same shape, with the command:

- Other front-ends building a PTT controller from `[modem]`: `grep -rn "ptt_builder::build_ptt(\|build_ptt_controller(\|NoOpPtt::new()" --include=*.rs crates | grep -v /tests/ | grep -v openpulse-radio/src` -> `crates/openpulse-kiss/src/main.rs:166` (still fail-open, finding 5), `crates/openpulse-cli/src/radio.rs:10-16` (CLI, its own builder, `none` -> `NoOpPtt`), `crates/openpulse-daemon/src/server.rs:394` (rig_b, via the same adapter), `crates/openpulse-ardop/src/lib.rs:87` and `bridge.rs:81` (test-only constructors). `NoOpPtt::new()` hits in `openpulse-daemon/src/lib.rs` (4668, 4844, 5886) are after the `#[cfg(test)]` at line 4389.
- Other station-side keyers: `grep -rlE 'TX1;|serial\.Serial|rigctl .* T 1' scripts --include=*.py --include=*.sh` -> the two keyers, `scripts/run-onair-ic9700-ft991a.sh` (`verify_ptt_control_a`, finding 4), `scripts/audio-device-sweep-a.sh:97-109` (keys the IC-9700 by `rigctl T 1`; sets no power, so the lower-only rule is not at issue) and `scripts/test-rig-power.sh` itself. No other script speaks Yaesu ASCII CAT.
- Error classification twins of finding 6: `grep -n "not compiled in" crates/openpulse-radio/src/*.rs` -> `gpio.rs:73` (`Serial`), `serial.rs:93,106,109` (`Config`); the two files disagree on the same condition.

## Disposition

All taken in the same branch before the PR.

| # | Disposition |
|---|---|
| 1 | Fixed. On an ID mismatch `gate` reports nothing touched and `main` returns without `force_rx`: the refused rig gets `ID;` and nothing else. The harness asserts `sent == ["ID"]` for a silent rig and for an FT-991 (`ID0570;`); with the guard removed it fails both. |
| 2 | Fixed. The Python scan matches any `PC` string literal except the bare read `PC;` and a compared bare `"PC"`. A positive control plants six spellings (`%`, f-string, single quotes, concatenation, `.format`, bytes) and asserts 6; a negative control asserts the reads match 0. A planted f-string write fails the scan. |
| 3 | Fixed. The `split_frame` doc says v0.16.0's JSON frames carry the same magic and version and fail on the length or body decode. |
| 4 | Fixed. The on-air plan says "before the test transmission" and names the `sidea` PTT probe and the high-SWR tuner as the two keyings that precede `preflight_check`. |
| 5 | Fixed. The TNC builder moved to `openpulse_radio::ptt_builder::build_ptt_or_retrying`; the ARDOP and KISS TNCs both call it and refuse to start on a config error. `crates/openpulse-ardop/src/ptt.rs` is gone; its tests live in `retry_tests`. |
| 6 | Fixed. "GPIO PTT not compiled in" and a CM108 pin outside 1..=8 are `PttError::Config`, so every front-end refuses to start on them. Two tests. |
| 7 | Fixed. Traceability entry, work plan M2 row, and this artifact as the PR's `Review:` line. |
| 8 | Fixed. The keyers install a SIGTERM handler that exits through `SystemExit`, so the `finally` unkey runs under `timeout`. |
| 9 | Fixed with 1: the FT-818 fixture answers nothing. |
| 10 | Fixed. `<max_watts>` must be 5–20 (the FT-991A's lowest setting to the attenuator cap), checked before the port is opened; harness cases for 3 W and 25 W assert nothing is sent. |
| 11 | Fixed. `TXSTART` prints the power the gate verified. |
