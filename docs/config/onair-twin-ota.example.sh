#!/usr/bin/env bash
# Profile for the ON-AIR TWIN-OTA scenario: two real openpulse-server daemons on
# two RF stations, driving receiver-led OTA adaptive rate-stepping over the air,
# observed live in openpulse-twinview. The real-radio counterpart of the
# in-process twin-station rig (docs §13.2.A2/A3).
#
# Fill in every value marked "FILL IN" before sourcing, then:
#   source docs/config/onair-twin-ota.example.sh
#   ./scripts/run-onair-twin-ota.sh supervise
#
# Find serial ports:   ls -la /dev/serial/by-id/ 2>/dev/null
# Verify Hamlib model: rigctl -l | grep -i "<your rig>"
# List cpal devices:   openpulse devices   (or arecord -L / aplay -L)

# ── SSH targets (both stations are remote daemons) ────────────────────────────
export A_SSH="dc0sk@dc0sk-rpi51"          # FILL IN: station A (ISS — drives traffic)
export B_SSH="dd2zm@dd2zm-landline"       # FILL IN: station B (IRS — receiver-led)
export SSH_OPTS='-o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=30'

# ── Callsigns / grids ─────────────────────────────────────────────────────────
export CALLSIGN_A="DC0SK"                 # FILL IN
export CALLSIGN_B="DD2ZM"                 # FILL IN
export GRID_A="JN49"
export GRID_B="JN49"
export A_LABEL="Station A"
export B_LABEL="Station B"

# ── Station A rig (CAT + PTT via one rigctld) ─────────────────────────────────
export A_HAMLIB_MODEL=3081                # FILL IN (rigctl -l)
export A_CAT_PORT="/dev/serial/by-id/FILL-IN-A-CAT"
export A_CAT_BAUD=115200
export A_PTT_PORT="/dev/serial/by-id/FILL-IN-A-PTT"   # may equal A_CAT_PORT
export A_PTT_TYPE="RTS"                   # RTS | DTR | CAT  (FT-991A etc. → CAT)
export A_RIGCTLD_PORT=4532
export A_AUDIO_DEVICE="pulse"             # FILL IN: cpal device name (openpulse devices)
export A_REPO_DIR='${HOME}/git/OpenPulseHF'

# ── Station B rig ─────────────────────────────────────────────────────────────
export B_HAMLIB_MODEL=1035               # FILL IN
export B_CAT_PORT="/dev/serial/by-id/FILL-IN-B-CAT"
export B_CAT_BAUD=38400
export B_PTT_PORT="/dev/serial/by-id/FILL-IN-B-PTT"
export B_PTT_TYPE="CAT"
export B_RIGCTLD_PORT=4532
export B_AUDIO_DEVICE="pulse"            # FILL IN
export B_REPO_DIR='${HOME}/openpulse/OpenPulseHF'

# ── RF parameters ─────────────────────────────────────────────────────────────
# Pick a band/frequency legal for both stations; keep power low for first passes.
export TEST_FREQ_HZ=14070000             # FILL IN (e.g. 14070000 / 7040000 / 144640000)
export TEST_MODE_RIG="USB"               # USB | PKTUSB depending on the rig
export A_RFPOWER=0.10                     # 0.0–1.0 fraction of full power
export B_RFPOWER=0.10

# ── OTA / modem ───────────────────────────────────────────────────────────────
# hpx_hf / hpx_modcod exercise the modulation×FEC ladder; hpx500 is the simplest.
export OTA_PROFILE="hpx_hf"
export START_MODE="BPSK250"              # daemon [modem] mode (initial)
export DAEMON_TCP_PORT=9000             # each daemon's control port (same on both hosts)

# ── Traffic generation (station A → B drives the ladder) ──────────────────────
export TRAFFIC_INTERVAL=2                # seconds between sends
export TRAFFIC_SIZE=128                  # random body bytes per send
export TRAFFIC_DURATION=120             # seconds to run the OTA traffic for

# ── Output ────────────────────────────────────────────────────────────────────
export OUTPUT_DIR="docs/dev/test-reports"
export ON_AIR_NOTE="twin-OTA on-air scenario, low power, agreed test window"
