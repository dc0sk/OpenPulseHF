#!/usr/bin/env bash
# Profile for: Elecraft KX3 on THIS laptop (IRS) + Lab599 TX500 on Raspberry Pi 5 (ISS)
# Both rigs use Digirig for audio and CAT control.
#
# Fill in every value marked "FILL IN" before sourcing this file.
# Then run:
#   source docs/config/onair-tx500-kx3-local.example.sh
#   ./scripts/run-onair-tx500-kx3.sh supervise --quick
#
# To find serial ports on this laptop:
#   ls -la /dev/ttyUSB* /dev/ttyACM* 2>/dev/null
#   udevadm info -a /dev/ttyUSB0 | grep -E 'idVendor|idProduct|manufacturer'
#
# To find serial ports on the Pi:
#   ssh $PI_SSH 'ls -la /dev/ttyUSB* /dev/ttyACM* 2>/dev/null'
#
# To verify Hamlib model numbers:
#   rigctl -l | grep -i "kx3"      # should show 229
#   rigctl -l | grep -i "tx-500"   # should show 3020  (run on Pi)

# ── SSH target ────────────────────────────────────────────────────────────────
# FILL IN: SSH target for the Raspberry Pi 5 (user@host or ~/.ssh/config alias)
export PI_SSH="pi@raspberrypi.local"
export SSH_OPTS="-o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=30"

# ── Callsigns ─────────────────────────────────────────────────────────────────
# FILL IN: your licensed callsigns
export CALLSIGN_A="N0CALL"      # TX500 on Pi    — ISS (initiating)
export CALLSIGN_B="N0CALL"      # KX3 on laptop  — IRS (responding)
export GRID_A="AA00"
export GRID_B="AA00"

# ── KX3 (local, this laptop) ──────────────────────────────────────────────────
# Hamlib model 229 = Elecraft KX3
# KX3 CAT baud rate: check Menu > MNU > KXUSB on the rig (typically 38400).
# Digirig on Linux usually creates one serial port for CAT; PTT via RTS on the
# same port.  Check with: ls /dev/ttyUSB* and dmesg | tail -20 after plugging in.
export KX3_HAMLIB_MODEL=229
export KX3_CAT_PORT="/dev/ttyUSB0"     # FILL IN: CAT serial port on this laptop
export KX3_CAT_BAUD=38400             # KX3 default; match rig Menu > KXUSB setting
export KX3_PTT_PORT="/dev/ttyUSB0"    # Usually same port; RTS line drives PTT
export KX3_PTT_TYPE="RTS"
export KX3_RIGCTLD_ADDR="127.0.0.1"
export KX3_RIGCTLD_PORT=4532

# ── TX500 (Lab599 TX-500 on Raspberry Pi 5) ───────────────────────────────────
# Hamlib model 3020 = Lab599 TX-500
# TX-500 uses USB-C for CAT; Digirig provides the serial interface on the Pi.
# Check /dev/ttyUSB* on the Pi after plugging in the Digirig.
export TX500_HAMLIB_MODEL=3020
export TX500_CAT_PORT="/dev/ttyUSB0"    # FILL IN: CAT serial port on the Pi
export TX500_CAT_BAUD=19200             # TX-500 default
export TX500_PTT_PORT="/dev/ttyUSB0"
export TX500_PTT_TYPE="RTS"
export TX500_RIGCTLD_ADDR="127.0.0.1"
export TX500_RIGCTLD_PORT=4532

# ── RF parameters ─────────────────────────────────────────────────────────────
# Both rigs are tuned to this dial frequency (USB) by the script.
# For first-pass close-range tests with attenuation any clear frequency works.
export TEST_FREQ_HZ=14070000    # 20m USB — FILL IN or override per session
export TEST_MODE_RIG="USB"

# ── Remote paths (Pi) ─────────────────────────────────────────────────────────
# Single-quoted so $HOME expands on the Pi, not on this machine.
export PI_REPO_DIR='${HOME}/git/OpenPulseHF'
export PI_LOG_DIR='${HOME}/var/log/openpulse/on-air'

# ── Timing ────────────────────────────────────────────────────────────────────
export IRS_STARTUP_WAIT=5       # seconds to wait for IRS TNC to become ready
export TX_TIMEOUT=120           # seconds before ISS transmit is declared failed

# ── Safety note ───────────────────────────────────────────────────────────────
export ON_AIR_FIRST_PASS_NOTE="low power, external attenuation, close spacing"
