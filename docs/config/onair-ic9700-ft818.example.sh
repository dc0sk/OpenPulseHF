#!/usr/bin/env bash
# Profile for the IC-9700 <-> FT-818 pairing (2 m, 144.640 MHz).
#
#   Station A (stationary): IC-9700 on dc0sk-rpi51, reached over the VPN tunnel
#   Station B (portable):   FT-818 + SCU-17 + LDG Z817, on THIS laptop (local)
#
# Usage:
#   source docs/config/onair-ic9700-ft818.example.sh
#   ./scripts/run-onair-ic9700-ft991a.sh supervise --quick   # runner is rig-B-agnostic
#
# The runner filename still says "ft991a" — it is really "IC-9700 + a configurable
# Station B". Everything rig-B-specific is set below; no script edit is needed.
#
# ============================ CONFIRM BEFORE THE RUN ============================
# The SCU-17 was NOT connected when this profile was written, so every value marked
# TODO-CONFIRM must be filled in from the real hardware. Discovery commands are given
# inline. See docs/dev/onair-ic9700-ft818-setup.md for the full station procedure,
# including the FT-818 menu settings (rear antenna, DIG/USER-U mode) that are set on
# the radio and are not configurable here.
# ================================================================================

# ── SSH targets ────────────────────────────────────────────────────────────────
# A is remote over the VPN. B is THIS laptop; the runner always uses ssh, so B points
# at localhost — the laptop must run sshd and accept a key for the local user.
#   Enable once:  sudo systemctl enable --now sshd   (and add your key to authorized_keys)
export A_SSH="dc0sk@dc0sk-rpi51"          # over the VPN tunnel
export B_SSH="dc0sk@localhost"            # FT-818 on this laptop
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
export A_RFPOWER=0.5

# ── Station B — FT-818 + SCU-17 on THIS laptop ─────────────────────────────────
# CAT serial port of the SCU-17. TODO-CONFIRM once plugged in:
#   ls -l /dev/serial/by-id/    # the SCU-17 enumerates a virtual COM port for CAT
export B_CAT_PORT="TODO-CONFIRM"          # e.g. /dev/serial/by-id/usb-Silicon_Labs_...-if00-port0
# FT-818 CAT rate — set the FT-818 menu "CAT RATE" to match. 38400 is responsive.
export B_CAT_BAUD=38400                   # confirm against the FT-818 menu setting
# PTT: CAT keying via hamlib is the most robust (survives USB re-enumeration) and
# matches Side A. The SCU-17's hardware RTS-PTT is the alternative (B_PTT_TYPE=RTS +
# B_PTT_PORT = the SCU-17 serial port); pick ONE and verify with `calibrate ptt`.
export B_PTT_TYPE="CAT"
export B_PTT_PORT=""                      # only needed if B_PTT_TYPE=RTS (the SCU-17 port)
export B_RIGCTLD_ADDR="127.0.0.1"
export B_RIGCTLD_PORT=4532
# SCU-17 USB Audio CODEC on this laptop. TODO-CONFIRM once plugged in:
#   arecord -l | grep -i codec      # find the card
#   ./target/release/openpulse --backend cpal devices | grep -i codec   # exact cpal name
export B_AUDIO_DEVICE="TODO-CONFIRM"      # cpal device name of the SCU-17 CODEC
export B_AUDIO_DEVICE_LABEL="FT-818 SCU-17 USB Audio CODEC"
export B_REPO_DIR='${HOME}/git/OpenPulseHF'
export B_LOG_DIR='${HOME}/var/log/openpulse/on-air'
export B_RFPOWER=0.5                      # ~2-3 W of the FT-818's 6 W; confirm on the rig

# ── Frequency / mode / 2 m safety guard (same as the earlier 2 m runs) ──────────
export TEST_FREQ_HZ=144640000             # 144.640 MHz (moved off 144.651 to dodge a 1286 Hz birdie)
export TEST_MODE_RIG="PKTUSB"             # IC-9700 side; the FT-818 uses DIG/USER-U (see the setup doc)
export BAND2M_MIN_HZ=144500000            # the runner refuses to key outside this range
export BAND2M_MAX_HZ=144750000

# ── Timing (corrected values) ──────────────────────────────────────────────────
export IRS_STARTUP_WAIT=10                # RX AFC settle ~6.4 s + margin
export KILL_WAIT=12
export TX_TIMEOUT=120

# ── Telemetry ──────────────────────────────────────────────────────────────────
export TELEMETRY_ENABLE=1
export TELEMETRY_SAMPLES=40
export TELEMETRY_INTERVAL=0.2

# ── Notes carried into the report metadata ─────────────────────────────────────
export ON_AIR_FIRST_PASS_NOTE="IC-9700(rpi51,stationary) <-> FT-818(laptop,portable), 2 m 144.640, low power, agreed window"

# The rpi51<->rpi52 audio-loopback regression from the FT-991A profile is NOT wired
# here (rpi52 is not part of this pairing). Leave it disabled.
export LOOPBACK_REGRESSION_INTERVAL=0
export POWER_CYCLE_ENABLE=0

export SIDE_A_SINGLE_CASE="BPSK250|none|64"
export ALLOW_TUNER_ON_HIGH_SWR=0          # the Z817 is HF-only; it cannot tune 2 m — do not rely on it
export QSY_MODE_ENABLED=0
