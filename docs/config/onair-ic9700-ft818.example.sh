#!/usr/bin/env bash
# Profile for the IC-9700 <-> FT-818 pairing (2 m, 144.640 MHz).
#
#   Station A (stationary): IC-9700 on dc0sk-rpi51, reached over the VPN tunnel
#   Station B:              FT-818 + SCU-17 + LDG Z817, on dc0sk-rpi53
#
# Usage:
#   source docs/config/onair-ic9700-ft818.example.sh
#   ./scripts/run-onair-ic9700-ft991a.sh supervise --quick   # runner is rig-B-agnostic
#
# The runner filename still says "ft991a" — it is really "IC-9700 + a configurable
# Station B". Everything rig-B-specific is set below; no script edit is needed.
#
# ============================ CONFIRM BEFORE THE RUN ============================
# Values marked TODO-CONFIRM must be filled in from the real hardware; discovery commands are
# given inline. Read on rpi53 (2026-10-14): CAT baud, audio device, the CAT port's interface. See docs/dev/onair-ic9700-ft818-setup.md for the full station procedure,
# including the FT-818 settings made on the radio and not configurable here: the FRONT antenna
# (selected, and it stays selected), L1 power (0.5 W; never raised), DIG/USER-U mode.
# ================================================================================

# ── SSH targets ────────────────────────────────────────────────────────────────
# The runner reaches both stations over ssh, including the one it runs on. From 2026-10-26 the
# session runs ON rpi51 (the control host), so rpi51 needs key-based ssh to itself and to rpi53:
# docs/dev/onair-ic9700-ft818-setup.md, Station preparation, step 1. scripts/onair-station-check.sh verifies it.
export A_SSH="dc0sk@dc0sk-rpi51"          # this host when the session runs on rpi51
export B_SSH="dc0sk@dc0sk-rpi53"          # FT-818 on rpi53
export SSH_OPTS='-o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=30'

# ── Callsigns / labels ─────────────────────────────────────────────────────────
export CALLSIGN_A="DC0SK"                 # IC-9700 (stationary)
export CALLSIGN_B="DC0SK"                 # FT-818 (portable) — set the portable call/SSID
export A_LABEL="IC-9700"
export B_LABEL="FT-818"

# ── Hamlib models (verified: rigctl -l) ────────────────────────────────────────
export A_HAMLIB_MODEL=3081                # Icom IC-9700
export B_HAMLIB_MODEL=1041                # Yaesu FT-818

# ── Station A — IC-9700 on rpi51 (unchanged from the working profile) ───────────
export A_CAT_PORT="/dev/serial/by-id/usb-Silicon_Labs_CP2102N_USB_to_UART_Bridge_Controller_IC-9700_13012889_A-if00-port0"
export A_CAT_BAUD=115200
export A_PTT_PORT="/dev/serial/by-id/usb-Silicon_Labs_CP2102N_USB_to_UART_Bridge_Controller_IC-9700_13012889_B-if00-port0"
export A_PTT_TYPE="CAT"                   # CAT PTT on the IC-9700 (RTS is a fallback only)
export A_RIGCTLD_ADDR="127.0.0.1"
export A_RIGCTLD_PORT=4532
# IC-9700 USB CODEC is held exclusively by PulseAudio; use the pulse device, not hw:.
export A_AUDIO_DEVICE="pulse"
export A_AUDIO_DEVICE_LABEL="IC-9700 USB Audio CODEC (PulseAudio)"
export A_REPO_DIR='${HOME}/git/OpenPulseHF'
export A_RFPOWER=0.05                     # IC-9700: ~5 W of 100 W (0.5 would be ~50 W — do not)
# IC-9700 AGC SLOW (Hamlib reads 3). With FAST the USB audio held a S9+55 dB burst at 4.9x the idle
# floor and Gate 3 failed; SLOW gave 7.9x (2026-10-27). Set on the rig; the station check reads it.
export A_EXPECT_AGC=3

# ── Station B — FT-818 + SCU-17 on rpi53 ─────────────────────────────────
# CAT serial port of the SCU-17. TODO-CONFIRM once plugged in:
#   ls -l /dev/serial/by-id/    # the SCU-17's CP2105 has two ports; CAT is the FIRST (-if00-port0)
# On rpi53 (2026-10-14) CAT answered on the CP2105's FIRST port (...CP2105...-if00-port0); on
# 2026-10-26 that was the CP2105 with serial 00B5B685. scripts/onair-station-check.sh lists the
# by-id paths present on rpi53 while this is unset: copy the -if00-port0 one here.
export B_CAT_PORT="TODO-CONFIRM"          # the full /dev/serial/by-id/...CP2105...00B5B685-if00-port0 path
# FT-818 CAT rate — set the FT-818 menu "CAT RATE" to match. 38400 is responsive.
export B_CAT_BAUD=38400                   # read over CAT on rpi53, 2026-10-14
# PTT: CAT keying via hamlib is the most robust (survives USB re-enumeration) and
# matches Side A. The SCU-17's hardware RTS-PTT is the alternative (B_PTT_TYPE=RTS +
# B_PTT_PORT = the SCU-17 serial port); pick ONE and verify with `calibrate ptt`.
export B_PTT_TYPE="CAT"
export B_PTT_PORT=""                      # only needed if B_PTT_TYPE=RTS (the SCU-17 port)
export B_RIGCTLD_ADDR="127.0.0.1"
export B_RIGCTLD_PORT=4532
# SCU-17 USB Audio CODEC on rpi53. Found with:
#   arecord -l | grep -i codec      # find the card
#   ./target/release/openpulse --backend cpal devices | grep -i codec   # exact cpal name
# "pulse", as on station A: the modem then goes through PipeWire, whose default source and sink
# onair-setup-audio-routing.sh points at the SCU-17 CODEC at the levels below. With a direct
# plughw:CARD=CODEC,DEV=0 (the ALSA name, 2026-10-14) the modem bypassed PipeWire, so neither the
# 0.15 TX drive nor the 0.45 capture level applied to it (review-station-readiness-2026-10-26.md).
export B_AUDIO_DEVICE="pulse"
export B_AUDIO_DEVICE_LABEL="FT-818 SCU-17 USB Audio CODEC"
export B_REPO_DIR='${HOME}/git/OpenPulseHF'
export B_LOG_DIR='${HOME}/var/log/openpulse/on-air'
# Empty: the runner sends no power to the FT-818. Set L1 (0.5 W) on the front panel; Hamlib cannot
# set FT-818 power, and the runners only ever lower a rig's power (scripts/lib/rig-power.sh).
export B_RFPOWER=""

# ── Frequency / mode / 2 m safety guard (same as the earlier 2 m runs) ──────────
export TEST_FREQ_HZ=144640000             # 144.640 MHz (moved off 144.651 to dodge a 1286 Hz birdie)
export TEST_MODE_RIG_A="PKTUSB"           # IC-9700
export TEST_MODE_RIG_B="keep"             # FT-818: no mode sent; DIG/USER-U is set on the radio
# What `rigctl m` reads once DIG/USER-U is set on the FT-818: PKTUSB (passband 0), read on rpi53 on
# 2026-10-26 with Hamlib 4.6.2. The run stops if the reading differs.
export TEST_MODE_EXPECT_B="PKTUSB"
export BAND2M_MIN_HZ=144500000            # the runner refuses to key outside this range
export BAND2M_MAX_HZ=144750000

# ── Audio levels (PipeWire; lost on reboot — scripts/onair-setup-audio-routing.sh re-applies) ──
# Capture levels measured on 2026-10-26 so the idle floor sits under the energy gate's ceiling
# (onair-rx-level-check.sh: idle mean-sq 0.000500 on A, 0.000518 on B, ceiling 0.00107).
export A_RX_SOURCE_VOLUME=0.44
export B_RX_SOURCE_VOLUME=0.45
# FT-818 drive 0.075, measured 2026-10-27: at 0.15 the FT-818 (through the SCU-17) suppresses the first
# ~1 s of every transmission (4 of 4 tone tests lost 0.72 s, then a 0.34 s rise; the preamble goes with
# it); at 0.075 the envelope is flat from the first 20 ms and the RSPdx decodes every frame. Why 0.15
# does this (ALC, codec, rig protection) is not established. onair-tasks.md, task 4.
export B_TX_SINK_VOLUME=0.075

# ── Witness SDR and preflight ──────────────────────────────────────────────────
export WITNESS_SDR_SIDE=A                 # the RSPdx is on rpi51 (decision 27)
# The #1060 stand-down proof failed on rpi53 (cause not captured) and, run on x86 in release on
# 2026-10-26, failed its own precondition: the 250 Hz fixture derived 0.468, below the PN-63
# delivered-frame bound 0.51, so it no longer reaches the stand-down path (traceability.md,
# 2026-10-26). Skipped until the fixture is replaced (work plan M3 parking line).
export SKIP_RHO_STANDDOWN=1

# ── Timing (corrected values) ──────────────────────────────────────────────────
export IRS_STARTUP_WAIT=10                # RX AFC settle ~6.4 s + margin
export KILL_WAIT=12
export TX_TIMEOUT=120

# ── Telemetry ──────────────────────────────────────────────────────────────────
# Off: Hamlib's FT-817/818 backend toggles VFO A/B on every rigctl client call to read VFO B, so the
# runner's telemetry loop (t, ALC, RF meter, ~3 calls per 0.2 s) swapped the FT-818 between VFOs while it
# transmitted; with VFO B on 7.077 MHz that is the 40 m band (2026-10-27, rigctld -vvvvv). Keep VFO B on
# the test frequency on the rig, and do not poll the FT-818 during a transmission.
export TELEMETRY_ENABLE=0
export TELEMETRY_SAMPLES=40
export TELEMETRY_INTERVAL=0.2

# ── Notes carried into the report metadata ─────────────────────────────────────
export ON_AIR_FIRST_PASS_NOTE="IC-9700(rpi51,stationary) <-> FT-818(rpi53), 2 m 144.640, low power, agreed window"

# The rpi51<->rpi52 audio-loopback regression from the FT-991A profile is NOT wired
# here (rpi52 is not part of this pairing). Leave it disabled.
export LOOPBACK_REGRESSION_INTERVAL=0
export POWER_CYCLE_ENABLE=0

export SIDE_A_SINGLE_CASE="BPSK250|none|64"
export ALLOW_TUNER_ON_HIGH_SWR=0          # the Z817 is HF-only; it cannot tune 2 m — do not rely on it
export QSY_MODE_ENABLED=0
