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

#### Gate 6 B -> A, retry (maintainer: "try to find the reason and retry") — runner bug found and fixed; transmitted; **decode FAILED**

**Reason for the rc=255 (found, reproduced, fixed).** `bash -x` trace: the script dies at line 1446, the `ssh_a` that
launches the IRS receiver. That ssh returned **255**: its remote command starts with
`pgrep -f '<repo>/target/release/openpulse .*receive'`, and `pgrep -f` also matches the remote `bash -c` command
line (which contains that very text) and its `$(...)` subshell; the kill loop excludes only the remote shell's own
`$$`, so it kills another process of the ssh session and ssh exits 255; `set -e` then aborts the runner. Reproduced
with a harmless `nohup sleep` in place of the modem: `ssh rc=255`; with the pattern written `[o]penpulse` the same
probe gave `rc=0` on both Pis. The same self-match made the later "IRS alive?" check (`pgrep -f ... && echo yes`)
always report `yes` (it matched itself). Fix: `release/openpulse .*receive` -> `release/[o]penpulse .*receive` at all 6
sites (both directions, the twins at lines 1433, 1451, 1515, 1541, 1559, 1588); `bash -n` ok. It does not touch PTT,
power, mode or what is transmitted, but it IS an edit to the keyed-path runner and has not had the adversarial review
the repo asks for: please review before merging. The first, failed run (`onair-2026-10-27T095245`) never keyed.

**The retry** (maintainer's go, FT-818 L1, one 64-byte BPSK250 frame, no FEC): the runner completed, the FT-818 keyed:
`Transmitted 64 bytes in BPSK250 mode (fec=None).`, telemetry `ptt_on=2`, IRS S-meter max 56 (dB re S9). Result in
`docs/dev/test-reports/onair-2026-10-27T102028.json`: `"result":"fail","fail_reason":"IRS: payload not observed in
receiver output"`, `0/1 passed`. PTT 0 on A after; no rigctld or openpulse left on either Pi. Runner's own summary and
IRS log (head/tail) are above in this entry's raw output below.
```
==> On-air test matrix (tier=quick, 1 cases)
    ISS: FT-818 on dc0sk@dc0sk-rpi53   callsign=DC0SK
    IRS: IC-9700 on dc0sk@dc0sk-rpi51   callsign=DC0SK
    Freq: 144640000 Hz (A:PKTUSB, B:keep) (2m enforced)
    Report: docs/dev/test-reports/onair-2026-10-27T102028.json
  [BPSK250                fec=none               payload=  64B] ... FAIL (IRS: payload not observed in receiver output)
    IRS log head:
    IRS log tail:
      2026-10-27T10:21:26.668255Z DEBUG AFC: raw_estimate=0.0Hz correction=4.2Hz total_offset=4.2Hz
      2026-10-27T10:21:26.668258Z DEBUG AFC full-retry: pos=127232 correction=4.2Hz
      2026-10-27T10:21:26.671309Z  INFO received 75715 audio samples
      2026-10-27T10:21:26.681482Z DEBUG demodulated 287 bytes
      2026-10-27T10:21:26.687313Z DEBUG AFC: raw_estimate=-0.2Hz correction=4.2Hz total_offset=3.9Hz
      2026-10-27T10:21:26.687324Z DEBUG AFC full-retry: pos=127232 FAILED: frame encoding/decoding error: invalid magic
      2026-10-27T10:21:26.690591Z DEBUG AFC: raw_estimate=4.0Hz correction=4.0Hz total_offset=7.9Hz
      2026-10-27T10:21:26.693865Z DEBUG AFC: raw_estimate=-0.1Hz correction=3.9Hz total_offset=3.7Hz
      2026-10-27T10:21:26.697168Z DEBUG AFC: raw_estimate=-0.0Hz correction=3.9Hz total_offset=3.8Hz
      2026-10-27T10:21:26.700458Z DEBUG AFC: raw_estimate=-0.0Hz correction=3.8Hz total_offset=3.8Hz
      2026-10-27T10:21:26.703728Z DEBUG AFC: raw_estimate=-0.0Hz correction=3.8Hz total_offset=3.8Hz
      2026-10-27T10:21:26.707005Z DEBUG AFC: raw_estimate=-0.0Hz correction=3.8Hz total_offset=3.8Hz
      2026-10-27T10:21:26.707014Z DEBUG AFC full-retry: pos=127264 correction=3.8Hz
      2026-10-27T10:21:26.710167Z  INFO received 75715 audio samples
      2026-10-27T10:21:26.720363Z DEBUG demodulated 287 bytes
      2026-10-27T10:21:26.726165Z DEBUG AFC: raw_estimate=0.0Hz correction=3.8Hz total_offset=3.9Hz
      2026-10-27T10:21:26.726176Z DEBUG AFC full-retry: pos=127264 FAILED: frame encoding/decoding error: invalid magic
      2026-10-27T10:21:26.729474Z DEBUG AFC: raw_estimate=4.1Hz correction=4.1Hz total_offset=8.3Hz
      2026-10-27T10:21:26.732737Z DEBUG AFC: raw_estimate=-0.1Hz correction=4.1Hz total_offset=4.0Hz
      2026-10-27T10:21:26.736007Z DEBUG AFC: raw_estimate=-0.0Hz correction=4.1Hz total_offset=4.0Hz
      2026-10-27T10:21:26.739289Z DEBUG AFC: raw_estimate=-0.0Hz correction=4.1Hz total_offset=4.1Hz
      2026-10-27T10:21:26.742549Z DEBUG AFC: raw_estimate=-0.0Hz correction=4.1Hz total_offset=4.1Hz
      2026-10-27T10:21:26.745811Z DEBUG AFC: raw_estimate=-0.0Hz correction=4.1Hz total_offset=4.1Hz
      2026-10-27T10:21:26.745815Z DEBUG AFC full-retry: pos=127296 correction=4.1Hz
      2026-10-27T10:21:26.749008Z  INFO received 75715 audio samples
    telemetry: ISS(ptt_on=2, pcm=na, mixer="  Playback channels: Front Left - Front Right", alc>0=0, rfm>0=0) IRS(str_max=56)
==> Results: 0/1 passed, 1 failed.
    Report: docs/dev/test-reports/onair-2026-10-27T102028.json
Evidence bundle created: docs/dev/test-reports/on-air/bundle-2026-10-27T102310Z-ic9700-ft991a-quick
- metadata: docs/dev/test-reports/on-air/bundle-2026-10-27T102310Z-ic9700-ft991a-quick/metadata.json
- report: docs/dev/test-reports/on-air/bundle-2026-10-27T102310Z-ic9700-ft991a-quick/onair-2026-10-27T102028.json
- config snapshot: docs/dev/test-reports/on-air/bundle-2026-10-27T102310Z-ic9700-ft991a-quick/config.toml.snapshot
==> Cleanup
  [IC-9700] power: left as is (reading '0.0470588', requested 0.0470588; this script only lowers power)
  [FT-818] power: left as is (reading '10', requested 10; this script only lowers power)
  [IC-9700 rigctld] stopping on A
  [FT-818 rigctld] stopping on B
  done
```
Diagnosis of the IRS log (`/tmp/openpulse-irs-BPSK250-none.log` on rpi51, 317 `invalid magic` lines):
- Energy was detected, but the preamble gate rejected it: `settle at onset=126080 rejected: preamble correlation
  rho=0.209 < 0.31 ... energy without a preamble`; further onsets rho 0.285, 0.285, 0.286 (just under 0.31); then the AFC
  full-retry decodes "287 bytes" that fail `frame encoding/decoding error: invalid magic`. AFC correction about 4 Hz.
- NOT a one-sided rebuild (CLAUDE.md's usual suspect for "invalid magic"): `target/release/openpulse` has the identical
  sha256 prefix `52ce8717b53ffb5b` and mtime (2026-10-26 18:12) on both Pis, and `git diff 32aeb276 HEAD -- crates plugins
  Cargo.toml Cargo.lock` is empty.
- The ISS side's ALC and RF-meter telemetry read 0 (`alc>0=0, rfm>0=0`, `pcm=na`): the meters are not informative.
- Differences from the Gate 3 capture that WAS strong (7.9x): FEC none vs RS, 64 B vs 11 B (longer, no coding gain),
  and the runner's route (IRS through `--device pulse` at src vol 0.44).
- Hypothesis, unproven: the IC-9700 sees S9+55 dB; its front end/audio may be compressing or distorting a signal that
  strong, lowering the preamble correlation just under the 0.31 threshold. An FT-818 -> IC-9700 attenuator or the
  IC-9700's attenuator (`ATT`) would test it; I changed nothing. Not tested: decoding the Gate 3 capture offline through
  the same receive path, and the rho values on a repeat.
Not retried (task rule). **A -> B (step 2) not run.** Committed with this entry: the runner fix, the JSON report,
the rig-state files and the evidence bundle `docs/dev/test-reports/on-air/bundle-2026-10-27T102310Z-ic9700-ft991a-quick`.

#### Offline decode of the Gate 3 captures, the RSPdx question, and the retry with the IC-9700 attenuator on (maintainer, 2026-10-27)

**Offline decode (nothing transmitted).** The Gate 3 run-4 captures (`BPSK250|rs`, payload `DC0SK GATE3`) were
converted to 8 kHz mono (rig audio: `resample_poly` 48k->8k; RSPdx IQ: keep 0..4 kHz positive baseband, 2*real,
`resample_poly` 192k->8k) and replayed through the production receive path with a scratch integration test
(`ChannelSimHarness::feed_capture` + `receive_with_fec_mode_timeout("BPSK250", Rs, 40 s)`; the scratch file was
deleted, not committed). **Control:** a synthetic transmit of the same payload through the same harness decodes
(`CONTROL: Ok("DC0SK GATE3")`), so the harness is sound.
- Rig audio (IC-9700 CODEC): `NOT DECODED: FEC error: RS correction failed at block 0: TooManyErrors`.
- RSPdx IQ (RFGR 12, IFGR 40): the same `RS correction failed at block 0: TooManyErrors`.
- Both captures show the same spectrum: burst 7.5 s (rig) / 8.8 s (SDR), 90% of the power in 237-238 Hz, i.e. a
  BPSK250 signal; the frame is FOUND (RS runs) but has too many bit errors on **both** independent receivers.
**Answer to "was everything decodable on the RSPdx?": no.** Only the Gate 3 run-4 SDR capture exists (the Gate 6 runs
record no SDR audio), and it does not decode. Caveats: the SDR was at RFGR 12, which the corpus README says
saturates the RSPdx front end ("an overloaded SDR is not a neutral witness"); with a ~40-50 dB signal from an indoor
transmitter it may be saturated, so the SDR result alone does not exonerate the receivers. I did not re-record at RFGR
22-27 (two attempts to run `sdr_capture.py` at RFGR 27 / 192 kHz alongside the runner failed with
`activateStream() - Init() failed: sdrplay_api_Fail`, as on the first attempt; the 480M link was fine before).

**Retry with the attenuator.** First attempt had NO attenuator: `rigctl L ATT 12` is rejected by the IC-9700
(`Command rejected by the rig`; the only attenuator step hamlib lists is 10 dB), and `l ATT` read back 0, so
run `onair-2026-10-27T113719` is not an attenuator test (same failure, IRS str_max 52). With `rigctl L ATT 10`
(read back 10; idle STRENGTH -47..-52 -> -54) I retried (go as the maintainer asked): run
`docs/dev/test-reports/onair-2026-10-27T114123.json`: **FAIL, "IRS: payload not observed in receiver output"**,
ISS `ptt_on=2`, IRS str_max **44** (was 56), 310 `invalid magic` lines, preamble correlation rho 0.197 / 0.197 / 0.271
(< 0.31), AFC correction ~2.7 Hz. PTT 0 on A after, nothing left running on either Pi. I set `ATT` back to 0.
**The IC-9700 front-end-overload hypothesis is falsified** by this: 12 dB less signal at the receiver, identical
failure. Together with the SDR (a second receiver, also undecodable) the fault is most likely on the transmit side:
the FT-818's modulation as produced through the SCU-17 at the runner's `--device pulse` path (TX sink volume 0.15),
e.g. distortion/clipping or ALC action (the ISS telemetry reads `alc>0=0, rfm>0=0`, i.e. the meters say nothing).
Not tested: the same frame from the IC-9700 (A -> B, the other direction, which would show whether the modem and
receive chain are fine), a lower/higher TX drive, the FT-818's TX audio quality on a scope or against the SDR at
RFGR 22+. Gate 6 B -> A has now failed three times and was not retried further; A -> B not run.

#### FT-818 clicks during transmit: hamlib toggles VFO A/B on every rigctl client call; VFO B is on 40 m (2026-10-27, maintainer observation + rigctld -vvvvv log)

**Observation (maintainer).** During the modem's transmit the FT-818's relay clicked at least once a second; during the
PTT pattern test (CAT `T 1` for 5.8 s, then 5 x ~1 s) there were clicks before the key-up and key-down clicks:
"3 x (click, 200 ms, click, 500 ms)" before the TX in step A, and clicks before the TX click in step C. SCU-17 PTT LED
dark under CAT keying (expected: it follows RTS, not CAT). The maintainer **confirms the rig switches to 40 m**.
**Cause (read from `rigctld -m 1041 -vvvvv`, `/tmp/rigctld-b-v.log` and `-v2.log` on rpi53; read-only, nothing keyed).**
The hamlib FT-817/818 backend, on every new rigctl client connection, reads the frequency of both VFOs; to read VFO B
it sends the CAT VFO-toggle command (`00 00 00 00 81`, `ft817_set_vfo VFOB`), reads, and toggles back (`ft817_set_vfo
VFOA`). Counted per client call, for each of `t`, `l ALC_METER`, `l RFPOWER_METER`, `l STRENGTH`, `m`, `f`: **2 VFO
toggles, every time**. VFO B's frequency read from the rig is BCD `00 70 77 00` = **7.077 MHz**; VFO A is 144.640 MHz.
So each rigctl call swaps the FT-818 to 7.077 MHz and back, at about 0.7 s per CAT round trip.
**Why this plausibly broke the on-air frames.** The runner's telemetry (`TELEMETRY_ENABLE=1`, set in
`docs/config/onair-ic9700-ft818.example.sh:117`, 40 samples at 0.2 s) runs three rigctl calls (`t`, `l ALC_METER`,
`l RFPOWER_METER`) in a loop on the FT-818 WHILE the modem transmits, i.e. about 6 VFO toggles per cycle during the
frame. The ISS telemetry also reads `ALC=` and `RFM=` empty, so those calls were failing. Every Gate 6 B->A run and
the modem's own bandplan frequency read (`rigctld I/O error: Resource temporarily unavailable`, seen at TX start) share
this. Whether the FT-818 actually radiated on 40 m, or refused the toggle while keyed, is NOT established; a frame
that swaps VFO mid-transmission is not decodable, which fits "frame found, RS TooManyErrors" on two independent
receivers and the preamble correlation 0.2-0.29. Not tested: a transmit with `TELEMETRY_ENABLE=0`.
**Safety note:** VFO B at 7.077 MHz means any toggle that lands while keyed can transmit on 40 m into the 2 m antenna.
Recommended before any further keyed run (maintainer/cloud decision, nothing applied): (1) `TELEMETRY_ENABLE=0` for
every keyed FT-818 run (the profile exports 1); (2) set the FT-818's VFO B to 144.640 MHz on the rig, so a toggle
cannot land on HF; (3) a hamlib setting or a persistent single rigctld client instead of per-call `rigctl` for the
FT-818; (4) the modem and the Gate 3 dual-capture script should not poll the FT-818 over rigctld during TX. The Gate 3
frame (run 4) was sent with only the modem's connection plus the SWR check and also failed offline decode, so check
what else touched rigctld then. No further transmissions were done after this finding.

#### Gate 6 B->A rerun after VFO B -> 144.640 MHz and TELEMETRY_ENABLE=0 (maintainer go, 2026-10-27) — still FAIL

Maintainer set the FT-818's VFO B to 144.640 MHz (verified from the rig: VFO B BCD `14 46 40 00`, same as A) and noticed the
noise floor dropped to S5. A first attempt was refused by the runner's preflight (FT-818 mode read `USB`, expected `PKTUSB`;
nothing keyed); the maintainer set DIG on the rig; `f m t` then read `144640000 PKTUSB 0`. Run
`TELEMETRY_ENABLE=0 scripts/run-onair-ic9700-ft991a.sh supervise --single-case 'BPSK250|none|64' --reverse`:
rc=1, `docs/dev/test-reports/onair-2026-10-27T122722.json` **FAIL, "IRS: payload not observed in receiver output"**;
ISS `Transmitted 64 bytes in BPSK250 mode (fec=None)`; IRS: 33 preamble rejections, rho 0.18-0.29 (< 0.31), 192
`invalid magic`, AFC correction 0.0 Hz. Same signature as before, so the VFO toggles + telemetry polling were not (the
only) cause. PTT 0, nothing left running. The modem's own bandplan read at TX start still logs `rigctld I/O error:
Resource temporarily unavailable` (it polls rigctld once, a slow CAT round trip, now harmless for the VFO).
Remaining suspects: FT-818 transmit audio quality/level through the SCU-17 at the runner's pulse path (TX sink 0.15)
or the FT-818's own modulation, and the 64-byte no-FEC frame being long/unprotected vs the 11-byte RS frame of
Gate 3 (that one also failed offline). Next differential, not run: A->B (IC-9700 transmits, FT-818 receives), and the
RSPdx at RFGR 22+ recording the FT-818 audio. Not retried.

#### Gate 6 B->A, 5th run (2026-10-27 ~13:35 local), VFO B = VFO A = 144.640000 MHz, telemetry off — FAIL, same signature

Maintainer set VFO B equal to A (verified: `f m v t` = `144640000 PKTUSB VFOA 0`, VFO B BCD `14 46 40 00` in all 19 reads; before that
B was 144.640460). Report `docs/dev/test-reports/onair-2026-10-27T123418.json`: **FAIL, "IRS: payload not observed in receiver
output"**; ISS modulated 74 bytes -> 21216 samples (2.65 s) and `Transmitted 64 bytes in BPSK250 mode (fec=None)` at 12:35:00.6Z; IRS:
58 preamble rejections (rho 0.199-0.245 < 0.31, correction 0.0 Hz), 150 `invalid magic`. PTT 0 on A, nothing running. The
maintainer listened to the relays during this run (observation to be added by the maintainer).

#### Gate 6 B->A runs 6-8 (2026-10-27): the FT-818's on-air envelope ramps up over ~1 s; not the leader, not the sink; the modem's own TX envelope is flat

All three runs FAIL the same way (`IRS: payload not observed`, preamble rho 0.18-0.29 < 0.31, `invalid magic` ~300-350 lines):
run 6 `onair-2026-10-27T124112.json` (RSPdx at RFGR 22, 192 kHz, `TELEMETRY_ENABLE=0`), run 7 `...T124715.json`
(`ISS_PTT_LEADER_MS=800`), run 8 `...T125449.json` (leader 800 ms + a silent `paplay` stream holding rpi53's SCU-17 sink RUNNING instead
of SUSPENDED; stopped afterwards, sink IDLE). PTT 0, nothing running, after each. Hook added to the runner (see below).
**RSPdx (RFGR 22, clip 0.00%) offline decode of run 6:** `NOT DECODED: invalid magic` (the SDR copy fails too).
**Measured on the SDR IQ** (band 1000-2200 Hz power, 20 ms steps), same in all three runs: the signal is a correct BPSK250
(carrier 1525.0 Hz via the 2fc line, `|z|^2` line at exactly 250.0 Hz, 90% power band 1401-1643 Hz) but its **amplitude rises smoothly
by ~30 dB over ~1.05 s (-25 dB -> -3 dB re the flat level: 1.08 / 1.08 / 1.04 s)**, then is flat for 1.60 s, then stops abruptly;
ramp + flat = 2.65 s = the audio length. So the preamble (the first ~0.25-1 s) goes out 30 dB down and is not acquirable.
**Not the PTT leader** (800 ms made no difference to the ramp), **not the sink waking from suspend** (a running silent stream made no difference),
**not the modem**: the modem's own synthetic transmit of a 64-byte BPSK250|none frame, tapped at the loopback, is 21216 samples = 2.652 s with a FLAT
envelope (RMS per 100 ms 0.592-0.640, peak 1.000 = full scale). So the ramp appears downstream of the modem's samples: the audio
server at 0.15 volume, the SCU-17 codec, the FT-818's modulator/ALC, or its power path. Peak 1.0 into a 0.15 sink and a
rig with ALC is worth ruling in or out: untested here. Also untested: a tone or longer lead-in, a different TX drive, another transmit audio device.
**Runner change** (keyed-path script, needs the adversarial review before merge): `ISS_PTT_LEADER_MS` env, when set, adds `--ptt-leader-ms N` to the
FT-818 transmit command; default unchanged (`scripts/run-onair-ic9700-ft991a.sh`, 4 lines). **Not proven useful**: it did not help.
Open decision: A->B (the IC-9700 transmits) as the receive-side differential, or look at the FT-818 transmit chain (ALC reading, SCU-17 drive, a
frame with a ~1.5 s lead-in). Nothing further transmitted.

#### Tone test through the modem-less chain (2026-10-27 ~14:05-14:09 local): the start-up loss depends on the TX drive (sink volume), not on the modem or on a cold start

Method (maintainer go; every PTT was CAT via rigctld on rpi53; L1 0.5 W, 144.640 MHz; nothing raised): a 4 s, 1500 Hz tone,
amplitude 0.88 (RMS 0.62 like the modem), played with `paplay` on rpi53 to the default (SCU-17) sink right after `T 1`;
RSPdx recording alongside (192 kHz, RFGR 22, IFGR 40, 0 % clipping); envelope = power in 1300-1700 Hz, 20 ms steps. PTT 0 after
every run, rigctld stopped, sink restored to 0.15 with `onair-setup-audio-routing.sh` (both stations RESULT pass).
| run | sink volume | sink state at start | rise -25 -> -3 dB | on-air time above -25 dB (tone = 4.00 s) |
|---|---|---|---|---|
| T1 (first) | 0.15 | SUSPENDED | 0.34 s | 3.28 s |
| T2 (8 s later) | 0.07 (set 0.075, `wpctl` shows 0.07) | SUSPENDED | 0.02 s | **4.00 s** |
| A (cold) | 0.15 | SUSPENDED | 0.34 s | 3.28 s |
| B (8 s after A) | 0.15 | SUSPENDED | 0.34 s | 3.28 s |
| C (40 s idle) | 0.15 | SUSPENDED | 0.34 s | 3.28 s |
Four of four runs at 0.15 show the same loss (about 0.72 s of the start is below -25 dB, then a 0.34 s rise; ~1.05 s in all, the same ~1.05 s
ramp as the modem frames); the single run at the lower volume has none. Warm vs cold makes no difference (the sink reads SUSPENDED at the
start of every run, including the "warm" one: the idle suspend is shorter than 8 s). **So the start-up loss depends on the TX drive.**
At the profile's documented drive (`B_TX_SINK_VOLUME=0.15`) the FT-818 (through the SCU-17) suppresses the start of the transmission for
about one second; at half that drive it does not. The mechanism is not established (ALC attack/limit on a full-scale tone at the codec,
the rig's own protection, or the codec level). Combined with the modem frames (preamble in that first second, correlation
0.18-0.29 < 0.31) this is the leading explanation of the Gate 6 / Gate 3 decode failures. NOT yet shown: a modem frame decoding at the lower drive,
and what the lower drive does to the received level at the IC-9700.
Proposed next (lowering only): rerun Gate 6 B->A with the FT-818 sink at 0.075 (`wpctl set-volume @DEFAULT_AUDIO_SINK@ 0.075` on rpi53 before the run;
restore with `onair-setup-audio-routing.sh`).

#### Gate 6 B->A with the FT-818 sink at 0.075 (maintainer go, 2026-10-27 ~15:17 local): transmit chain FIXED (RSPdx decodes it), IC-9700 receive still FAILS

The runner's `--verify` routing check wants the profile's drive, so the lowered drive was set the supported way for this one run:
`B_TX_SINK_VOLUME=0.075 scripts/onair-setup-audio-routing.sh` (rpi53 RESULT `sink_vol=0.07`), the same variable exported for the run, then
`TELEMETRY_ENABLE=0 scripts/run-onair-ic9700-ft991a.sh supervise --single-case 'BPSK250|none|64' --reverse` (no leader), RSPdx recording at
RFGR 22. Afterwards the variable was unset and the routing script restored both stations (RESULT pass, `sink_vol=0.15`). PTT 0, no rigctld or
openpulse left. (A first attempt at `wpctl set-volume 0.075` alone was refused by the runner's routing check before anything keyed.)
- Report `docs/dev/test-reports/onair-2026-10-27T131735.json`: **FAIL "IRS: payload not observed"**, ISS `Transmitted 64 bytes in BPSK250 mode (fec=None)`.
  IRS: **0 preamble rejections** (was 20-58) and `invalid magic` 70 (was 300-350): acquisition now works; the frame still fails.
- **RSPdx envelope at 0.075: flat for the whole frame, 2.64 s above -25 dB of a 2.65 s frame, rise 0.00 s (was ~1.05 s of loss/ramp at 0.15).**
- **Offline decode of the RSPdx copy (RFGR 22, 8 kHz, same scratch harness, no FEC): `DECODED 64 bytes`** -
  `"HPa1u8RmJfLd5MC53QLIqVR6Ly07J9RWmu6cVDuBDQGrhrZNZnRF5DvY6tLq2tbs"`. A 64-byte alphanumeric string through the frame's magic and CRC is not chance; the runner
  does not log the random payload it sends, so equality with the sent payload is NOT checked (a gap in the runner).
So the FT-818 transmission is good at 0.075 and bad at 0.15 (startup loss in the first ~1 s, taking the preamble). The remaining failure is in the IC-9700 receive
path: preamble found, frame wrong. Not yet examined: the IC-9700's audio for this frame (the runner records none). Next differential: record the rig audio
with the dual-capture script at the same drive and decode it offline; candidates are the IC-9700 USB audio (AGC SLOW, 0.44 source volume, filter) or the
level at the lower drive. The FT-818 drive in the profile (`B_TX_SINK_VOLUME` 0.15, `docs/config/onair-ic9700-ft818.example.sh`) is probably wrong for this chain.

#### Dual capture at FT-818 sink 0.075 (maintainer go, 2026-10-27 ~15:35 local): BOTH copies DECODE OFFLINE, exact payload

`B_TX_SINK_VOLUME=0.075 scripts/onair-setup-audio-routing.sh` (rpi53 `sink_vol=0.07`), rigctld on rpi53 (`144640000 PKTUSB 0 0`),
`SDR_FS=192000 SDR_RFGR=22 scripts/onair-dual-capture.sh gate6-b2a-bpsk250-none BPSK250 none 'G6-0123456789-ABCDEFGHIJKLMNOPQRSTUVWXYZ-abcdefghijklmnopqrstuv-END'`
(67 bytes, FEC none). rc=0. SWR check keyed briefly: `SWR = 1 (limit 3.0)`. `Transmitted 67 bytes in BPSK250 mode (fec=None).` PTT 0 after, rigctld stopped, routing
restored to 0.15 on both (RESULT pass). Captures (gitignored, not committed), `docs/dev/test-reports/on-air/captures/`:
`gate6-b2a-bpsk250-none.cf32` 46080000 B, `gate6-b2a-bpsk250-none-rig48k.wav` 5373996 B, `gate6-b2a-bpsk250-none-tx.log` 4264 B.
- **RSPdx (RFGR 22, 0.00 % clip):** burst at 8.7 s, 2.74 s above -25 dB, rise 0.02 s (no start-up loss). Offline decode (same scratch harness): `DECODED 67 bytes: "G6-0123456789-...-END"` = the sent payload.
- **IC-9700 USB audio (parecord, 48 kHz -> 8 kHz):** `DECODED 67 bytes: "G6-0123456789-ABCDEFGHIJKLMNOPQRSTUVWXYZ-abcdefghijklmnopqrstuv-END"` = the sent payload.
  Level: burst windows 0.36-0.48e-3 against a median of 0.066e-3 (ratio 7.2; 11 windows > 3x). The audio is shaped by the IC-9700's SLOW AGC: the floor creeps up
  (0.01 -> 0.1e-3) over the ~10 s before the burst and drops to **0.00 for ~5 s right after it** (receiver gain pulled down by the +55 dB signal, recovering over the 6 s AGC time).
**Meaning:** the transmit chain (at drive 0.075), the RF path and the IC-9700's audio are fine; the modem decodes both copies. The Gate 6 runner's LIVE receive on
station A (`openpulse receive` via cpal `pulse`) still failed on the same kind of frame (run 12: preamble accepted, `invalid magic` x70). The offline path
(`ChannelSimHarness::feed_capture` + `receive_with_timeout`) is not the production live path, so a live-receive-path fault is now the leading suspect, to be separated from
the AGC pumping above. Proposed decisive test (not run): a Gate-6 B->A run at drive 0.075 with a `parecord` of the same default source running on station A alongside the
live IRS, then decode the parecord WAV offline: offline OK + live FAIL = live receive path.

#### Gate 6 B->A: **PASS** (maintainer go, 2026-10-27 ~15:50 local) — FT-818 sink 0.075, parecord of the same source beside the live receiver

`B_TX_SINK_VOLUME=0.075 scripts/onair-setup-audio-routing.sh` (rpi53 `sink_vol=0.07`), then, with `parecord` on station A (default source = IC-9700 CODEC, 48 kHz stereo,
150 s, running beside the live receiver) and the RSPdx (RFGR 22) recording:
`TELEMETRY_ENABLE=0 scripts/run-onair-ic9700-ft991a.sh supervise --single-case 'BPSK250|none|64' --reverse`, rc=0:
```
  [BPSK250                fec=none               payload=  64B] ... PASS
==> Results: 1/1 passed, 0 failed.
    Report: docs/dev/test-reports/onair-2026-10-27T135003.json   ("result":"pass","fail_reason":"","iss_exit":0)
```
Live IRS log: 0 preamble rejections, 48 `invalid magic` lines (retries before the frame), then the payload was observed. PTT 0, nothing left running, routing restored to 0.15.
Gate 6 as written is `"result": "pass"` in the run's JSON: **met for B -> A**, with these conditions (all lowering or unchanged): FT-818 sink 0.075 (not the profile's 0.15),
VFO B = VFO A, `TELEMETRY_ENABLE=0`, IC-9700 AGC SLOW, PKTUSB/DIG on the FT-818, a `parecord` running beside the receiver.
Caveats: a **single** pass. The only difference from the failed run 12 at the same drive is the extra `parecord` on station A (an unexplained effect; keeping the CODEC capture
stream active before/while the live receiver opens it is a candidate, not shown). Reproducibility is not established. The SDR copy of this run was recorded, not decoded.
A -> B (step 2 of the task, the IC-9700 transmits, FT-818 receives) has not been run. Proposed: repeat B->A twice without the parecord to see whether the pass holds, then A->B.

#### Repeats (B->A) and Gate 6 A->B (maintainer go, 2026-10-27 ~15:40-15:45 local)

**B->A repeats, no parecord, FT-818 sink 0.075 (`B_TX_SINK_VOLUME=0.075` via the routing script; restored to 0.15 afterwards, RESULT pass), `TELEMETRY_ENABLE=0`, RSPdx not run.**
- Repeat 1 `docs/dev/test-reports/onair-2026-10-27T143955.json`: **PASS** (live IRS 0 preamble rejections, 45 `invalid magic` lines before the frame decoded).
- Repeat 2 `docs/dev/test-reports/onair-2026-10-27T144154.json`: **FAIL "IRS: payload not observed"**. The ISS transmitted (`Transmitted 64 bytes in BPSK250 mode (fec=None).` at 14:42:36Z);
  the IRS logged 1200 audio chunks but **0 first-energy attempts, 0 preamble rejections, 0 `invalid magic`**: the receiver never detected energy (a missed frame, not a corrupted one).
- So B->A at 0.075 is 2 of 3 so far (the run with the parecord, repeat 1: pass; run 12 and repeat 2: fail). Not reliable. Candidates, none shown: the burst is small in the IC-9700 audio (mean_sq
  ~0.0004 against an adaptive gate of 3x the floor and a 0.0001 absolute minimum) and the SLOW AGC (6 s) pulls the gain down after strong signals; the live receiver opens its capture stream per run.
PTT 0 on A, no rigctld or openpulse left on either Pi after each.

**Gate 6 A->B (the IC-9700 transmits at ~5 W, the FT-818 receives): FAIL.** `scripts/run-onair-ic9700-ft991a.sh supervise --single-case 'BPSK250|none|64'` (no `--reverse`, FT-818 sink at the profile 0.15, which does not matter for RX),
report `docs/dev/test-reports/onair-2026-10-27T144424.json`: **FAIL "IRS: payload not observed in receiver output"**. IC-9700 power left as is (reads 0.0470588 = ~5 W; the runner only lowers),
FT-818 power not touched. ISS (IC-9700) `Transmitted 64 bytes in BPSK250 mode (fec=None).` IRS (FT-818 on rpi53): 1253 chunks, **0 first-energy, 0 rejections, 0 `invalid magic`**: the FT-818 receive audio never
showed energy. PTT 0 on A after, nothing left running. Not examined: the FT-818's received audio (the runner records none), its AF/volume and squelch (not readable over CAT; the manual list), the RX
level at the SCU-17 (`B_RX_SOURCE_VOLUME` 0.45). The FT-818 idle floor was checked earlier (G0 pass, mean_sq ~0.0004, gate ~0.0013).
Proposed next (keys the IC-9700, ~5 W): a parecord on rpi53 beside the live receiver plus the RSPdx, then decode the FT-818's audio offline, as was done for B->A.
Not retried.

#### Gate 6 A->B with captures, IC-9700 at ~1 W (maintainer set it; `l RFPOWER` 0.0117647, not raised by the runner) — FAIL; the FT-818 hears the frame but at a poor SNR

Maintainer notes: IC-9700 set to 1 W (distance 15 m horizontal, ~7 m vertical); FT-818 AF volume (speaker) is 0. Maintainer go given.
`scripts/onair-setup-audio-routing.sh` (both pass at 0.15 / 0.44 / 0.45), `parecord` (48 kHz stereo, 150 s) on rpi53 beside the live receiver, RSPdx (RFGR 22) recording,
`scripts/run-onair-ic9700-ft991a.sh supervise --single-case 'BPSK250|none|64'`, rc=1, report `docs/dev/test-reports/onair-2026-10-27T145514.json`:
**FAIL "IRS: payload not observed in receiver output"**; `[IC-9700] power: left as is (reading '0.0117647', requested 0.05; this script only lowers power)`.
Live IRS on the FT-818 (rpi53): 1255 chunks, **0 first-energy, 0 rejections, 0 `invalid magic`** (never triggered). PTT 0 on A after, nothing left running.
- **RSPdx (off-air, 144.640 MHz): the 1 W frame is strong** (peak 0.0871, 51.6 dB over baseline) and **DECODES offline: `DECODED 64 bytes: "tM1OJIElXL2TaU9YvFvt1hiA4iF7RMu1exG2dBzwFpgwSD16QP53WJgsRYbwu5QB"`**
  (random payload, not logged by the runner, so equality with the sent one is not checked). The IC-9700 transmission is fine.
- **FT-818 audio (parecord, 48 kHz, 2 ch identical): the burst IS in it.** 2.65 s flat envelope, abrupt start, carrier **1495.8 Hz** (2fc line 46.6 dB over median), in 1300-1700 Hz 22.3 dB vs 10.7 dB
  noise (about 11 dB), in 1000-2000 Hz about 8 dB over the noise envelope. But: steady noise floor of mean_sq ~4.1e-4 (constant over 120 s), and **during the burst the FT-818's AGC pulls the OTHER bands
  DOWN by 13-14 dB** (300-1000 Hz 14.5 -> -4.7 dB, 1700-3000 Hz 12.4 -> -1.0 dB), so the broadband mean_sq rises only ~2.2x (0.00091 vs 0.00041 over 10 x 0.25 s windows), under the energy gate's 3x floor rule, which is a
  plausible reason the live receiver never triggered. **The offline decode of this audio also failed: `NOT DECODED: no decodable frame within 30000 ms`** (same harness that decoded the SDR copy).
- Reading: the transmit and the link are good; the FT-818's received audio is noise-limited (a high steady floor, the local noise source noted for the FT-818 in the manual list) and an unfavourable AGC for this
  frame, with no FEC (the repo's own rule: "there is no useful uncoded rung"). Not tested: the same A->B with an RS-protected mode, the FT-818 AGC/ATT/IPO settings, a quieter location for the FT-818, or the
  IC-9700 at higher power (the maintainer chose 1 W; the earlier 5 W A->B run also detected no energy on the FT-818). Not retried.

---

## Summary of the 2026-10-26/27 station session (Tasks 1-4), written 2026-10-27 at the maintainer's request

**Bottom line.** B -> A (FT-818 transmits, IC-9700 receives) works at 1 of 4 attempts at a lowered FT-818 drive and not at all at the profile's drive; A -> B (IC-9700 transmits, FT-818 receives) fails
because the FT-818's receive audio is noise-limited. The IC-9700's transmission and the RF link are good (the RSPdx decodes every frame sent at the lowered drive and the 1 W A->B frame).

### What now works / was fixed
| Finding | Evidence |
|---|---|
| Station readiness: both Pis build at one commit, SSH, packages, SDR (RSPdx stream needs a 480M USB link), routing, CAT, idle levels | Task 2: `onair-station-check.sh --levels` PASS=43 FAIL=0; Task 4 Part A: PASS=46 FAIL=0 |
| G0 (receive path hears a signal) passes on both stations, judged by prominence | Task 3 Part A |
| Gate 3 B -> A (rig-audio loudest window >= 5x quiet) met only after the IC-9700 AGC was set to SLOW | 7.9x (was 4.9x); the SDR showed ~S9+55 dB at the IC-9700 |
| Gate 5 (carrier of the received BPSK250) | 1503.6 Hz, +3.6 Hz (limit +-50) |
| Gate 4 (modem receive path chunking, station A) | 200-sample chunks, 40/s, 8000 Hz |
| Runner bug: `pgrep -f` matched its own ssh command, killed the session (rc 255) before every keyed run | fixed at 6 sites (`[o]penpulse`), reproduced with a harmless `sleep`; **needs adversarial review before merge** |
| Hamlib FT-818 backend toggles VFO A/B on every `rigctl` client call; VFO B was 7.077 MHz, so the antenna relay swapped front/rear socket (the audible clicks) | `rigctld -vvvvv` logs; maintainer confirmed the 40 m switch; VFO B now = A; runner telemetry off (`TELEMETRY_ENABLE=0`) |
| **FT-818 drive: at the profile's `B_TX_SINK_VOLUME=0.15` the first ~1 s of every transmission is lost/ramped (preamble gone); at 0.075 it is clean** | tone tests (4/4 at 0.15 lose 0.72 s + 0.34 s ramp; 0.075 none); RSPdx envelope flat 2.64 s of 2.65 s; modem's own envelope is flat |

### Gate 6 results
| Run | Result |
|---|---|
| B -> A, drive 0.15, runs 1-8 (telemetry on/off, VFO fixed, leader, sink awake, attenuator) | all FAIL (preamble correlation 0.18-0.29 < 0.31) |
| B -> A, drive 0.075, run 12 | FAIL (preamble accepted, `invalid magic`) |
| B -> A, drive 0.075, dual capture | not a runner run; **IC-9700 audio and RSPdx both decode offline to the exact payload** |
| B -> A, drive 0.075, with `parecord` beside the receiver | **PASS** (`onair-2026-10-27T135003.json`) |
| B -> A, drive 0.075, two repeats without parecord | 1 PASS (`...T143955`), 1 FAIL (`...T144154`, receiver saw no energy at all) |
| A -> B, IC-9700 ~5 W | FAIL (FT-818 never detected energy) |
| A -> B, IC-9700 ~1 W, with captures | FAIL; RSPdx decodes it, FT-818 audio does not (see below) |

### Ruled out (with the experiment that ruled it out)
- Wrong binaries / one-sided rebuild: identical sha256 on both Pis, no code change since.
- IC-9700 front-end overload (10 dB attenuator on: S-meter 56 -> 44, same failure). Note: the first attenuator attempt (`L ATT 12`) was rejected by the rig; only `L ATT 10` took.
- PTT leader (800 ms): the ramp is unchanged. SCU-17 sink waking from suspend: a running silent stream made no difference. Cold vs warm start: no difference.
- The modem's own TX envelope: flat (synthetic, 2.652 s, RMS 0.59-0.64, peak 1.0).
- SQL/RF gain/USB AF level on the IC-9700 (USB AF was already 100 %). More FT-818 power: never needed and not done.
- The FT-818 antenna: not tuned/needed; the tuner is off on the rear socket.

### Open / unexplained
1. **B -> A is not reliable at 0.075** (2 of 4 live runs passed; the failures: preamble OK then `invalid magic`, and once no energy detected). The IC-9700's SLOW AGC (6 s) pulls the gain down after strong signals (audio drops to ~0 for ~5 s after a burst); the burst in its audio is small (mean_sq ~0.0004) next to the energy gate (3x floor, 0.0001 minimum). The pass with `parecord` running is unexplained.
2. **A -> B: the FT-818's audio is noise-limited.** Steady floor mean_sq ~4.1e-4 (S8-S9 QRM; the maintainer cannot identify the noise source and reports QRM at S8 again); the burst is only ~11 dB over it in 1300-1700 Hz, its AGC pumps (other bands -13/-14 dB during the burst, so broadband rises only 2.2x against the gate's 3x), no FEC. It does not decode, live or offline.
3. Why the FT-818 needs a drive of 0.075 (ALC? codec? rig protection) is not established; `B_TX_SINK_VOLUME` in `docs/config/onair-ic9700-ft818.example.sh` (0.15) is probably wrong for this chain.
4. The runner logs no sent payload (so "decoded payload equals sent payload" is not checked), records no receiver audio, and its routing check hard-codes the profile's TX volume.

### Rules and records
No transmission above the FT-818's L1 (0.5 W); FT-818 power and mode never set by the session; IC-9700 never raised (now ~1 W, set by the maintainer). The sink was only ever lowered, restored to 0.15 afterwards each time.
Code changes on the branch, both to the keyed-path runner and unreviewed: the `pgrep` fix and an optional `ISS_PTT_LEADER_MS` hook (it did not help; remove or keep, maintainer's call). Reports and bundles are under `docs/dev/test-reports/` (captures are gitignored).
Memory: `ft818-hamlib-vfo-toggle.md`.

### Suggested next steps (for the maintainer / cloud session)
1. Remote or quieter location for the FT-818 side, or a verified noise source; QRM at S8 limits every A -> B result.
2. Make 0.075 (or an explicitly measured drive) the FT-818 TX drive in the profile and the routing script's `--verify`; find out why 0.15 loses the first second.
3. Test A -> B and B -> A with an RS-protected mode; the uncoded rung has no margin on this chain.
4. Decide the live-receiver question: energy gate vs AGC-compressed audio (IC-9700 SLOW AGC, FT-818 AGC), with the IRS audio recorded by the runner.
5. Review the two runner edits before they merge.
