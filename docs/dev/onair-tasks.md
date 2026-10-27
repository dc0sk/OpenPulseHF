---
project: openpulsehf
doc: docs/dev/onair-tasks.md
status: review
last_updated: 2026-10-27
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


---

## Task 2 — rerun the readiness check after the maintainer's fixes (status: done)

Read-only again; nothing transmits. Both rigs on.

**Before starting, ask the maintainer whether these are done** (they need sudo, a password or a
third-party build, so the session does not do them). If any is not, record which and stop.
- rpi51: `ssh-keygen -t ed25519` (if no key) and `ssh-copy-id dc0sk@dc0sk-rpi51`; then
  `ssh -o BatchMode=yes dc0sk@dc0sk-rpi51 true` succeeds.
- rpi53: `sudo apt install python3-scipy`.
- rpi51: SoapySDRPlay3 built and installed (*Station preparation*, step 3) and the RSPdx plugged in;
  `SoapySDRUtil --find="driver=sdrplay"` lists it.

Then:
1. Fast-forward both Pis to the latest `origin/claude/affectionate-brahmagupta-sdn8qg`. The check now
   reads `~/.cargo/env` itself (cargo was missing on rpi53's non-interactive PATH) and retries a CAT
   read up to three times.
2. Commit mismatch from task 1: in one check run rpi53 reported `8fe05837` while `git rev-parse` there
   gave `32aeb276`. Report, on rpi53, `git -C ~/git/OpenPulseHF rev-parse HEAD`, `git -C
   ~/git/OpenPulseHF branch -vv`, and `ls -d ~/git/*/` (is there a second checkout?), and the value of
   `B_REPO_DIR` after sourcing the local profile.
3. `source ~/onair-local/ic9700-ft818.sh && scripts/onair-setup-audio-routing.sh`, then
   `scripts/onair-station-check.sh --levels` (nothing transmitting). Paste the output in full.
4. Go through the MANUAL list with the maintainer and record each answer.

### Results

**Status: done** (check passes; MANUAL list partly unconfirmed, see item 4). Nothing transmitted.

**Prerequisites** (maintainer confirmed all three done): rpi51 ssh to itself, rpi53 python3-scipy, SoapySDRPlay3 + RSPdx.
Verified: `ssh -o BatchMode=yes dc0sk@dc0sk-rpi51 true` rc=0; rpi53 scipy 1.15.3;
`SoapySDRUtil --find="driver=sdrplay"` -> `Found device 0 / driver = sdrplay / label = SDRplay Dev0 RSPdx 2003052C43 / serial = 2003052C43`.

**1.** Both Pis fast-forwarded to `a20e0fe1`.

**2. Commit mismatch (rpi53).**
```
HEAD a20e0fe16f17900f99f0827bc13dc784c73fa693
* claude/affectionate-brahmagupta-sdn8qg a20e0fe1 [origin/claude/affectionate-brahmagupta-sdn8qg] fix(onair): station check loads ~/.cargo/env and retries flaky CAT reads; task 2 (M3)
  decision-27-local                      3cd019d6 [origin/claude/affectionate-brahmagupta-sdn8qg: behind 6] docs: record the 2026-10-26 on-air session on the 2 m pair (M3)
  main                                   8fe05837 [origin/main] fix: lower-only station keyers, TNC PTT fails closed, review follow-ups (M2/M3) (#1515)
/home/dc0sk/git/OpenPulseHF/
```
Only one checkout under `~/git/`. `B_REPO_DIR=${HOME}/git/OpenPulseHF` after sourcing the profile. The 8fe05837
seen in task 1 was probably the cargo-less ssh path reading a different state; it did not recur (check reports
"same commit as the control host (a20e0fe1)"). Not proven further.

**3. Routing** (`onair-setup-audio-routing.sh`, rc=0) and **check** (`--levels`).
First run, IC-9700 was off (CAT failed after 3 tries, idle mean_sq exactly 0.000000, Summary PASS=42 FAIL=1):
```
==> IC-9700 (dc0sk@dc0sk-rpi51) — set routing (match='Codec', tx-vol=0.15, rx-vol=0.44)
    RESULT pass sink=85 src=83 default_sink=85 default_src=83 sink_vol=0.15 src_vol=0.44
==> FT-818 (dc0sk@dc0sk-rpi53) — set routing (match='Codec', tx-vol=0.15, rx-vol=0.45)
    RESULT pass sink=57 src=58 default_sink=57 default_src=58 sink_vol=0.15 src_vol=0.45

PASS: both stations' default sink+source point at their rig CODEC; TX level set.
      The runner's --device pulse will now reach the radios. Re-run --verify anytime.
== control host: DC0SK-RPI51
[PASS] control checkout clean at a20e0fe1

== station A: dc0sk@dc0sk-rpi51
[PASS] SSH to dc0sk@dc0sk-rpi51 without a password
[INFO] host: Debian GNU/Linux 13 (trixie), rigctl Hamlib 4.6.2 2025-02-09T21:03:50Z SHA=870364 64-bit
[PASS] repo at /home/dc0sk/git/OpenPulseHF
[PASS] checkout clean
[PASS] command: cargo
[PASS] command: rigctl
[PASS] command: rigctld
[PASS] command: arecord
[PASS] command: wpctl
[PASS] command: pactl
[PASS] command: python3
[PASS] command: timeout
[PASS] python3 numpy + scipy (G0 analyser)
[PASS] onair-preflight --strict: Summary: PASS=11 WARN=0 FAIL=0
[PASS] openpulse-server built (the daemon path)
[PASS] CAT port present: /dev/serial/by-id/usb-Silicon_Labs_CP2102N_USB_to_UART_Bridge_Controller_IC-9700_13012889_A-if00-port0
[INFO]   CAT read needed 3 tries
[FAIL] CAT read failed (no frequency); is the rig on and the CAT rate right?
[PASS] capture card matching 'Codec'
[PASS] python3 SoapySDR bindings
[PASS] sdrplay API service active
[PASS] SoapySDRPlay3 finds the SDR
[PASS] same commit as the control host (a20e0fe1)

== station B: dc0sk@dc0sk-rpi53
[PASS] SSH to dc0sk@dc0sk-rpi53 without a password
[INFO] host: Debian GNU/Linux 13 (trixie), rigctl Hamlib 4.6.2 2025-02-09T21:03:50Z SHA=870364 64-bit
[PASS] repo at /home/dc0sk/git/OpenPulseHF
[PASS] checkout clean
[PASS] command: cargo
[PASS] command: rigctl
[PASS] command: rigctld
[PASS] command: arecord
[PASS] command: wpctl
[PASS] command: pactl
[PASS] command: python3
[PASS] command: timeout
[PASS] python3 numpy + scipy (G0 analyser)
[PASS] onair-preflight --strict: Summary: PASS=11 WARN=0 FAIL=0
[PASS] openpulse-server built (the daemon path)
[PASS] CAT port present: /dev/serial/by-id/usb-Silicon_Labs_CP2105_Dual_USB_to_UART_Bridge_Controller_00B5B685-if00-port0
[PASS] CAT reads: 144640000 Hz, mode PKTUSB
[PASS] mode matches the profile: PKTUSB (set on the rig, sent by nothing)
[PASS] capture card matching 'Codec'
[PASS] same commit as the control host (a20e0fe1)

== audio routing (scripts/onair-setup-audio-routing.sh --verify)
==> IC-9700 (dc0sk@dc0sk-rpi51) — verify routing (match='Codec', tx-vol=0.15, rx-vol=0.44)
    RESULT pass sink=85 src=83 default_sink=85 default_src=83 sink_vol=0.15 src_vol=0.44
==> FT-818 (dc0sk@dc0sk-rpi53) — verify routing (match='Codec', tx-vol=0.15, rx-vol=0.45)
    RESULT pass sink=57 src=58 default_sink=57 default_src=58 sink_vol=0.15 src_vol=0.45

PASS: both stations' default sink+source point at their rig CODEC; TX level set.
      The runner's --device pulse will now reach the radios. Re-run --verify anytime.
[PASS] both stations route to their CODEC at the profile's volumes

== idle RX level (scripts/onair-rx-level-check.sh) — nothing may transmit now
==> RX capture-level check (idle; no TX anywhere). Gate must be able to discriminate.
  IC-9700: idle mean_sq=0.000000  gate threshold=0.000100  (on the absolute floor — signal must clear 0.0001)
  FT-818: idle mean_sq=0.000430  gate threshold=0.001291  OK

PASS: both stations' idle floor leaves the energy gate free to discriminate.
[PASS] idle RX level lets the energy gate discriminate on both stations

== MANUAL — not readable over CAT; confirm on the rigs before the session counts
  [ ] FT-818: FRONT antenna selected at 2 m, L1 (0.5 W), DIG/USER-U, CAT RATE matches the profile
  [ ] FT-818: away from the local noise source found on 2026-10-26 (S8–S9 floor)
  [ ] Both rigs: TX time-out timer (TOT) enabled — the A5 backstop
  [ ] IC-9700: the required settings in onair-signal-chain-verification.md (DATA MOD USB, AF output)
  [ ] RSPdx: a fixed attenuator or measured isolation against the IC-9700 beside it; its maximum
      input rating checked in SDRplay's datasheet (UNCHECKED in the repo)
  [ ] Antennas connected (Diamond RH3 on the FT-818); the agreed window and frequency announced

Summary: PASS=42 FAIL=1
```
After the maintainer powered the IC-9700 on, rerun (rc=0, PASS=43 FAIL=0):
```
== control host: DC0SK-RPI51
[PASS] control checkout clean at a20e0fe1

== station A: dc0sk@dc0sk-rpi51
[PASS] SSH to dc0sk@dc0sk-rpi51 without a password
[INFO] host: Debian GNU/Linux 13 (trixie), rigctl Hamlib 4.6.2 2025-02-09T21:03:50Z SHA=870364 64-bit
[PASS] repo at /home/dc0sk/git/OpenPulseHF
[PASS] checkout clean
[PASS] command: cargo
[PASS] command: rigctl
[PASS] command: rigctld
[PASS] command: arecord
[PASS] command: wpctl
[PASS] command: pactl
[PASS] command: python3
[PASS] command: timeout
[PASS] python3 numpy + scipy (G0 analyser)
[PASS] onair-preflight --strict: Summary: PASS=11 WARN=0 FAIL=0
[PASS] openpulse-server built (the daemon path)
[PASS] CAT port present: /dev/serial/by-id/usb-Silicon_Labs_CP2102N_USB_to_UART_Bridge_Controller_IC-9700_13012889_A-if00-port0
[PASS] CAT reads: 144640000 Hz, mode PKTUSB
[INFO]   mode is set by the runner (PKTUSB); reads 'PKTUSB' now
[PASS] capture card matching 'Codec'
[PASS] python3 SoapySDR bindings
[PASS] sdrplay API service active
[PASS] SoapySDRPlay3 finds the SDR
[PASS] same commit as the control host (a20e0fe1)

== station B: dc0sk@dc0sk-rpi53
[PASS] SSH to dc0sk@dc0sk-rpi53 without a password
[INFO] host: Debian GNU/Linux 13 (trixie), rigctl Hamlib 4.6.2 2025-02-09T21:03:50Z SHA=870364 64-bit
[PASS] repo at /home/dc0sk/git/OpenPulseHF
[PASS] checkout clean
[PASS] command: cargo
[PASS] command: rigctl
[PASS] command: rigctld
[PASS] command: arecord
[PASS] command: wpctl
[PASS] command: pactl
[PASS] command: python3
[PASS] command: timeout
[PASS] python3 numpy + scipy (G0 analyser)
[PASS] onair-preflight --strict: Summary: PASS=11 WARN=0 FAIL=0
[PASS] openpulse-server built (the daemon path)
[PASS] CAT port present: /dev/serial/by-id/usb-Silicon_Labs_CP2105_Dual_USB_to_UART_Bridge_Controller_00B5B685-if00-port0
[PASS] CAT reads: 144640000 Hz, mode PKTUSB
[PASS] mode matches the profile: PKTUSB (set on the rig, sent by nothing)
[PASS] capture card matching 'Codec'
[PASS] same commit as the control host (a20e0fe1)

== audio routing (scripts/onair-setup-audio-routing.sh --verify)
==> IC-9700 (dc0sk@dc0sk-rpi51) — verify routing (match='Codec', tx-vol=0.15, rx-vol=0.44)
    RESULT pass sink=87 src=82 default_sink=87 default_src=82 sink_vol=0.15 src_vol=0.44
==> FT-818 (dc0sk@dc0sk-rpi53) — verify routing (match='Codec', tx-vol=0.15, rx-vol=0.45)
    RESULT pass sink=57 src=58 default_sink=57 default_src=58 sink_vol=0.15 src_vol=0.45

PASS: both stations' default sink+source point at their rig CODEC; TX level set.
      The runner's --device pulse will now reach the radios. Re-run --verify anytime.
[PASS] both stations route to their CODEC at the profile's volumes

== idle RX level (scripts/onair-rx-level-check.sh) — nothing may transmit now
==> RX capture-level check (idle; no TX anywhere). Gate must be able to discriminate.
  IC-9700: idle mean_sq=0.000110  gate threshold=0.000331  OK
  FT-818: idle mean_sq=0.000510  gate threshold=0.001530  OK

PASS: both stations' idle floor leaves the energy gate free to discriminate.
[PASS] idle RX level lets the energy gate discriminate on both stations

== MANUAL — not readable over CAT; confirm on the rigs before the session counts
  [ ] FT-818: FRONT antenna selected at 2 m, L1 (0.5 W), DIG/USER-U, CAT RATE matches the profile
  [ ] FT-818: away from the local noise source found on 2026-10-26 (S8–S9 floor)
  [ ] Both rigs: TX time-out timer (TOT) enabled — the A5 backstop
  [ ] IC-9700: the required settings in onair-signal-chain-verification.md (DATA MOD USB, AF output)
  [ ] RSPdx: a fixed attenuator or measured isolation against the IC-9700 beside it; its maximum
      input rating checked in SDRplay's datasheet (UNCHECKED in the repo)
  [ ] Antennas connected (Diamond RH3 on the FT-818); the agreed window and frequency announced

Summary: PASS=43 FAIL=0
```

**4. MANUAL list (maintainer answers).**
- FT-818 front antenna / L1 / DIG-USER-U / CAT rate, and away from the 2026-10-26 noise source: **NOT all confirmed**
  (maintainer did not say which; ask again before any session counts).
- TOT on both rigs and IC-9700 required settings (DATA MOD USB, AF output): confirmed.
- RSPdx attenuation/isolation, max input rating, antennas connected, window announced: confirmed.

**Reviewed (cloud session, 2026-10-27):** accepted as readiness evidence. The commit mismatch in task 1
is closed (one checkout; `8fe05837` is `main`, not seen again). The first run passed the IC-9700's idle
level at exactly 0 with the rig off: `onair-rx-level-check.sh` now fails a floor below 1e-7 as "no
audio from the rig". Open before any keyed task: the FT-818 manual items, and how G0 is judged.

---

## Task 3 — G0 by prominence, then Gate 3 B → A (status: blocked)

**Maintainer decisions (2026-10-27):** the FT-818 items are confirmed (FRONT antenna, L1, DIG/USER-U,
CAT rate 38400) except the noise source: no quiet location yet. G0 is judged on line prominence only.

Fast-forward both Pis to the latest branch first, and `source ~/onair-local/ic9700-ft818.sh` in the
shell you work in.

### Part A — G0, nothing transmits

Both rigs on, nothing transmitting anywhere. `OPHF_ABS_DBFS=0` turns the absolute limit off, so only
prominence decides.
1. Station B: `ssh dc0sk@dc0sk-rpi53 'cd ~/git/OpenPulseHF && OPHF_ABS_DBFS=0 scripts/onair-rx-idle-floor.sh plughw:CARD=CODEC,DEV=0'`
2. Station A with the RSPdx **stopped**: `OPHF_ABS_DBFS=0 scripts/onair-rx-idle-floor.sh plughw:CARD=CODEC,DEV=0`
3. Station A with the RSPdx **streaming** (decision 27: it shares rpi51's USB with the IC-9700). Start
   `python3 scripts/onair-sdr/sdr_capture.py 144640000 250000 20 12 40 /tmp/g0-sdr.cf32 &`, wait 3 s,
   run step 2's command again (its "SDR is running" warning is expected here), then `wait`.
4. If a device is busy (PipeWire holding the CODEC), report it and stop; do not kill audio services.

Paste each output in full. **Stop here (status `blocked`) if any of the three FAILs**, and report the
lines it lists; the maintainer decides. Otherwise continue to Part B.

### Part B — Gate 3, the FT-818 transmits one BPSK250 frame (keys a transmitter)

The FT-818 at L1 transmits once for a few seconds, plus a 0.4 s antenna (SWR) check. The RSPdx on
rpi51 and the IC-9700's USB audio record it.
1. `scripts/onair-station-check.sh` must PASS (it checks the FT-818's mode reads PKTUSB). If not, stop.
2. Start rigctld for the FT-818 on rpi53 and confirm it reads 144.640 MHz, PKTUSB and PTT 0:
   ```bash
   ssh dc0sk@dc0sk-rpi53 "nohup rigctld -m 1041 -r '$B_CAT_PORT' -s 38400 -t 4532 >/tmp/rigctld-b.log 2>&1 &"
   ssh dc0sk@dc0sk-rpi53 'sleep 3; rigctl -m 2 -r 127.0.0.1:4532 f m t'
   ```
3. **Ask the maintainer for a go** in this session, stating: FT-818 at L1 into the Diamond RH3, 144.640
   MHz, one BPSK250 frame plus a 0.4 s SWR check. Key nothing without that go.
4. Run (the SDR records on this host; nothing here touches power or mode):
   ```bash
   SDR_FS=192000 SDR_RFGR=12 scripts/onair-dual-capture.sh gate3-b2a-bpsk250 BPSK250 rs 'DC0SK GATE3'
   ```
5. Afterwards, whatever happened: `ssh dc0sk@dc0sk-rpi53 'rigctl -m 2 -r 127.0.0.1:4532 t'` must read
   `0`; then stop that rigctld (`pkill -x rigctld` on rpi53 is fine — it matches the process name, not
   this shell).
6. Gate 3's criterion — the rig capture's loudest 250 ms window at least 5× its quiet level:
   ```bash
   python3 - docs/dev/test-reports/on-air/captures/gate3-b2a-bpsk250-rig48k.wav <<'EOP'
   import sys, wave, numpy as np
   w = wave.open(sys.argv[1], "rb"); fs, ch = w.getframerate(), w.getnchannels()
   x = np.frombuffer(w.readframes(w.getnframes()), dtype="<i2").astype(float)[0::ch] / 32768
   n = fs // 4; m = np.array([np.mean(x[i:i + n] ** 2) for i in range(0, len(x) - n, n)])
   idle, peak = np.median(m), m.max()
   print(f"windows={len(m)} idle(median)={idle:.6f} peak={peak:.6f} ratio={peak / max(idle, 1e-12):.1f} "
         f"loud_windows={(m > 5 * idle).sum()}")
   EOP
   ```
7. Paste the dual-capture output in full (including the SDR `captured … OK/OVERLOAD` line), the PTT
   read, and the ratio line. Do **not** commit the `.cf32` / `.wav` captures; list their paths and sizes.

### Results

**Status: blocked at Part A step 3.** Steps 1 and 2 PASS; the SDR would not stream, so step 3 was not
achieved; Part B (keys a transmitter) not started. Nothing transmitted.

Both Pis fast-forwarded to `7c7bdaa5`.

**Step 1, station B (rpi53), OPHF_ABS_DBFS=0**, rc=0:
```
==> Capturing 5s of idle RX audio from 'plughw:CARD=CODEC,DEV=0' at 48000 Hz ...
==> Analysing idle floor ...
capture:        /tmp/onair-rx-idle-48000.wav
duration:       5.00 s @ 48000 Hz
overall RMS:     -12.8 dBFS
band policed:   300-2600 Hz
broadband floor: -37.2 dBFS (median in-band)
occupied band:    2707 Hz (41-2748) at -20 dB  <- the FILTER WIDTH; hamlib reports 0 on an IC-9700, so record this one
criteria:       prominence >= 15 dB  OR  level >= 0 dBFS  (max allowed: 0)

PASS: idle floor is clean in the modem passband; the receive path can hear.

G0 PASS for 'plughw:CARD=CODEC,DEV=0'. This rig's receive path can hear a signal in the passband.
```
**Step 2, station A (rpi51), RSPdx stopped**, rc=0:
```
==> Capturing 5s of idle RX audio from 'plughw:CARD=CODEC,DEV=0' at 48000 Hz ...
==> Analysing idle floor ...
capture:        /tmp/onair-rx-idle-48000.wav
duration:       5.00 s @ 48000 Hz
overall RMS:     -18.5 dBFS
band policed:   300-2600 Hz
broadband floor: -43.1 dBFS (median in-band)
occupied band:    2666 Hz (152-2818) at -20 dB  <- the FILTER WIDTH; hamlib reports 0 on an IC-9700, so record this one
criteria:       prominence >= 15 dB  OR  level >= 0 dBFS  (max allowed: 0)

PASS: idle floor is clean in the modem passband; the receive path can hear.

G0 PASS for 'plughw:CARD=CODEC,DEV=0'. This rig's receive path can hear a signal in the passband.
```
**Step 3, station A with the RSPdx streaming: NOT ACHIEVED.** `sdr_capture.py` failed to start the stream:
```
[INFO] devIdx: 0
[INFO] SerNo: 2003052C43
[INFO] hwVer: 4
[INFO] rspDuoMode: 0
[INFO] tuner: 1
[INFO] rspDuoSampleFreq: 0.000000
[INFO] Using format CF32.
[ERROR] error in activateStream() - Init() failed: sdrplay_api_Fail
```
The process then hung (I killed it, PID 2469020, mine). The idle-floor run made meanwhile printed the expected
"SDR is running" warning only because that hung process existed; it is NOT a reading with the SDR streaming
(floor -43.0 dBFS, PASS, same as step 2), so it does not count. A retry after the kill:
```
RuntimeError: SoapySDR::Device::make() no driver specified and no enumeration results
```
`systemctl is-active sdrplay` = active, `lsusb` still lists `1df7:3030 SDRplay RSPdx`. So the sdrplay API
service is wedged after the first failed `activateStream` (the same service did enumerate the device fine
in Task 2). Needs the maintainer: `sudo systemctl restart sdrplay` (and, if it persists, replug the RSPdx),
then rerun step 3. Cause of the first `Init() failed` not established.

**Note for Part A:** station A's overall RMS is -18.5 dBFS against a -43 dBFS in-band floor, so there is
strong content outside 300–2600 Hz on the IC-9700 capture; not examined.



#### Task 3 retry after `sudo systemctl restart sdrplay` (maintainer, 2026-10-27) — still blocked

`SoapySDRUtil --find="driver=sdrplay"` lists the RSPdx again, but `sdr_capture.py 144640000 250000 20 12 40`
fails the same way (no capture file written):
```
[INFO] Using format CF32.
[ERROR] error in activateStream() - Init() failed: sdrplay_api_Fail
```
The G0 run made alongside it (floor -42.9 dBFS, PASS) is again not a streaming reading. `journalctl -u sdrplay`
for the attempt: `SelectDevice: ERROR: InitialiseMappingTableFromFile error -1: invalid filename`, then the
stream thread starts and exits when the script is stopped. `lsusb -t` shows the RSPdx (Dev 025, `1df7:3030`)
on bus 002 behind a hub, `Driver=[none]`, at **12M (full speed)**; the RSPdx needs USB 2.0 high speed
(480M), so the likely cause is the link speed (cable, hub port or the shared hub with the IC-9700 CODEC on
bus 001/004 aside). Not proven: I did not change the port. Suggest replugging the RSPdx directly into a
different rpi51 port with a short high-quality cable, check `lsusb -t` shows 480M, then rerun step 3.

#### Task 3 final run (2026-10-27) — Part A passes, Part B blocked (TX failed before keying)

**Part A step 3 — PASS** after the maintainer replugged the RSPdx (`lsusb -t`: `1df7:3030 … Driver=[none], 480M`).
SDR streaming (`sdr_capture.py 144640000 250000 20 12 40`, rc=0) with the idle-floor check run during it:
```
WARNING: an openpulse / SDR / aplay process is running. This capture must be IDLE
         (no TX anywhere, SDR stopped) or the floor reading is meaningless.
==> Capturing 5s of idle RX audio from 'plughw:CARD=CODEC,DEV=0' at 48000 Hz ...
==> Analysing idle floor ...
capture:        /tmp/onair-rx-idle-48000.wav
duration:       5.00 s @ 48000 Hz
overall RMS:     -18.3 dBFS
band policed:   300-2600 Hz
broadband floor: -42.6 dBFS (median in-band)
occupied band:    2666 Hz (152-2818) at -20 dB  <- the FILTER WIDTH; hamlib reports 0 on an IC-9700, so record this one
criteria:       prominence >= 15 dB  OR  level >= 0 dBFS  (max allowed: 0)

PASS: idle floor is clean in the modem passband; the receive path can hear.

G0 PASS for 'plughw:CARD=CODEC,DEV=0'. This rig's receive path can hear a signal in the passband.
```
SDR: `captured 20.00s peak=0.0029 rms=0.0009 clip=0.00% OK` (`/tmp/g0-sdr.cf32`, 40000000 bytes).
Part A is complete: B PASS, A (RSPdx stopped) PASS, A (RSPdx streaming) PASS.

**Part B.** Station check: first run PASS=41 FAIL=1 (rpi53 behind on commit, docs-only; fast-forwarded to
`86d2fc56`); after the rigctld was started the check's FT-818 CAT read FAILs because my rigctld holds the
serial port (expected; the same check passed CAT 144640000 Hz PKTUSB on the run before). rigctld on rpi53:
```
144640000
PKTUSB
0
0
```
(f, m, t; the extra `0` is the passband). Maintainer gave the **go** (FT-818 at L1, RH3, 144.640 MHz, one
BPSK250 frame + 0.4 s SWR check). Dual capture:
```
==> Preflight
    TX dc0sk@dc0sk-rpi53 on 144640000 Hz, PTT released
    SDR RSPdx, Antenna A, centre 144640000 Hz, fs 192000, RFGR 12 IFGR 40
==> Antenna check: brief keyed SWR reading
    SWR = 1 (limit 3.0)
==> Starting SDR capture (30s)
==> Starting rig USB-audio capture on dc0sk@dc0sk-rpi51 (30s)
==> Transmitting: mode=BPSK250 fec=rs payload='DC0SK GATE3'
    PTT after TX: 0
==> Waiting for captures to finish
==> Results
    TRANSMIT: FAILED (ssh exit 0; no 'Transmitted N bytes' line in /home/dc0sk/git/OpenPulseHF/docs/dev/test-reports/on-air/captures/gate3-b2a-bpsk250-tx.log)
              Nothing was keyed, so BOTH captures below contain only ambient noise.
              Do not use them as evidence. Common cause: TX_REPO_DIR points at the
              wrong station's layout (it defaults to Station B's).
    SDR IQ : /home/dc0sk/git/OpenPulseHF/docs/dev/test-reports/on-air/captures/gate3-b2a-bpsk250.cf32 (46080000 bytes)
             cfg: f=144.6400MHz RFGR=12 IFGR=40 ant=Antenna A RFGR range 0, 27
             captured 30.00s peak=0.0076 rms=0.0015 clip=0.00% OK
    rig aud: /home/dc0sk/git/OpenPulseHF/docs/dev/test-reports/on-air/captures/gate3-b2a-bpsk250-rig48k.wav (28.0s @ 48000Hz)  mean_sq=0.000106  peak=0.044
No transmission took place; the captures hold ambient noise only and are NOT evidence.
```
Ratio line: `windows=111 idle(median)=0.000106 peak=0.000130 ratio=1.2 loud_windows=0`

**Result: the SWR check keyed (SWR = 1, limit 3.0), but the BPSK250 frame was NOT transmitted.** The TX log:
`Error: transmit failed / Caused by: audio error: device not found: pulse` (preceded by a WARN that the
bandplan guardrail's rigctld frequency read failed with `Resource temporarily unavailable`, and ALSA noise).
The captures hold ambient noise only (ratio 1.2) and are NOT evidence; Gate 3's >=5x criterion is not met.
PTT after: 0; rigctld on rpi53 stopped and confirmed gone.
Likely cause, unproven: on rpi53 the cpal build cannot open a device named `pulse` (the profile sets
`B_AUDIO_DEVICE=pulse`, whereas the earlier station A runs used it). Next for the maintainer/cloud session:
check `openpulse --backend cpal devices` on rpi53 for the name the modem sees (e.g. `default`, or
`pipewire`), and whether the pipewire-alsa/pulse ALSA plugin is installed there. I did not retry or change
the device.
Captures (gitignored, not committed), in `docs/dev/test-reports/on-air/captures/`:
`gate3-b2a-bpsk250.cf32` 46080000 B, `gate3-b2a-bpsk250-rig48k.wav` 5373996 B, `gate3-b2a-bpsk250-tx.log` 2793 B.

#### Task 3 Gate 3 rerun (2026-10-27) after `libasound2-plugins` on rpi53 — transmitted; criterion NOT met (4.9x < 5x)

rpi53 `devices` now lists `pipewire`, `pulse`, `default`; routing set, both RESULT pass (TX 0.15, RX 0.44 / 0.45).
Maintainer gave a fresh **go** (FT-818 at L1, RH3, 144.640 MHz, one BPSK250 frame + 0.4 s SWR check).
rigctld before keying (f, m, t, passband): `144640000 / PKTUSB / 0 / 0`.
```
==> Preflight
    TX dc0sk@dc0sk-rpi53 on 144640000 Hz, PTT released
    SDR RSPdx, Antenna A, centre 144640000 Hz, fs 192000, RFGR 12 IFGR 40
==> Antenna check: brief keyed SWR reading
    SWR = 1 (limit 3.0)
==> Starting SDR capture (30s)
==> Starting rig USB-audio capture on dc0sk@dc0sk-rpi51 (30s)
==> Transmitting: mode=BPSK250 fec=rs payload='DC0SK GATE3'
    Transmitted 11 bytes in BPSK250 mode (fec=Rs).
    PTT after TX: 0
==> Waiting for captures to finish
==> Results
    SDR IQ : /home/dc0sk/git/OpenPulseHF/docs/dev/test-reports/on-air/captures/gate3-b2a-bpsk250.cf32 (46080000 bytes)
             cfg: f=144.6400MHz RFGR=12 IFGR=40 ant=Antenna A RFGR range 0, 27
             captured 30.00s peak=0.2654 rms=0.0450 clip=0.00% OK
    rig aud: /home/dc0sk/git/OpenPulseHF/docs/dev/test-reports/on-air/captures/gate3-b2a-bpsk250-rig48k.wav (28.0s @ 48000Hz)  mean_sq=0.000189  peak=0.043
Both captures succeeded. To turn them into corpus entries:
  1. Prepare the rig audio for the corpus (48k stereo -> 8k mono, anti-aliased).
     This capture is ALREADY on disk, so decimate it in place — do NOT call
     onair-record-capture.sh, which always records a NEW live capture (passing it 0
     seconds records nothing at all):
       python3 - '/home/dc0sk/git/OpenPulseHF/docs/dev/test-reports/on-air/captures/gate3-b2a-bpsk250-rig48k.wav' crates/openpulse-modem/tests/captures/<rig>-gate3-b2a-bpsk250.wav <<'EOP'
       import sys, wave, struct, numpy as np
       from scipy.signal import resample_poly
       src, dst = sys.argv[1], sys.argv[2]
       w = wave.open(src, "rb"); n, ch, fs = w.getnframes(), w.getnchannels(), w.getframerate()
       x = np.frombuffer(w.readframes(n), dtype="<i2").astype(np.float64)[0::ch] / 32768.0; w.close()
       y = resample_poly(x, 1, fs // 8000)
       o = wave.open(dst, "wb"); o.setnchannels(1); o.setsampwidth(2); o.setframerate(8000)
       o.writeframes(struct.pack("<%dh" % len(y), *np.clip(y*32768, -32768, 32767).astype(int))); o.close()
       EOP
  2. Add a row to crates/openpulse-modem/tests/captures/README.md recording PROVENANCE
     (rig, frequency, mode, FEC, payload, level settings, date) and the property the file
     is meant to preserve. A capture with no recorded expectation cannot be asserted against.
  3. The payload transmitted was: 'DC0SK GATE3'
     A rig capture WITH a known payload is the corpus's missing piece: it is the only thing
     that can assert an end-to-end DECODE against real audio.
```
PTT after: `0`; rigctld on rpi53 stopped (`rigctld stopped`).

Gate 3 criterion (loudest 250 ms window >= 5x the quiet level, rig capture):
```
windows=111 idle(median)=0.000104 peak=0.000511 ratio=4.9 loud_windows=0
```
**Result:** the frame WAS transmitted (`Transmitted 11 bytes in BPSK250 mode (fec=Rs)`), and the RSPdx saw it
(peak 0.2654, rms 0.0450, against peak 0.0076, rms 0.0015 in the no-TX run: ~35x in rms-amplitude terms).
The IC-9700's audio capture did not meet the criterion: ratio **4.9** (< 5.0), `loud_windows=0`
(peak 0.000511 vs median 0.000104). Whole-capture mean_sq rose 0.000106 -> 0.000189 and peak stayed 0.043,
so the IC-9700 audio barely carries the signal. Gate 3 as written: **not met** (marginally). Not examined:
the IC-9700 receive level / AF output / squelch (the manual list's IC-9700 settings), signal level at the
IC-9700 (SDR shows a strong signal, so the antenna path is fine), and the 250 ms window vs a ~1 s BPSK250
frame. Decode of the capture not attempted (outside the task).
Captures (gitignored, not committed), `docs/dev/test-reports/on-air/captures/`: `gate3-b2a-bpsk250.cf32`
46080000 B, `gate3-b2a-bpsk250-rig48k.wav` 5373996 B, `gate3-b2a-bpsk250-tx.log` 4168 B,
`gate3-b2a-bpsk250.cf32.log` 990 B.

#### IC-9700 receive-settings check after the Gate 3 rerun (read-only; maintainer asked, 2026-10-27)

Direct `rigctl -m 3081 -r $A_CAT_PORT -s 115200` reads (no rigctld running, nothing set, `t` = 0):
```
f 144640000 | m PKTUSB 0 | l AF 0.00784314 | l SQL 0.203922 | l RF 1 | l STRENGTH -54 | l NB 0
l NR 0.0313726 | l PREAMP 0 | l ATT 0 | l AGC 2 | l RFPOWER 0.0470588 | l MICGAIN 0.74902 | l RAWSTR 0
```
`NOTCHF`, `IF` are not hamlib levels on this backend. Per-250 ms mean_sq of the rig capture (L channel; R is
silent), x1e-3: windows 0-39 and 72-111 are 0.08-0.12 (noise); **windows 40-71 (8 s) are 0.20-0.51, mean about
0.40**, i.e. the burst is there, consistently ~4x the noise, which is why `peak/median` is 4.9 and
`> 5*median` counts none. Spectrum of the capture: 1000-2000 Hz holds 80 dB against 64 dB at 300-1000 Hz and
63 dB at 2-3 kHz; above 3 kHz 15-18 dB. So the signal is in the passband where BPSK250 at 1500 Hz belongs.
Findings:
- Signal level at the CODEC is low: burst mean_sq ~0.0004; `onair-signal-chain-verification.md` (Gate 4) wants
  a peak mean_sq >= 0.005 for the energy gate, ~10x more. The chain is aligned, the level is not.
- AF reads 0.0078 (2/255): the AF knob is at the bottom. On the IC-9700 the USB audio level is the menu
  item USB AF Level (MENU > SET > Connectors), whether the AF knob also scales the USB output is not verified
  here. Both are front-panel checks (UNCHECKED: the menu values are not readable over CAT).
- SQL reads 0.204; the doc wants minimum (off). The idle floor is nonzero, so squelch is not gating the audio
  fully, but fix it anyway.
- NR level 0.031 (noise reduction possibly on) and AGC 2: the doc does not specify them; left as found.
- Mode PKTUSB, RF 1, preamp/attenuator 0: as expected. IC-9700 RF power 0.047 (~5 W, tx only, irrelevant here).
FT-818 power: not touched and not needed. The RSPdx measured a strong signal; the shortfall is the IC-9700
audio output level, which more TX power would not fix. The station rules in this file keep the FT-818's power
from ever being increased; I did not do or recommend that.
Suggested next steps for the maintainer (front panel): SQL fully CCW; raise USB AF Level (and AF if it scales
USB) in steps, re-record an idle capture and a transmit, aim for burst mean_sq ~0.005 without clipping.

#### Gate 3 third run (2026-10-27) after SQL=0 and RF gain back at 1 — transmitted; criterion still NOT met (4.9x)

Maintainer: USB AF Level was already at 100% (also during the first runs); SQL set to 0; RF gain restored to 1
(CAT: SQL 0, RF 1, mode PKTUSB, AGC 2). Fresh **go** given. rigctld before keying: `144640000 / PKTUSB / 0 / 0`.
```
==> Preflight
    TX dc0sk@dc0sk-rpi53 on 144640000 Hz, PTT released
    SDR RSPdx, Antenna A, centre 144640000 Hz, fs 192000, RFGR 12 IFGR 40
==> Antenna check: brief keyed SWR reading
    SWR = 1 (limit 3.0)
==> Starting SDR capture (30s)
==> Starting rig USB-audio capture on dc0sk@dc0sk-rpi51 (30s)
==> Transmitting: mode=BPSK250 fec=rs payload='DC0SK GATE3'
    Transmitted 11 bytes in BPSK250 mode (fec=Rs).
    PTT after TX: 0
==> Waiting for captures to finish

==> Results
    SDR IQ : /home/dc0sk/git/OpenPulseHF/docs/dev/test-reports/on-air/captures/gate3-b2a-bpsk250.cf32 (46080000 bytes)
             cfg: f=144.6400MHz RFGR=12 IFGR=40 ant=Antenna A RFGR range 0, 27
             captured 30.00s peak=0.2054 rms=0.0320 clip=0.00% OK
    rig aud: /home/dc0sk/git/OpenPulseHF/docs/dev/test-reports/on-air/captures/gate3-b2a-bpsk250-rig48k.wav (28.0s @ 48000Hz)  mean_sq=0.000202  peak=0.043
```
Ratio: `windows=111 idle(median)=0.000104 peak=0.000504 ratio=4.9 loud_windows=0`. PTT after: 0; rigctld stopped.
Analysis: the result is the same as run 2 (peak 0.043, ratio 4.9). Burst = 32 windows, mean_sq 0.000457 vs
noise 0.000104: burst-over-noise SNR about 5.3 dB in the CODEC audio, while the RSPdx saw a strong signal
(peak 0.2054, rms 0.0320). So SQL and RF gain were not what limited it; USB AF was already at 100%. Levers
that remain: (1) the PipeWire capture volume `A_RX_SOURCE_VOLUME=0.44` (software; raising it scales signal and
noise together, helps the energy gate's absolute level but not the ratio); the CODEC has no ALSA capture
control (`amixer -c CODEC`: PCM playback only); (2) the 5.3 dB itself: IC-9700 AGC/filter/noise (NR 0.031,
AGC 2, DATA filter width) or the signal reaching the IC-9700 being weak relative to its audio noise; an
IC-9700 S-meter (`l RAWSTR`/`l STRENGTH`) read DURING a transmit would show the RF level; (3) the criterion
itself: 5x peak-window over the whole-capture median measures a ~8 s burst in a 28 s capture and a flat-AGC
receiver; whether it is the right test for this chain is for the cloud session. Not changed: FT-818 power.
Captures overwritten in place (gitignored, not committed): same three paths as run 2.

#### IC-9700 settings and RF level, read-only (maintainer asked, 2026-10-27)

CAT reads (nothing set): mode PKTUSB (width unreadable, hamlib 0; the idle capture's -20 dB band was 152-2818 Hz,
so 1500 Hz is inside it), AGC 2 (hamlib enum: FAST), AGC_TIME 0.3, NR func off (level 0.031 unused), NB off,
ANF off, MN off, COMP off, preamp 0, att 0, RF 1, SQL 0, AF 0.0039, PBT_IN/OUT 0.502 (centred), RIT off, dual
watch off, sat mode off, OVF_STATUS 0 (no overload), STRENGTH -51, RAWSTR 1.
RF level at the RSPdx, from `gate3-b2a-bpsk250.cf32` (run 3), 0.5 s windows, 500-2500 Hz offset vs a 6-8 kHz
reference band: noise windows differ by 0.1-1 dB; the burst (about t=9..18 s) sits **41 dB above the quiet
in-band level, 47-52 dB above the reference band** (reference band also rises with the burst, so the true SNR
is at least ~40 dB). So the RF signal is very strong; the IC-9700 audio shows only ~5.3 dB burst-over-noise.
Reading: with a ~40-50 dB RF signal the receiver's FAST AGC holds the audio near a fixed output level, so the
CODEC audio burst-over-idle ratio is set by the AGC target against the idle noise (AGC at full gain, S3-level
band noise), not by the signal strength; more TX power, SQL or RF gain cannot move it. Nothing misconfigured
was found. Options (maintainer/cloud decision, not applied): AGC to SLOW or OFF on the IC-9700 (changes the
level behaviour; with AGC off a 40-50 dB signal may clip the CODEC, use RF gain/att to bring it down), or
treat the 5x criterion as not applicable to an AGC-levelled chain and judge Gate 3 by decode of the capture.

#### IC-9700 AGC set to SLOW (maintainer request, 2026-10-27)

`rigctl -m 3081 ... L AGC 3` (hamlib SLOW): read back `l AGC` 2 -> 3, `l AGC_TIME` 0.3 -> 6, mode still PKTUSB,
PTT 0. Maintainer context: the RSPdx antennas and the FT-818 are indoors at the same level (so the RSPdx's ~40-50
dB reflects a short indoor path); the IC-9700 antenna is outdoors a few metres higher, so the RF level at the
IC-9700 is NOT known from the SDR. An IC-9700 S-meter read during a transmit (`l STRENGTH`, `l RAWSTR`) is the
missing measurement. A second FT-818 antenna was offered by the maintainer; not used. Not rerun yet.

#### Gate 3 fourth run (2026-10-27) — IC-9700 AGC SLOW, S-meter sampled during TX — **criterion MET (7.9x)**

Fresh **go** given. Routing RESULT pass on both; rigctld before keying `144640000 / PKTUSB / 0 / 0`.
```
==> Preflight
    TX dc0sk@dc0sk-rpi53 on 144640000 Hz, PTT released
    SDR RSPdx, Antenna A, centre 144640000 Hz, fs 192000, RFGR 12 IFGR 40
==> Antenna check: brief keyed SWR reading
    SWR = 1 (limit 3.0)
==> Starting SDR capture (30s)
==> Starting rig USB-audio capture on dc0sk@dc0sk-rpi51 (30s)
==> Transmitting: mode=BPSK250 fec=rs payload='DC0SK GATE3'
    Transmitted 11 bytes in BPSK250 mode (fec=Rs).
    PTT after TX: 0
==> Waiting for captures to finish
==> Results
    SDR IQ : /home/dc0sk/git/OpenPulseHF/docs/dev/test-reports/on-air/captures/gate3-b2a-bpsk250.cf32 (46080000 bytes)
             cfg: f=144.6400MHz RFGR=12 IFGR=40 ant=Antenna A RFGR range 0, 27
             captured 30.00s peak=0.2098 rms=0.0320 clip=0.00% OK
    rig aud: /home/dc0sk/git/OpenPulseHF/docs/dev/test-reports/on-air/captures/gate3-b2a-bpsk250-rig48k.wav (28.0s @ 48000Hz)  mean_sq=0.000147  peak=0.043
```
Ratio: `windows=111 idle(median)=0.000063 peak=0.000500 ratio=7.9 loud_windows=30 peak_sample=0.043`. PTT after: 0; rigctld stopped.
Gate 3 (loudest 250 ms window >= 5x quiet level): **7.9x, 30 loud windows: met.** Change from run 3 (4.9x, 0 loud
windows): the quiet median fell 0.000104 -> 0.000063 with AGC SLOW, the burst peak stayed 0.000500 (peak sample
0.043 both times). So AGC was the lever; SQL/RF/USB AF were not.
IC-9700 `l STRENGTH` (dB re S9) sampled about every 0.5 s by a loop of direct rigctl reads while the capture ran (55 reads,
counts of each value, sorted): idle -50..-44 (about 20 reads), rising through -42..+1 during the key-up, then
**+22, +30, +39, +49, +50, +53, +54 (x3), +55 (x9), +56 (x3)** during the burst. So the signal at the IC-9700 is
about S9+55 dB (very strong; the outdoor antenna is not weak at all). Raw samples (epoch, dB):
```
1793093947.976 -47
1793093948.493 -49
1793093949.011 -45
1793093949.525 -47
1793093950.042 -50
1793093950.566 -36
1793093951.079 -40
1793093951.592 -45
1793093952.104 -49
1793093952.617 -49
1793093953.130 -2
1793093953.641 1
1793093954.153 1
1793093954.665 -5
1793093955.179 -11
1793093955.693 -17
1793093956.215 -22
1793093956.740 -26
1793093957.259 -29
1793093957.774 -33
1793093958.288 -37
1793093958.803 -42
1793093959.320 -45
1793093959.837 -47
1793093960.354 -39
1793093960.874 -44
1793093961.390 -48
1793093961.903 -46
1793093962.418 -47
1793093962.936 -44
1793093963.451 -47
1793093963.963 -44
1793093964.476 -44
1793093964.992 -48
1793093965.505 1
1793093966.020 53
1793093966.541 49
1793093967.059 54
1793093967.573 54
1793093968.090 55
1793093968.608 56
1793093969.126 56
1793093969.646 56
1793093970.162 55
1793093970.677 55
1793093971.192 55
1793093971.709 55
1793093972.224 55
1793093972.742 55
1793093973.254 55
1793093973.767 55
1793093974.287 50
1793093974.806 39
1793093975.321 30
1793093975.838 22
```
Because the signal is S9+55 dB, a second antenna on the FT-818 is not needed for this gate, and the receiver is
being driven hard (OVF_STATUS not sampled during the burst: UNCHECKED). Decode of the capture not attempted.
Captures (gitignored, not committed), same paths: `gate3-b2a-bpsk250.cf32` 46080000 B,
`gate3-b2a-bpsk250-rig48k.wav` 5373996 B, `gate3-b2a-bpsk250-tx.log`.
Task 3 status left as `blocked` in the heading by the earlier run: Part A and Part B now both pass; the cloud
session sets it to done.

**Reviewed (cloud session, 2026-10-27):** done. G0 PASS on all three runs (prominence only): FT-818
floor −37.2 dBFS, IC-9700 −43.1 dBFS with the RSPdx stopped and −42.6 with it streaming. Gate 3 B → A
**met at 7.9x** (30 loud windows) on run 4, with the IC-9700's AGC on SLOW; the FT-818 at L1 arrives at
S9+55 dB. Fixed along the way: the RSPdx must enumerate at 480M (it failed at 12M); rpi53 needed
`libasound2-plugins` for the ALSA `pulse` device. Taken into the tooling: the installer adds
`libasound2-plugins`; the station check fails a station without the `pulse` device and reads the
IC-9700's AGC against `A_EXPECT_AGC=3`. The ~8 s burst is expected: 11 bytes plus RS fill one 255-byte
block, ~8.2 s at 250 bit/s.

---

## Task 4 — Gates 4 and 5 without keying, then Gate 6 both ways (status: blocked)

Fast-forward both Pis and `source ~/onair-local/ic9700-ft818.sh`. The IC-9700's AGC stays on SLOW.

### Part A — nothing transmits

1. `scripts/onair-station-check.sh --levels` must PASS (it now also checks the IC-9700's AGC reads 3
   and that `pulse` exists as an ALSA device on both). Paste the summary and any FAIL lines.
2. **Gate 5 from the Gate 3 capture** (no new transmission): the carrier of the received BPSK250 burst.
   Squaring BPSK removes the modulation and leaves a line at twice the carrier:
   ```bash
   python3 - docs/dev/test-reports/on-air/captures/gate3-b2a-bpsk250-rig48k.wav <<'EOP'
   import sys, wave, numpy as np
   w = wave.open(sys.argv[1], "rb"); fs, ch = w.getframerate(), w.getnchannels()
   x = np.frombuffer(w.readframes(w.getnframes()), dtype="<i2").astype(float)[0::ch] / 32768
   n = fs // 4; m = np.array([np.mean(x[i:i + n] ** 2) for i in range(0, len(x) - n, n)])
   loud = np.where(m > 3 * np.median(m))[0]
   seg = x[loud[0] * n:(loud[-1] + 1) * n]
   sq = seg ** 2; sq -= sq.mean()
   spec = np.abs(np.fft.rfft(sq * np.hanning(len(sq)))); f = np.fft.rfftfreq(len(sq), 1 / fs)
   band = (f > 2000) & (f < 4000); k = np.argmax(spec[band]); f2 = f[band][k]
   print(f"burst {len(seg) / fs:.1f}s  2fc line {f2:.1f} Hz  carrier {f2 / 2:.1f} Hz  offset {f2 / 2 - 1500:+.1f} Hz")
   EOP
   ```
   Pass: carrier 1500 Hz ± 50 Hz.
3. **Gate 4 on station A**, the modem's own receive path at the device the runner uses:
   ```bash
   target/release/openpulse --backend cpal --log debug --ptt none receive --mode BPSK250        --listen-ms 10000 --device pulse > /tmp/gate4.log 2>&1; echo rc=$?
   grep -iE 'chunk|samples|callback|stream|rate' /tmp/gate4.log | head -40
   ```
   Paste that, and if the grep finds nothing, the first 40 lines of `/tmp/gate4.log`. Pass (from the
   signal-chain doc): chunks ≤ 2048 samples, ≥ 4 per second, about the nominal sample rate. Do not tune
   anything if it looks wrong; report.

### Part B — Gate 6, one 64-byte BPSK250 frame each way (keys both transmitters)

Gate 6 as written: `"result": "pass"` in the run's JSON report. The runner starts and stops rigctld,
reads the band guard and the mode, and only ever lowers power (the IC-9700 to `A_RFPOWER=0.05`; the
FT-818 is never sent a power level). The RSPdx stays connected through its attenuator.
1. **B → A (the FT-818 transmits).** Ask the maintainer for a go (FT-818 at L1, 144.640 MHz, one
   BPSK250 frame, 64 bytes, no FEC), then:
   `scripts/run-onair-ic9700-ft991a.sh supervise --single-case 'BPSK250|none|64' --reverse`
2. **A → B (the IC-9700 transmits).** Ask for a separate go (IC-9700 lowered to ~5 W if it reads
   higher; one frame), then the same command without `--reverse`.
3. After each: PTT reads 0 on both rigs, and no rigctld left running. Paste the runner's summary, the
   result line of each JSON report (`ls -t docs/dev/test-reports/on-air/ | head -3`), and on a fail the
   IRS log tail it prints. Do not retry a failed direction without asking.

### Results

_(station session: append here)_


**Part A (nothing transmitted). Both Pis fast-forwarded to `407be1c6`. IC-9700 `l AGC` = 3 (SLOW).**

**A1.** `onair-station-check.sh --levels`, rc=0, **Summary: PASS=46 FAIL=0**. Non-PASS lines:
```
== control host: DC0SK-RPI51
== station A: dc0sk@dc0sk-rpi51
[INFO] host: Debian GNU/Linux 13 (trixie), rigctl Hamlib 4.6.2 2025-02-09T21:03:50Z SHA=870364 64-bit
[INFO]   mode is set by the runner (PKTUSB); reads 'PKTUSB' now
== station B: dc0sk@dc0sk-rpi53
[INFO] host: Debian GNU/Linux 13 (trixie), rigctl Hamlib 4.6.2 2025-02-09T21:03:50Z SHA=870364 64-bit
== audio routing (scripts/onair-setup-audio-routing.sh --verify)
==> IC-9700 (dc0sk@dc0sk-rpi51) — verify routing (match='Codec', tx-vol=0.15, rx-vol=0.44)
    RESULT pass sink=87 src=82 default_sink=87 default_src=82 sink_vol=0.15 src_vol=0.44
==> FT-818 (dc0sk@dc0sk-rpi53) — verify routing (match='Codec', tx-vol=0.15, rx-vol=0.45)
    RESULT pass sink=57 src=58 default_sink=57 default_src=58 sink_vol=0.15 src_vol=0.45
PASS: both stations' default sink+source point at their rig CODEC; TX level set.
      The runner's --device pulse will now reach the radios. Re-run --verify anytime.
== idle RX level (scripts/onair-rx-level-check.sh) — nothing may transmit now
==> RX capture-level check (idle; no TX anywhere). Gate must be able to discriminate.
  IC-9700: idle mean_sq=0.000077  gate threshold=0.000230  OK
  FT-818: idle mean_sq=0.000404  gate threshold=0.001213  OK
PASS: both stations' idle floor leaves the energy gate free to discriminate.
== MANUAL — not readable over CAT; confirm on the rigs before the session counts
  [ ] FT-818: FRONT antenna selected at 2 m, L1 (0.5 W), DIG/USER-U, CAT RATE matches the profile
  [ ] FT-818: away from the local noise source found on 2026-10-26 (S8–S9 floor)
  [ ] Both rigs: TX time-out timer (TOT) enabled — the A5 backstop
  [ ] IC-9700: the required settings in onair-signal-chain-verification.md (DATA MOD USB, AF output)
  [ ] RSPdx: a fixed attenuator or measured isolation against the IC-9700 beside it; its maximum
      input rating checked in SDRplay's datasheet (UNCHECKED in the repo)
  [ ] Antennas connected (Diamond RH3 on the FT-818); the agreed window and frequency announced
Summary: PASS=46 FAIL=0
```
**A2 Gate 5 (from the Gate 3 capture, run 4), PASS:**
```
burst 7.5s  2fc line 3007.2 Hz  carrier 1503.6 Hz  offset +3.6 Hz
```
**A3 Gate 4 (station A, modem receive path, `--device pulse`), PASS on the doc's criteria.** `rc=1` is the expected
"no decodable frame" (nothing was transmitting):
```
Error: receive failed / Caused by: demodulation failed: no decodable frame within 10000 ms
```
`opened cpal input stream on 'pulse'`, then `received N accumulated audio samples` lines (first 40 shown; the log's
chunk lines are the only ones the grep matched besides ALSA noise):
```
ALSA lib pcm_dmix.c:973:(snd_pcm_dmix_open) The dmix plugin supports only playback stream
ALSA lib pcm_dmix.c:973:(snd_pcm_dmix_open) The dmix plugin supports only playback stream
ALSA lib pcm_dmix.c:973:(snd_pcm_dmix_open) The dmix plugin supports only playback stream
ALSA lib pcm_dmix.c:973:(snd_pcm_dmix_open) The dmix plugin supports only playback stream
ALSA lib pcm_dsnoop.c:541:(snd_pcm_dsnoop_open) The dsnoop plugin supports only capture stream
ALSA lib pcm_dmix.c:973:(snd_pcm_dmix_open) The dmix plugin supports only playback stream
ALSA lib pcm_dmix.c:973:(snd_pcm_dmix_open) The dmix plugin supports only playback stream
ALSA lib pcm_dsnoop.c:541:(snd_pcm_dsnoop_open) The dsnoop plugin supports only capture stream
2026-10-27T09:51:23.945423Z DEBUG opened cpal input stream on 'pulse'
2026-10-27T09:51:23.965972Z DEBUG received 200 accumulated audio samples
2026-10-27T09:51:23.996241Z DEBUG received 400 accumulated audio samples
2026-10-27T09:51:24.016359Z DEBUG received 600 accumulated audio samples
2026-10-27T09:51:24.036529Z DEBUG received 800 accumulated audio samples
2026-10-27T09:51:24.076834Z DEBUG received 1000 accumulated audio samples
2026-10-27T09:51:24.096990Z DEBUG received 1200 accumulated audio samples
2026-10-27T09:51:24.117124Z DEBUG received 1400 accumulated audio samples
2026-10-27T09:51:24.147278Z DEBUG received 1600 accumulated audio samples
2026-10-27T09:51:24.167412Z DEBUG received 1800 accumulated audio samples
2026-10-27T09:51:24.187554Z DEBUG received 2000 accumulated audio samples
2026-10-27T09:51:24.227803Z DEBUG received 2200 accumulated audio samples
2026-10-27T09:51:24.247942Z DEBUG received 2400 accumulated audio samples
2026-10-27T09:51:24.268079Z DEBUG received 2600 accumulated audio samples
2026-10-27T09:51:24.298313Z DEBUG received 2800 accumulated audio samples
2026-10-27T09:51:24.318435Z DEBUG received 3000 accumulated audio samples
2026-10-27T09:51:24.338585Z DEBUG received 3200 accumulated audio samples
2026-10-27T09:51:24.378858Z DEBUG received 3400 accumulated audio samples
2026-10-27T09:51:24.399025Z DEBUG received 3600 accumulated audio samples
2026-10-27T09:51:24.419195Z DEBUG received 3800 accumulated audio samples
2026-10-27T09:51:24.449412Z DEBUG received 4000 accumulated audio samples
2026-10-27T09:51:24.469562Z DEBUG received 4200 accumulated audio samples
2026-10-27T09:51:24.489732Z DEBUG received 4400 accumulated audio samples
2026-10-27T09:51:24.530058Z DEBUG received 4600 accumulated audio samples
2026-10-27T09:51:24.550233Z DEBUG received 4800 accumulated audio samples
2026-10-27T09:51:24.570405Z DEBUG received 5000 accumulated audio samples
2026-10-27T09:51:24.590599Z DEBUG received 5200 accumulated audio samples
2026-10-27T09:51:24.620886Z DEBUG received 5400 accumulated audio samples
2026-10-27T09:51:24.641548Z DEBUG received 5600 accumulated audio samples
2026-10-27T09:51:24.681958Z DEBUG received 5800 accumulated audio samples
2026-10-27T09:51:24.702364Z DEBUG received 6000 accumulated audio samples
2026-10-27T09:51:24.722569Z DEBUG received 6200 accumulated audio samples
```
Derived from all 400 chunk lines: chunk 200 samples (<= 2048), 40 chunks/s (>= 4), 8000 Hz over 9.98 s.

**Part B, 1. B -> A (maintainer go given): FAILED BEFORE ANY TRANSMISSION, status blocked.**
`scripts/run-onair-ic9700-ft991a.sh supervise --single-case 'BPSK250|none|64' --reverse` exited **rc=255** after the
preflight reported "Pre-flight OK" and printed the case header; no JSON report was written
(`docs/dev/test-reports/onair-2026-10-27T095245.json` does not exist), no summary, no IRS log excerpt. The last output:
```
  [BPSK250                fec=none               payload=  64B] ... ==> Cleanup
  [IC-9700] power: left as is (reading '0.0470588', requested 0.0470588; this script only lowers power)
  [FT-818] power: left as is (reading '10', requested 10; this script only lowers power)
  [IC-9700 rigctld] stopping on A
  [FT-818 rigctld] stopping on B
  done
```
Preflight WARNs (from the same run): FT-818 passband=0 Hz ("digital modes need >= 2400 Hz"), IC-9700 PulseAudio
output volume 15% "may be too low", FT-818 NB/NR unreadable, IC-9700 and FT-818 RF GAIN unreadable over CAT (UNVERIFIED),
`rig_set_level` "Feature not available" (hamlib trace). The FT-818 rfpower reads 10 (its CAT scale); not sent anything.
Evidence nothing was keyed: no `/tmp/openpulse-iss-BPSK250-none.log` or telemetry log on rpi53; PTT 0 on A after;
no rigctld on B, none on A. The IRS receiver the runner started on A (`/tmp/openpulse-irs-BPSK250-none.log`,
`--listen-ms 16000`) was still running ~minutes later; I killed that one process (PID 3445232, the runner's).
Cause of the rc=255 not established: the runner died between starting the IRS on A and the ISS ssh to B (script is
`set -euo pipefail`; an unguarded command in the --reverse block, e.g. around the telemetry or the ISS ssh, is
the suspect, unproven). Per the task, not retried. **A -> B (step 2) not run.**
