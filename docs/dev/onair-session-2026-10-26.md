---
project: openpulsehf
doc: docs/dev/onair-session-2026-10-26.md
status: resolved
last_updated: 2026-10-26
---

# On-air session wrap-up, 2026-10-26 (run on rpi53, Station B)

Branch checked out on both Pis: `decision-27-local` = `origin/claude/affectionate-brahmagupta-sdn8qg` @ `87c5abe0`
(decision 27 docs, on top of `main` 8fe05837; not merged to `main`). Callsign DC0SK. Only one transmission
sequence was made: two bounded 4 s tones from the FT-818 at L1. Nothing else keyed.

## State per station

| | Station A: rpi51 + IC-9700 | Station B: rpi53 + FT-818 + SCU-17 |
|---|---|---|
| Code | `87c5abe0`, release build done (cpal), strict preflight 11/11 (rho test skipped) | same; strict preflight 11/11 (rho test skipped) |
| Config | `~/.config/openpulse/config.toml`, callsign DC0SK | same |
| CAT | `/dev/serial/by-id/...IC-9700_13012889_A-if00-port0`, 115200, Hamlib 3081; read `f`/`m` OK | `/dev/serial/by-id/...CP2105...00B5B685-if00-port0` (ttyUSB0), 38400, Hamlib 1041 (4.6.2); read `f`/`m` OK |
| Mode read | was FM; set to PKTUSB, 144.640 MHz | `PKTUSB`, passband 0 (DIG/USER-U); unchanged. `TEST_MODE_EXPECT_B=PKTUSB` |
| RX capture level (PipeWire source) | 0.55 -> **0.44**; idle mean_sq 0.000500 (OK, ceiling 0.00107) | 1.00 -> **0.45**; idle mean_sq 0.000518 (OK) |
| TX sink | not touched | 0.40 -> **0.15** (documented drive) |
| Packages added | python3-soapysdr, soapysdr-tools + modules, libusb/libudev(+dev), cmake, build-essential, libsoapysdr-dev; **SDRplay API 3.15.2 installed by the user** (service `sdrplay` active) | pulseaudio-utils |
| RSPdx | on USB (1df7:3030). **`SoapySDRPlay3` not built**, so `sdr_capture.py` cannot open it | none |

Local profile (untracked): `~/onair-local/ic9700-ft818.sh`, sources the repo example, overrides `B_CAT_PORT`
and `TEST_MODE_EXPECT_B`. PipeWire volumes do not survive a reboot. The scratchpad does not either.

## Results

- **G0 idle floor, `scripts/onair-rx-idle-floor.sh` (raw ALSA read): FAIL on both.** Not narrow birdies:
  rpi53 flat broadband at -36 dBFS, max line prominence 4 dB (FT-818 shows S8-S9 local noise, to be
  moved); rpi51 48 weak "lines" at 4-16 dB prominence on a -44 dBFS floor. The script reads ALSA raw, so it
  overstates level versus the PipeWire path. The RSPdx-streaming G0 on rpi51 has **not** been run.
  An earlier rpi53 "pass" was void: the FT-818 was off.
- **RX level check `scripts/onair-rx-level-check.sh`: PASS** both (0.000500 / 0.000518) after the volumes above.
- **Gate 1 (CAT): PASS** both, read-only.
- **Gate 2 (FT-818 keyed tone, 144.640, L1, front BNC to Diamond RH3): PASS.** SWR 1.4, ALC raw 40 (not on a
  0-1 scale), RFPOWER raw 1. IC-9700 STRENGTH stepped from about -50 dB to **+51..+52 dB over S9** when the
  carrier arrived. PTT released and verified (`t` = 0).
- **Gates 3-6, `--quick`, A1, A2, A4/A5, Pat session: NOT run.**
- `rho_calibration_receive::the_veto_stands_down...` (#1060 / REQ-RX-03, run by strict preflight): the first run
  FAILED (cause not captured); the rerun was unoptimised and ran >30 min, so it was stopped with no verdict.

## Gotchas

- `pkill -f <text in my own command>` kills the invoking shell; use PIDs.
- `rigctl` takes ~2 s to open the FT-818 port, so a "key" timestamp is not the carrier start.
- Hamlib FT-818: `RFPOWER`/`ALC`/`SWR` are raw; cannot set or read power or antenna (`y` fails); `l RFPOWER_METER` unsupported.
- SSH forwarding: agent socket only in the sshd-started shell. Symlink `~/.ssh/ssh_auth_sock` via `.bashrc`.
- Permission blocks hit: running downloaded code as root; cloning and building from GitHub. Both need an explicit rule.
- `sh install_lib.sh` fails under dash; the SDRplay installer needs `bash`.
- Scripted G0 needs scipy: venv with `--system-site-packages` in the scratchpad (rpi51 already has it).

## Open items

1. Build `SoapySDRPlay3` on rpi51 (`git clone pothosware/SoapySDRPlay3`, cmake, make, `sudo make install`), then
   `SoapySDRUtil --find`; if "no sdrplay device matches": `sudo systemctl restart sdrplay`.
2. RSPdx maximum input rating: UNCHECKED. Strong coupling seen (+52 dB over S9 at 0.5 W): attenuate before the
   IC-9700-transmitting direction.
3. Confirm TOT is enabled on both rigs (A5 backstop); not readable over CAT.
4. FT-818: front BNC antenna, L1, DIG / USER-U, CAT RATE 38400 were reported by the owner; none readable via CAT.
5. Re-check FT-818 RX level after it moves away from the local noise source.
6. G1 PTT leader by SDR capture; Pi receive cost with and without capture; F8 corpus re-record; #1060 test.
7. Decision 27 docs branch not yet merged.
