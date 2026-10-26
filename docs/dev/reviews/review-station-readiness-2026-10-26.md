---
project: openpulsehf
doc: docs/dev/reviews/review-station-readiness-2026-10-26.md
status: resolved
last_updated: 2026-10-26
---

# Review: on-air station readiness check (2026-10-26)

Adversarial review (Fable) of the working tree on top of `c702573f` (committed locally as `042a3e57`,
with `scripts/onair-station-check.sh` still untracked): the new readiness check run from rpi51, the
`--with-onair` / `--with-sdr` package sets, the measured values in
`docs/config/onair-ic9700-ft818.example.sh`, the *Station preparation* runbook section, and the
2026-10-26 session marked a dry run. It was read-only and ran nothing that could reach a radio: the
check was exercised only against a fake harness (`bin/ssh` runs the "remote" command locally,
`bin/rigctl` prints `FAKE_FREQ`/`FAKE_MODE`, a fake `SoapySDRUtil`, a scratch clone of this repo at
`c702573f` as the "station" checkout). The three station rules (FT-818 power never raised, FRONT
antenna stays selected, mode checked before keying) were the standard.

## Prompt

Falsify, with file and line:
1. Can the check send anything to a rig other than a CAT read, key anything, or change audio or rig
   state, including through the reused `onair-preflight.sh`, `onair-setup-audio-routing.sh --verify`
   and `onair-rx-level-check.sh`? Can the direct CAT open (no rigctld) disturb a rig?
2. Can it report PASS on a station that is not ready?
3. Can it report FAIL on a ready station?
4. Are the installer package names right for Raspberry Pi OS?
5. Do the runbook steps match the code (features, binaries, profile values, SDR build)?
6. Is anything from the session record's gotchas and open items left uncovered?
Also: does any finding block Release 1?

## Verdict

**Not a Release 1 blocker.** The change is station tooling, a profile and docs for M3; no crate, wire
format or PTT path moves. Nothing in the check sends a rig a command other than `f` and `m`, keys
anything, or writes audio state: the audio script is called with `--verify` only
(`scripts/onair-station-check.sh:200`, and `onair-setup-audio-routing.sh:96-101` only writes under
`set`), the level check captures idle audio (`onair-rx-level-check.sh:66-75`), and preflight's only
hardware touch is cpal device enumeration (`onair-preflight.sh:128-129`). The one keying path needs a
wrong profile value (finding 7).

Counts: 1 must-fix, 7 should-fix, 5 nits. The must-fix is a false green: a station whose remote
script aborts before its first check contributes no FAIL and the summary counts only what ran.

## Findings

| # | Finding (evidence) | Class |
|---|---|---|
| 1 | **A station whose remote script aborts is silently uncounted.** `check_side` only tallies `PASS\|`/`FAIL\|` lines it receives (`onair-station-check.sh:165-174`); there is no end-of-script sentinel. With `A_CODEC_MATCH="O'Brien"` the single-quote wrapping at `:172-173` breaks the remote command (`bash: -c: line 1: unexpected EOF while looking for matching`), station A's block shrinks from 21 PASS/FAIL lines to 1 (the SSH PASS) and the summary FAIL count does not move for that station (harness run 7b versus run 1). The same happens for any remote abort after the SSH `true` probe: dropped connection, `bash` missing, `set -u` tripping on a missing positional. Fix: `main` prints `END\|` last; `check_side` calls `bad` when it never arrives; pass arguments with `printf '%q'` (or base64) instead of hand-wrapped single quotes. | must-fix |
| 2 | **The SoapySDR grep can match the no-device message.** `grep -qi "driver *= *sdrplay"` (`:144`) accepts zero spaces, so the text `No devices found! driver=sdrplay` (SoapySDRUtil echoes the filter it was given) matches. Harness run 1 with a fake `SoapySDRUtil` printing exactly that on stdout: `[PASS] SoapySDRPlay3 finds the SDR`. Positive control: `printf 'No devices found! driver=sdrplay\n' \| grep -ci 'driver *= *sdrplay'` prints 1. Whether the real binary writes that line to stdout or stderr was not verifiable offline (stderr is dropped at `:144`); either way, use the exit status (`SoapySDRUtil --find` returns non-zero with no device) or anchor on `^Found device`. | should-fix |
| 3 | **A directory with an empty `.git` passes "checkout clean" and skips the commit compare.** `[[ -d "$repo/.git" ]]` (`:76`) is the only repo test; `git status --porcelain` then fails with empty stdout, which `:79` reads as clean, and `:78` emits `SHA\|` with nothing after it, so `:175-179` runs neither branch. Harness run 5 (`mkdir -p fakerepo/.git`): `[PASS] repo at …/fakerepo`, two `fatal: not a git repository` lines, `[PASS] checkout clean`, no commit line. Fix: `git -C "$repo" rev-parse HEAD` must succeed, else `f`; `check_side` must `bad` when `remote_sha` is empty after a `repo at` PASS. | should-fix |
| 4 | **`eval echo "$1"` runs profile text as code on the station** (`:72`). Harness run 4 with `A_REPO_DIR='$(touch …/pwned)…/clone'` created the file on the "station" and the check still reported `[PASS] repo at …/clone`. The profile is owner-written, so this is a footgun rather than an attack surface, but it also word-splits and globs a path with spaces. The runner has the same construct (`run-onair-ic9700-ft991a.sh:209-210`, `ssh_a "echo ${A_REPO_DIR}"`). Fix: `repo="${1/#'${HOME}'/$HOME}"` (the profile only ever uses that one literal, `onair-ic9700-ft818.example.sh:49,73`). | should-fix |
| 5 | **Station A's mode check is a false red after every session.** For a side that is not `keep`, `expected_mode` returns the mode the runner will *send* (`:182-184`), and a mismatch is a FAIL (`:127-131`). But the runner sets A's mode itself (`run-onair-ic9700-ft991a.sh:554`) and restores the saved mode at teardown (`:594`), and its own preflight only WARNs on non-`keep` sides (`:957-959`). On 2026-10-26 the IC-9700 "was FM; set to PKTUSB" (`onair-session-2026-10-26.md:26`), so after the runner's restore the check fails A until someone sets PKTUSB by hand, for a setting the runner would set anyway. The check already treats frequency this way (`:122-124`, INFO: "the runner tunes inside it before keying"). Fix: FAIL only for `keep` sides; INFO otherwise. | should-fix |
| 6 | **`--levels` proves nothing about station B's modem path.** `onair-rx-level-check.sh:66-69` captures with `parecord` through PipeWire, at `B_RX_SOURCE_VOLUME` (0.45, `onair-ic9700-ft818.example.sh:93`). But B's modem opens `plughw:CARD=CODEC,DEV=0` directly (`:71`; `onair-ic9700-ft818-setup.md:29`), which bypasses the PipeWire source volume; the level check's premise is "A_AUDIO_DEVICE / B_AUDIO_DEVICE = pulse" (`onair-setup-audio-routing.sh:7-8`), true for the FT-991A profile, false for B here. So `[PASS] idle RX level lets the energy gate discriminate on both stations` (`:209`) and the profile's "so the idle floor sits under the energy gate's ceiling" (`:90-91`) are not evidence for B; the number the modem sees on B is the raw ALSA level, which the G0 analyser read at a flat −36 dBFS (`onair-session-2026-10-26.md:38`, roughly 2.5e-4 mean-square, under the 1.07e-3 ceiling, but measured by a different tool for a different purpose). Pre-existing hole, newly asserted as readiness. Fix: for a side whose device is not `pulse`, capture with `arecord -D <device>` (as `onair-rx-idle-floor.sh` does) and say so in the PASS line; or move B to `pulse` like A. | should-fix |
| 7 | **The direct CAT open does not vet the port it opens.** With no rigctld, `:115` runs `rigctl -m "$model" -r "$cat_port" -s "$baud" f m` on whatever `B_CAT_PORT` holds. Opening a tty raises DTR and RTS (Linux `tty_port_block_til_ready`, for blocking and non-blocking opens alike), and the repo itself says RTS on the SCU-17 port keys the DATA-jack PTT (`onair-ic9700-ft818-setup.md:181-182`; `onair-ic9700-ft818.example.sh:62`). The profile asks the operator to copy the port from a list of by-id paths the check prints (`:56-58`; `onair-station-check.sh:103,108`), and the CP2105 has two. Copy the wrong one and the check keys the FT-818 for the length of the open (about 2 s, `onair-session-2026-10-26.md:54`) and then prints `CAT read failed`. The runner has the same exposure through rigctld on the same port, but this check is the tool meant to catch a bad profile before anything opens it. Fix: refuse the direct open unless the path matches the documented CAT interface (`*CP2105*-if00-port0` for B) and otherwise FAIL with the by-id listing; the IC-9700's A port (`:40`) can be matched the same way. | should-fix |
| 8 | **An elimination is written down without a run, and the skipped proof has no parking entry.** The profile (`onair-ic9700-ft818.example.sh:98-101`) and the session record (`onair-session-2026-10-26.md:13`) state the #1060 proof "fails on every machine since the PN-63 preamble (0.468 < 0.51)" and "fails on x86 in release too". The session record's own result is "the first run FAILED (cause not captured); the rerun … was stopped with no verdict" (`:48-49`). No run is recorded in `docs/dev/project/traceability.md` (`grep -nE 'fails on x86\|stale fixture' docs/dev` hits only the session record; the ledger's only `0.468` is line 6216, a 500 Hz figure). `SKIP_RHO_STANDDOWN=1` is now baked into the profile (`:101`) and hard-coded in the check (`onair-station-check.sh:93`), so REQ-RX-03's only runner is off on every path. CLAUDE.md makes eliminations mandatory review before they are written, and the work plan has no line for replacing the fixture (`grep -c 1060 docs/dev/project/workplan.md` = 0; positive control: the profile = 1). Fix: record the x86 release run (command, threshold printed, pass/fail) in the ledger, or soften the wording to "not reproduced"; add a one-line parking entry. | should-fix |
| 9 | **Package names exist, but the OS assumption is unstated.** `apt-cache policy` on this Ubuntu 24.04 host finds `python3-numpy`, `python3-scipy`, `pulseaudio-utils`, `wireplumber`, `python3-soapysdr`, `soapysdr-tools`, `libsoapysdr-dev`, `libusb-1.0-0-dev` and `libhamlib-utils` (SoapySDR 0.8.1, hamlib 4.5.5); these are the Debian names and all are in bookworm, which could not be queried offline. Two observations: the profile says rpi53 read the mode "with Hamlib 4.6.2" (`:84`), which bookworm does not ship (4.5.5), so rpi53 runs trixie or a hand-built hamlib and the runbook should say which; and `wireplumber` is only safe to pull because both Pis already run WirePlumber (`onair-setup-audio-routing.sh:71`), since on a `pipewire-media-session` host it replaces the session manager. | nit |
| 10 | **Runbook build lines match the crates.** `cpal-backend` is `openpulse-cli`'s feature and its default (`crates/openpulse-cli/Cargo.toml:15-16`, so the flag is redundant but harmless); `cpal` is the feature on `openpulse-ardop`, `openpulse-kiss` and `openpulse-daemon` (`Cargo.toml:23`, `:23`, `:31`); bins `openpulse`, `openpulse-tnc`, `openpulse-kisstnc`, `openpulse-gateway` are what preflight checks (`onair-preflight.sh:115-118`). `openpulse-server` is built by step 4 but checked by nothing (not by preflight, and the runner only ships `openpulse` and `openpulse-tnc`, `run-onair-ic9700-ft991a.sh:642-646`); either drop it from step 4 or check it. | nit |
| 11 | **The control-host clean check fails on any untracked file, and step 6 does not say where the local profile lives.** `:57` FAILs on `??` entries; the session put the profile at `~/onair-local/ic9700-ft818.sh` (`onair-session-2026-10-26.md:32`), outside the tree. Step 6 (`onair-ic9700-ft818-setup.md:81-83`) should say "outside the repo", or the first operator to put it under `docs/config/` gets a false red. | nit |
| 12 | **With `WITNESS_SDR_SIDE` unset the SDR checks vanish silently** (`:139`, `:186`); no INFO says the witness was not checked, unlike the `--levels` case (`:214`). | nit |
| 13 | **`pgrep -x rigctld` (`:112`) matches any rigctld on the host**, whatever port or rig it serves; a stale one on another port turns the read into `CAT read failed`. A false red with a misleading hint, not a false green. Checking `-p "$rigctld_port"` in its command line would name the real cause. | nit |

Checked and not found: the FT-818's power is never touched (no `L RFPOWER`, no `l`, no `T` anywhere in the check; `grep -nE "rigctl" scripts/onair-station-check.sh` lists only `:113` and `:115`, both `f m`); the antenna is not touchable over CAT on this rig (`onair-session-2026-10-26.md:55`) and nothing tries; the mode is read and compared before anything could key; the `bash -s` design is sound (the `main` body is one compound command, parsed whole before `main "$@" </dev/null` runs, `:68-70,151`); `read -r -d ''` under `set -u` without `-e` is fine; the `f m` parse (line 1 frequency, line 2 mode, passband on line 3 ignored) fails closed on an empty read and on a missing mode (harness run 6: `CAT read failed`, then `mode reads ''`); an unreachable host is a FAIL (run 8); a dial outside the band is INFO by design (run 9); a value with spaces survives the quoting (run 7, `USB Audio CODEC`); the `-h` help prints the header. The session record's gotchas and open items are covered except the #1060 fixture (finding 8): SSH keys and the agent socket (step 1), scipy (installer and `:87`), SoapySDRPlay3 (step 3 and `:140-148`), volumes after reboot (step 7, `:200`), the CAT port and mode TODOs (profile and `:101-109`, `:125-131`), TOT, the RSPdx rating, the noise source, antennas and rigs on (the MANUAL list and the CAT read, which proves a rig is on). G1-by-SDR, the F8 corpus and the Pi receive cost are session work, not readiness.

## Consumer

The maintainer, on rpi51, after steps 1 to 7 of *Station preparation* and before any session that is
to count: `source ~/onair-local/ic9700-ft818.sh && scripts/onair-station-check.sh`, then
`scripts/onair-station-check.sh --levels` with both rigs on. Callers in the tree:
`grep -rn 'onair-station-check' --include=*.md --include=*.sh .` lists the profile comments
(`onair-ic9700-ft818.example.sh:24,56`), the runbook (`onair-ic9700-ft818-setup.md:45,86-87`) and the
work plan M3 row (`workplan.md:191`); no script calls it, so it stays a manual pre-session step.

## Prior art

Scripts that already reach both stations over `A_SSH`/`B_SSH` (`grep -l 'A_SSH' scripts/*.sh`):
`audio-device-sweep-a.sh`, `onair-dual-capture.sh`, `onair-rx-level-check.sh`,
`onair-setup-audio-routing.sh`, `run-onair-ic9700-ft991a.sh`, `run-onair-twin-ota.sh`. The runner's
`preflight_checks` (`run-onair-ic9700-ft991a.sh:940-959`) is the same mode comparison with the
`keep`/non-`keep` distinction the check lacks (finding 5); `onair-preflight.sh` is the per-station
binary and config check the new script wraps (`onair-station-check.sh:92-99`); `onair-rx-idle-floor.sh`
is the raw-ALSA capture the level check would need on a `plughw` side (finding 6).

## Twins

Sibling constructs of the same shape, found with the commands shown:
- Remote expansion of the repo dir: `grep -nE 'echo \$\{[AB]_REPO_DIR\}|eval echo' scripts/*.sh` gives
  `onair-station-check.sh:72` and `run-onair-ic9700-ft991a.sh:209-210` (finding 4 applies to both).
- Direct serial `rigctl` opens: `grep -nE 'rigctl -m "?\$' scripts/*.sh` gives only
  `onair-station-check.sh:115`; every other script reads through rigctld (`rigctl -m 2`), which the
  runner starts at `run-onair-ic9700-ft991a.sh:500-547` with `pkill -x rigctld` first.
- Per-station PASS/FAIL tallies with no end sentinel: `onair-preflight.sh:47-64` (local, so it
  cannot lose lines) and `onair-setup-audio-routing.sh:128-130` (`RESULT fail no-output` when the
  remote prints nothing, which is the pattern finding 1 asks for).

## Disposition

All taken in the same branch before the PR.

| # | Disposition |
|---|---|
| 1 | Fixed. The remote half ends with an `END|` line; a station without it is a FAIL ("did not run to the end"). Arguments go through `printf %q` and one `bash -s --` string. Fake-host run: a station cut after four lines now FAILs; a profile value with a single quote survives. |
| 2 | Fixed. SoapySDR passes only on a `^Found device` line. |
| 3 | Fixed. The repo check is `git rev-parse HEAD` succeeding; a failing `git status` is a FAIL. |
| 4 | Fixed. No `eval`: the literal `${HOME}` prefix is substituted. The runner's `a_repo`/`b_repo` twin is left as is (it expands the trusted profile on purpose). |
| 5 | Fixed. Only a `keep` side FAILs on its mode; a side whose mode the runner sets gets an INFO with the reading. |
| 6 | Fixed. Station B moves to `B_AUDIO_DEVICE="pulse"` like A, so the routed levels and `--levels` apply to the modem; the check FAILs any side not on `pulse`. With `plughw` the 0.15 TX drive never applied to B either. |
| 7 | Fixed. The direct CAT open is refused for the side's PTT port and any `-if01-` interface; a listening rigctld on the profile's port is used first (`ss -ltn`, finding 13). The runbook says to copy the `-if00-port0` path. |
| 8 | Fixed. The x86 release run is recorded in the traceability ledger (derived 0.46772602, bound 0.51, 407 s, rc 101); the wording no longer says "every machine" or attributes the cause; `SKIP_RHO_STANDDOWN` comes from the profile, not the check; a work plan parking line records that REQ-RX-03's proof runs nowhere until replaced. |
| 9 | Fixed. The check prints each station's OS release and Hamlib version; the runbook says to record them. |
| 10 | Fixed. The check FAILs a missing `openpulse-server`. |
| 11 | Fixed. Untracked files no longer fail the clean check (`--untracked-files=no`); the runbook puts the local profile outside the repo. |
| 12 | Fixed. An unset `WITNESS_SDR_SIDE` prints an INFO. |
| 13 | Fixed with 7: rigctld is detected by a listener on the profile's port, not by name. |
