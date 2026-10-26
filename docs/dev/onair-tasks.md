---
project: openpulsehf
doc: docs/dev/onair-tasks.md
status: review
last_updated: 2026-10-26
---

# On-air task channel (cloud session ↔ station session on rpi51)

The cloud session cannot reach the session on rpi51, so tasks and results travel on the branch
`claude/affectionate-brahmagupta-sdn8qg`. The cloud session writes the next task here and pushes. The
station session pulls, works the **first task whose status is `open`**, appends its results under that
task, sets the status to `done` (or `blocked`, saying why), commits and pushes. Then the maintainer
tells the cloud session, which pulls, reads the results and writes the next task.

Rules for the station session, every task:
- Work only the open task. Never key a transmitter unless the task says so, and then only after
  asking the maintainer in that session first.
- The station safety rules hold throughout: the FT-818's power is never increased; its FRONT antenna
  stays selected; the mode is checked before keying.
- Paste raw command output into the results (fenced blocks); do not summarise it away. Results that
  contain secrets (keys, passwords) are redacted.
- Commit only this file and session records; push to `claude/affectionate-brahmagupta-sdn8qg`.

---

## Task 1 — station readiness, read-only (status: blocked)

Nothing in this task transmits. Both rigs should be **on** (CAT and the idle level need them).

1. Report `hostname`, `git rev-parse --short HEAD` and the branch. Bring this checkout to the latest
   `origin/claude/affectionate-brahmagupta-sdn8qg` (fast-forward only), and the same on rpi53
   (`ssh dc0sk@dc0sk-rpi53 'cd ~/git/OpenPulseHF && git fetch && git checkout claude/affectionate-brahmagupta-sdn8qg && git merge --ff-only'`).
   If either has local changes, stop and report them.
2. Report which of the *Station preparation* steps 1–3 in `docs/dev/onair-ic9700-ft818-setup.md` are
   already done (SSH keys from rpi51 to itself and rpi53 with `ssh -o BatchMode=yes`; packages; the
   SDRplay API service and `SoapySDRUtil --find="driver=sdrplay"`). Do not install anything as root or
   build third-party code: list what is missing for the maintainer.
3. Release builds on both Pis per step 4 of *Station preparation* (cargo only, no sudo).
4. A local profile outside the repo (`~/onair-local/ic9700-ft818.sh`) that sources
   `docs/config/onair-ic9700-ft818.example.sh`. If one exists from 2026-10-26, show its contents and
   make it source the current example. Set `B_CAT_PORT` to rpi53's `-if00-port0` by-id path (never
   `-if01-`).
5. `source ~/onair-local/ic9700-ft818.sh && scripts/onair-setup-audio-routing.sh`, then
   `scripts/onair-station-check.sh` and, with nothing transmitting anywhere,
   `scripts/onair-station-check.sh --levels`. Paste both outputs in full.
6. Ask the maintainer for the MANUAL list the check prints and record the answers.

### Results

**Status: blocked.** Station A cannot ssh to itself (no key), so station A's audio routing and idle
capture fail; rpi53 has no cargo on the non-interactive PATH and no scipy; the witness SDR is not set up;
the MANUAL list is unanswered (no maintainer present in this session). Nothing transmitted.

**1. Hosts / commits.** rpi51: `DC0SK-RPI51`, `32aeb276`, branch `claude/affectionate-brahmagupta-sdn8qg`
(fast-forwarded; only the untracked `pki-tooling/Cargo.lock`, pre-existing). rpi53: `DC0SK-RPI53`,
fast-forwarded to `32aeb276`, clean.

**2. Station preparation steps 1–3.**
- Step 1: rpi51 -> rpi53 with `ssh -o BatchMode=yes` works. rpi51 -> rpi51 FAILS ("Host key verification
  failed"; `~/.ssh` has only a GitHub key, no `id_ed25519`). MISSING: key, `ssh-copy-id dc0sk@dc0sk-rpi51`.
- Step 2: rpi51 has rigctld, wpctl, pactl, parecord, numpy 2.2.4, scipy 1.15.3, SoapySDR Python bindings.
  rpi53 has the commands, but MISSING `python3-scipy` (`ModuleNotFoundError`), and `cargo` is in
  `~/.cargo/bin` but not on the non-interactive ssh PATH. Both Debian 13 (trixie); Hamlib 4.6.2 on rpi53.
- Step 3: `sdrplay` service active and enabled, `/opt/sdrplay_api` present, but
  `SoapySDRUtil --find="driver=sdrplay"` prints "No devices found!" and no sdrplay module is in the
  SoapySDR modules dir. MISSING: SoapySDRPlay3 build/install (maintainer), RSPdx connected.

**3. Release builds.** All five step-4 builds finished rc=0 on both Pis (rpi51 46 s, rpi53 33 s,
incremental). rpi53 needed `PATH=$HOME/.cargo/bin:$PATH`.

**4. Profile.** `~/onair-local/ic9700-ft818.sh` did not exist; created. It sources the repo example and sets
`B_CAT_PORT=/dev/serial/by-id/usb-Silicon_Labs_CP2105_Dual_USB_to_UART_Bridge_Controller_00B5B685-if00-port0`
(rpi53, ttyUSB0; `-if01-` not used).

**5. Raw output.** `scripts/onair-setup-audio-routing.sh`:
```
==> IC-9700 (dc0sk@dc0sk-rpi51) — set routing (match='Codec', tx-vol=0.15, rx-vol=0.44)
    RESULT fail no-output
==> FT-818 (dc0sk@dc0sk-rpi53) — set routing (match='Codec', tx-vol=0.15, rx-vol=0.45)
    RESULT pass sink=57 src=58 default_sink=57 default_src=58 sink_vol=0.15 src_vol=0.45

FAIL: a station is not routed to its rig CODEC (see RESULT lines above).
      Fix: confirm the CODEC is plugged in and A_CODEC_MATCH/B_CODEC_MATCH match its
      description in 'wpctl status'. Do NOT run the on-air matrix until this PASSES.
```
`scripts/onair-station-check.sh`:
```
== control host: DC0SK-RPI51
[PASS] control checkout clean at 32aeb276

== station A: dc0sk@dc0sk-rpi51
[FAIL] cannot reach dc0sk@dc0sk-rpi51 with -o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=30 (key-based SSH from this host is required)

== station B: dc0sk@dc0sk-rpi53
[PASS] SSH to dc0sk@dc0sk-rpi53 without a password
[INFO] host: Debian GNU/Linux 13 (trixie), rigctl Hamlib 4.6.2 2025-02-09T21:03:50Z SHA=870364 64-bit
[PASS] repo at /home/dc0sk/git/OpenPulseHF
[PASS] checkout clean
[FAIL] missing command: cargo
[PASS] command: rigctl
[PASS] command: rigctld
[PASS] command: arecord
[PASS] command: wpctl
[PASS] command: pactl
[PASS] command: python3
[PASS] command: timeout
[FAIL] python3 numpy/scipy missing (apt install python3-numpy python3-scipy)
[FAIL] onair-preflight --strict: Summary: PASS=10 WARN=0 FAIL=1
[INFO]   preflight: missing required command: cargo
[PASS] openpulse-server built (the daemon path)
[PASS] CAT port present: /dev/serial/by-id/usb-Silicon_Labs_CP2105_Dual_USB_to_UART_Bridge_Controller_00B5B685-if00-port0
[FAIL] CAT read failed (no frequency); is the rig on and the CAT rate right?
[PASS] capture card matching 'Codec'
[PASS] same commit as the control host (32aeb276)

== audio routing (scripts/onair-setup-audio-routing.sh --verify)
==> IC-9700 (dc0sk@dc0sk-rpi51) — verify routing (match='Codec', tx-vol=0.15, rx-vol=0.44)
    RESULT fail no-output
==> FT-818 (dc0sk@dc0sk-rpi53) — verify routing (match='Codec', tx-vol=0.15, rx-vol=0.45)
    RESULT pass sink=57 src=58 default_sink=57 default_src=58 sink_vol=0.15 src_vol=0.45

FAIL: a station is not routed to its rig CODEC (see RESULT lines above).
      Fix: confirm the CODEC is plugged in and A_CODEC_MATCH/B_CODEC_MATCH match its
      description in 'wpctl status'. Do NOT run the on-air matrix until this PASSES.
[FAIL] audio routing or volumes differ from the profile; PipeWire volumes do not survive a reboot — re-run scripts/onair-setup-audio-routing.sh
[INFO] idle RX level not checked (add --levels, with no TX anywhere and both rigs ON)

== MANUAL — not readable over CAT; confirm on the rigs before the session counts
  [ ] FT-818: FRONT antenna selected at 2 m, L1 (0.5 W), DIG/USER-U, CAT RATE matches the profile
  [ ] FT-818: away from the local noise source found on 2026-10-26 (S8–S9 floor)
  [ ] Both rigs: TX time-out timer (TOT) enabled — the A5 backstop
  [ ] IC-9700: the required settings in onair-signal-chain-verification.md (DATA MOD USB, AF output)
  [ ] RSPdx: a fixed attenuator or measured isolation against the IC-9700 beside it; its maximum
      input rating checked in SDRplay's datasheet (UNCHECKED in the repo)
  [ ] Antennas connected (Diamond RH3 on the FT-818); the agreed window and frequency announced

Summary: PASS=15 FAIL=6
```
`--levels`, first run (rigs on, nothing transmitting; before the builds finished):
```
== control host: DC0SK-RPI51
[PASS] control checkout clean at 32aeb276

== station A: dc0sk@dc0sk-rpi51
[FAIL] cannot reach dc0sk@dc0sk-rpi51 with -o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=30 (key-based SSH from this host is required)

== station B: dc0sk@dc0sk-rpi53
[PASS] SSH to dc0sk@dc0sk-rpi53 without a password
[INFO] host: Debian GNU/Linux 13 (trixie), rigctl Hamlib 4.6.2 2025-02-09T21:03:50Z SHA=870364 64-bit
[PASS] repo at /home/dc0sk/git/OpenPulseHF
[PASS] checkout clean
[FAIL] missing command: cargo
[PASS] command: rigctl
[PASS] command: rigctld
[PASS] command: arecord
[PASS] command: wpctl
[PASS] command: pactl
[PASS] command: python3
[PASS] command: timeout
[FAIL] python3 numpy/scipy missing (apt install python3-numpy python3-scipy)
[FAIL] onair-preflight --strict: Summary: PASS=10 WARN=0 FAIL=1
[INFO]   preflight: missing required command: cargo
[PASS] openpulse-server built (the daemon path)
[PASS] CAT port present: /dev/serial/by-id/usb-Silicon_Labs_CP2105_Dual_USB_to_UART_Bridge_Controller_00B5B685-if00-port0
[PASS] CAT reads: 144640000 Hz, mode ?
[FAIL] mode reads '', profile expects 'PKTUSB' (set it on the rig)
[PASS] capture card matching 'Codec'
[FAIL] commit 8fe05837 differs from the control host's 32aeb276; build both ends from one commit

== audio routing (scripts/onair-setup-audio-routing.sh --verify)
==> IC-9700 (dc0sk@dc0sk-rpi51) — verify routing (match='Codec', tx-vol=0.15, rx-vol=0.44)
    RESULT fail no-output
==> FT-818 (dc0sk@dc0sk-rpi53) — verify routing (match='Codec', tx-vol=0.15, rx-vol=0.45)
    RESULT pass sink=57 src=58 default_sink=57 default_src=58 sink_vol=0.15 src_vol=0.45

FAIL: a station is not routed to its rig CODEC (see RESULT lines above).
      Fix: confirm the CODEC is plugged in and A_CODEC_MATCH/B_CODEC_MATCH match its
      description in 'wpctl status'. Do NOT run the on-air matrix until this PASSES.
[FAIL] audio routing or volumes differ from the profile; PipeWire volumes do not survive a reboot — re-run scripts/onair-setup-audio-routing.sh

== idle RX level (scripts/onair-rx-level-check.sh) — nothing may transmit now
==> RX capture-level check (idle; no TX anywhere). Gate must be able to discriminate.
  IC-9700: FAIL — could not capture idle audio from the rig CODEC
  FT-818: idle mean_sq=0.000584  gate threshold=0.001753  OK

FAIL: a station's RX level prevents the energy gate from discriminating.
      Set the per-side RX level with scripts/onair-setup-audio-routing.sh
      (A_RX_SOURCE_VOLUME / B_RX_SOURCE_VOLUME) and re-run this check.
[FAIL] idle RX level check failed

== MANUAL — not readable over CAT; confirm on the rigs before the session counts
  [ ] FT-818: FRONT antenna selected at 2 m, L1 (0.5 W), DIG/USER-U, CAT RATE matches the profile
  [ ] FT-818: away from the local noise source found on 2026-10-26 (S8–S9 floor)
  [ ] Both rigs: TX time-out timer (TOT) enabled — the A5 backstop
  [ ] IC-9700: the required settings in onair-signal-chain-verification.md (DATA MOD USB, AF output)
  [ ] RSPdx: a fixed attenuator or measured isolation against the IC-9700 beside it; its maximum
      input rating checked in SDRplay's datasheet (UNCHECKED in the repo)
  [ ] Antennas connected (Diamond RH3 on the FT-818); the agreed window and frequency announced

Summary: PASS=15 FAIL=8
```
`--levels`, last run (after both builds):
```
== control host: DC0SK-RPI51
[PASS] control checkout clean at 32aeb276

== station A: dc0sk@dc0sk-rpi51
[FAIL] cannot reach dc0sk@dc0sk-rpi51 with -o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=30 (key-based SSH from this host is required)

== station B: dc0sk@dc0sk-rpi53
[PASS] SSH to dc0sk@dc0sk-rpi53 without a password
[INFO] host: Debian GNU/Linux 13 (trixie), rigctl Hamlib 4.6.2 2025-02-09T21:03:50Z SHA=870364 64-bit
[PASS] repo at /home/dc0sk/git/OpenPulseHF
[PASS] checkout clean
[FAIL] missing command: cargo
[PASS] command: rigctl
[PASS] command: rigctld
[PASS] command: arecord
[PASS] command: wpctl
[PASS] command: pactl
[PASS] command: python3
[PASS] command: timeout
[FAIL] python3 numpy/scipy missing (apt install python3-numpy python3-scipy)
[FAIL] onair-preflight --strict: Summary: PASS=10 WARN=0 FAIL=1
[INFO]   preflight: missing required command: cargo
[PASS] openpulse-server built (the daemon path)
[PASS] CAT port present: /dev/serial/by-id/usb-Silicon_Labs_CP2105_Dual_USB_to_UART_Bridge_Controller_00B5B685-if00-port0
[PASS] CAT reads: 144640000 Hz, mode PKTUSB
[PASS] mode matches the profile: PKTUSB (set on the rig, sent by nothing)
[PASS] capture card matching 'Codec'
[FAIL] commit 8fe05837 differs from the control host's 32aeb276; build both ends from one commit

== audio routing (scripts/onair-setup-audio-routing.sh --verify)
==> IC-9700 (dc0sk@dc0sk-rpi51) — verify routing (match='Codec', tx-vol=0.15, rx-vol=0.44)
    RESULT fail no-output
==> FT-818 (dc0sk@dc0sk-rpi53) — verify routing (match='Codec', tx-vol=0.15, rx-vol=0.45)
    RESULT pass sink=57 src=58 default_sink=57 default_src=58 sink_vol=0.15 src_vol=0.45

FAIL: a station is not routed to its rig CODEC (see RESULT lines above).
      Fix: confirm the CODEC is plugged in and A_CODEC_MATCH/B_CODEC_MATCH match its
      description in 'wpctl status'. Do NOT run the on-air matrix until this PASSES.
[FAIL] audio routing or volumes differ from the profile; PipeWire volumes do not survive a reboot — re-run scripts/onair-setup-audio-routing.sh

== idle RX level (scripts/onair-rx-level-check.sh) — nothing may transmit now
==> RX capture-level check (idle; no TX anywhere). Gate must be able to discriminate.
  IC-9700: FAIL — could not capture idle audio from the rig CODEC
  FT-818: idle mean_sq=0.000495  gate threshold=0.001486  OK

FAIL: a station's RX level prevents the energy gate from discriminating.
      Set the per-side RX level with scripts/onair-setup-audio-routing.sh
      (A_RX_SOURCE_VOLUME / B_RX_SOURCE_VOLUME) and re-run this check.
[FAIL] idle RX level check failed

== MANUAL — not readable over CAT; confirm on the rigs before the session counts
  [ ] FT-818: FRONT antenna selected at 2 m, L1 (0.5 W), DIG/USER-U, CAT RATE matches the profile
  [ ] FT-818: away from the local noise source found on 2026-10-26 (S8–S9 floor)
  [ ] Both rigs: TX time-out timer (TOT) enabled — the A5 backstop
  [ ] IC-9700: the required settings in onair-signal-chain-verification.md (DATA MOD USB, AF output)
  [ ] RSPdx: a fixed attenuator or measured isolation against the IC-9700 beside it; its maximum
      input rating checked in SDRplay's datasheet (UNCHECKED in the repo)
  [ ] Antennas connected (Diamond RH3 on the FT-818); the agreed window and frequency announced

Summary: PASS=16 FAIL=7
```
Notes: station A's `no-output` routing and idle-capture FAILs follow from the failed rpi51 -> rpi51 ssh
(not confirmed otherwise). The check says rpi53 is on commit `8fe05837` although `git rev-parse` there gave
`32aeb276` (`8fe05837` is the old `main` HEAD); cause not investigated. FT-818 CAT read 144640000 Hz but
mode empty vs expected PKTUSB. FT-818 idle mean_sq 0.000495 vs gate 0.001486: OK.

**6. MANUAL list.** Not answered; no maintainer was present to ask. Open: the six items printed above.

**For the maintainer:** `ssh-keygen` + `ssh-copy-id` to rpi51 itself; `apt install python3-scipy` on rpi53;
make cargo reachable over non-interactive ssh on rpi53; build SoapySDRPlay3 on rpi51; set the FT-818
mode; then rerun the check.

