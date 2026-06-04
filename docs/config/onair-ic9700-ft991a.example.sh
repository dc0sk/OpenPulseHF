#!/usr/bin/env bash
# Profile for dual-SSH setup:
#   Station A (ISS): IC-9700 on dc0sk-rpi51
#   Station B (IRS): FT-991A on dd2zm-landline (about 10 km away)
#
# Usage:
#   source docs/config/onair-ic9700-ft991a.example.sh
#   ./scripts/run-onair-ic9700-ft991a.sh supervise --quick

# SSH targets (must be reachable via ssh-agent keys)
export A_SSH="dc0sk@dc0sk-rpi51"
export B_SSH="dd2zm@dd2zm-landline"
export SSH_OPTS='-o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=30'

# Callsigns
export CALLSIGN_A="DC0SK"   # IC-9700 (ISS)
export CALLSIGN_B="DD2ZM"   # FT-991A (IRS)

# Friendly labels for report output
export A_LABEL="IC-9700"
export B_LABEL="FT-991A"

# Hamlib models (verify with: rigctl -l | grep -i 'ic-9700\|ft-991')
export A_HAMLIB_MODEL=3081
export B_HAMLIB_MODEL=1035

# Station A rig/CAT/PTT settings
export A_CAT_PORT="/dev/serial/by-id/usb-Silicon_Labs_CP2102N_USB_to_UART_Bridge_Controller_IC-9700_13012889_A-if00-port0"
export A_CAT_BAUD=115200
export A_PTT_PORT="/dev/serial/by-id/usb-Silicon_Labs_CP2102N_USB_to_UART_Bridge_Controller_IC-9700_13012889_B-if00-port0"
export A_PTT_TYPE="RTS"
export A_RIGCTLD_ADDR="127.0.0.1"
export A_RIGCTLD_PORT=4532

# Station B rig/CAT/PTT settings
export B_CAT_PORT="/dev/serial/by-id/usb-Silicon_Labs_CP2105_Dual_USB_to_UART_Bridge_Controller_008924A1-if00-port0"
export B_CAT_BAUD=38400
export B_PTT_PORT="/dev/serial/by-id/usb-Silicon_Labs_CP2105_Dual_USB_to_UART_Bridge_Controller_008924A1-if01-port0"
export B_PTT_TYPE="RTS"
export B_RIGCTLD_ADDR="127.0.0.1"
export B_RIGCTLD_PORT=4532

# 2m safety guard for this test window (script enforces this range)
export BAND2M_MIN_HZ=144500000
export BAND2M_MAX_HZ=144750000
export TEST_FREQ_HZ=144650000
export TEST_MODE_RIG="USB"

# Optional audio device pinning per station (leave empty for default)
export A_AUDIO_DEVICE="sysdefault:CARD=CODEC"
export B_AUDIO_DEVICE="default:CARD=CODEC"

# Paths:
# A is a normal repo checkout and used as build source.
export A_REPO_DIR='${HOME}/git/OpenPulseHF'

# B can be non-git with limited disk; runner keeps repo-like layout and only
# transfers needed binaries to target/release.
export B_REPO_DIR='${HOME}/openpulse/OpenPulseHF'
export B_LOG_DIR='${HOME}/var/log/openpulse/on-air'

# Timing
export IRS_STARTUP_WAIT=5
export TX_TIMEOUT=120

# Safety note for report metadata
export ON_AIR_FIRST_PASS_NOTE="2m only, low power, agreed test window"

# Optional high-SWR tuner policy (all values are explicit opt-in defaults).
# When enabled, the runner checks SWR and attempts integrated tuner operation
# on rigs that support the Hamlib TUNER function.
export ALLOW_TUNER_ON_HIGH_SWR=0
export HIGH_SWR_THRESHOLD=2.0

# If QSY mode is enabled for the run, trigger the same SWR+tuner policy after
# tune/QSY transitions.
export QSY_MODE_ENABLED=0
export TUNER_TRIGGER_ON_QSY=1
