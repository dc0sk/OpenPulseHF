---
project: openpulsehf
doc: docs/config/README.md
status: review
last_updated: 2026-10-15
---

# Configuration Examples

This folder contains copy-and-adapt examples for OpenPulseHF configuration files. The typed config
schema and every default live in `crates/openpulse-config/src/lib.rs`; `openpulse config init`
prints a commented template of the current schema. The manual's
[Chapter 12](../openpulse-manual.md#chapter-12-configuration-file-overview) summarises the files.

## Runtime config (TOML)

- `openpulse.config.toml`: runtime config for OpenPulse binaries (`~/.config/openpulse/config.toml`).
  `[modem] profile` takes `fast` (default) or `robust` only; any other name is rejected.
- `openpulse-tx500.toml`: example runtime config for the TX500 station.
- `openpulse-kx3.toml`: example runtime config for the KX3 station.
- `rig-icom-ic7300.toml`: generic serial CAT definition for the Icom IC-7300 (`[radio] cat_backend = "generic"`).
- `rig-yaesu-ft817.toml`: generic serial CAT definition for the Yaesu FT-817.

## Service environment

- `pki-tooling.env.example`: environment file template for the `pki-tooling` service (not a
  Release 1 component).

## On-air runner profiles (shell, `source` before the runner)

- `onair-ic9700-ft818.example.sh`: **the current 2 m station pair** — IC-9700 on rpi51 (A) and
  FT-818 + SCU-17 on rpi53 (B), 144.640 MHz. Runner: `scripts/run-onair-ic9700-ft991a.sh` (it takes
  any station B; the name is historical). Values marked `TODO-CONFIRM` must be read from the
  hardware first. Station procedure: [docs/dev/onair-ic9700-ft818-setup.md](../dev/onair-ic9700-ft818-setup.md).
- `onair-ic9700-ft991a.example.sh`: dual-SSH profile for IC-9700 (dc0sk-rpi51) and FT-991A
  (dd2zm-landline), 2 m. Also supports a side-A-only `sidea` smoke test using the IC-9700 internal
  USB soundcard and CAT PTT by default.
  Note: the Pi runs PulseAudio and holds the USB CODEC exclusively; use `A_AUDIO_DEVICE=pulse`, not `hw:CARD=CODEC,DEV=0`.
- `onair-twin-ota.example.sh`: two real `openpulse-server` daemons with OTA adaptive rate-stepping
  over the air, watched in `openpulse-twinview`. Runner: `scripts/run-onair-twin-ota.sh`.
- `onair-tx500-kx3.example.sh`: combined SSH profile for the TX500 Raspberry Pi and KX3 laptop
  (HF). Runner: `scripts/run-onair-tx500-kx3.sh`.
- `onair-tx500-kx3-local.example.sh`: the same pair with the KX3 on the local machine.
- `onair-stations.example.sh`: generic station pair for `scripts/run-onair-tests.sh` and
  `scripts/run-onair-validation-flow.sh`. Those runners accept `PTT_BACKEND` `none`, `rigctld`,
  `rts`, `dtr`, `vox`, `cm108` or `gpio`.
- `loopback-audio-devices.sh`: pinned USB audio device names for the rpi51/rpi52 cable loopback.

Rules the 2 m runners enforce (`run-onair-ic9700-ft991a.sh`, `run-onair-twin-ota.sh`):

- **Power is only ever lowered.** `A_RFPOWER` / `B_RFPOWER` (Hamlib 0–1 scale) are applied only
  when below the rig's current reading; an empty value leaves the rig's power alone
  (`scripts/lib/rig-power.sh`). The FT-818 cannot set power over Hamlib: set L1 on its panel.
- **`keep` mode.** A station whose mode is set on the rig (the FT-818's DIG/USER-U) uses `keep`, and
  must name the expected `rigctl m` reading: `TEST_MODE_RIG_B=keep` with `TEST_MODE_EXPECT_B` in the
  IC-9700 runner, `B_TEST_MODE_RIG=keep` with `B_TEST_MODE_EXPECT` in the twin-OTA runner. An unset
  or mismatched expectation stops the run before keying.
- **2 m band guard.** Every test frequency must lie in 144.500–144.750 MHz
  (`BAND2M_MIN_HZ`/`BAND2M_MAX_HZ` in the IC-9700 runner, `BAND_MIN_HZ`/`BAND_MAX_HZ` in the twin-OTA
  runner), or the runner exits.
- Callsigns must not be `N0CALL`.
