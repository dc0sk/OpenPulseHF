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
#   rigctl -l | grep -i "kx3"      # should show 2045
#   rigctl -l | grep -i "tx-500"   # should show 2050  (run on Pi)

# ── SSH target ────────────────────────────────────────────────────────────────
# FILL IN: SSH target for the Raspberry Pi 5 (user@host or ~/.ssh/config alias)
export PI_SSH="dc0sk@dc0sk-rpi51"
export SSH_OPTS="-o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=30"

# ── Callsigns ─────────────────────────────────────────────────────────────────
# FILL IN: your licensed callsigns
export CALLSIGN_A="DC0SK"      # TX500 on Pi    — ISS (initiating)
export CALLSIGN_B="DC0SK"      # KX3 on laptop  — IRS (responding)
export GRID_A="JN49"
export GRID_B="JN49"

# ── KX3 (local, this laptop) ──────────────────────────────────────────────────
# Hamlib model 2045 = Elecraft KX3
# KX3 CAT baud rate: check Menu > MNU > KXUSB on the rig (typically 38400).
# Digirig on Linux usually creates one serial port for CAT; PTT via RTS on the
# same port.  Check with: ls /dev/ttyUSB* and dmesg | tail -20 after plugging in.
export KX3_HAMLIB_MODEL=2045
export KX3_CAT_PORT="/dev/serial/by-id/usb-Silicon_Labs_CP2102N_USB_to_UART_Bridge_Controller_a22a349173e7ed1192816d6262c613ac-if00-port0"
export KX3_CAT_BAUD=38400             # KX3 default; match rig Menu > KXUSB setting
export KX3_PTT_PORT="/dev/serial/by-id/usb-Silicon_Labs_CP2102N_USB_to_UART_Bridge_Controller_a22a349173e7ed1192816d6262c613ac-if00-port0"
export KX3_PTT_TYPE="RTS"
export KX3_RIGCTLD_ADDR="127.0.0.1"
export KX3_RIGCTLD_PORT=4532

# ── TX500 (Lab599 TX-500 on Raspberry Pi 5) ───────────────────────────────────
# Hamlib model 2050 = Lab599 TX-500
# TX-500 uses USB-C for CAT; Digirig provides the serial interface on the Pi.
# Check /dev/ttyUSB* on the Pi after plugging in the Digirig.
export TX500_HAMLIB_MODEL=2050
export TX500_CAT_PORT="/dev/serial/by-id/usb-Silicon_Labs_CP2102N_USB_to_UART_Bridge_Controller_c4e324f98dafeb119418142518997a59-if00-port0"    # Verified on this Pi via rigctl probe
export TX500_CAT_BAUD=9600               # Verified on this Pi via rigctl probe
export TX500_PTT_PORT="/dev/serial/by-id/usb-Silicon_Labs_CP2102N_USB_to_UART_Bridge_Controller_c4e324f98dafeb119418142518997a59-if00-port0"
export TX500_PTT_TYPE="RTS"
export TX500_RIGCTLD_ADDR="127.0.0.1"
export TX500_RIGCTLD_PORT=4532

# ── RF parameters ─────────────────────────────────────────────────────────────
# Both rigs are tuned to this dial frequency (USB) by the script.
# For first-pass close-range tests with attenuation any clear frequency works.
export TEST_FREQ_HZ=14070000    # 20m USB — FILL IN or override per session
export TEST_MODE_RIG="USB"

# ── Audio devices (OpenPulse --device names) ─────────────────────────────────
# OpenPulse matches these device names exactly against `openpulse devices`.
# Leave empty to use the system default device, but for multi-device systems the
# on-air runner should pin them explicitly.
#
# Laptop: the Digirig audio card is the USB Audio Device on the same USB branch
# as the CP210 serial adapter; in `openpulse devices` this appears as
# `sysdefault:CARD=Device`.
# Pi: the Digirig audio card is the USB PnP Sound Device on the sibling USB
# branch to the verified TX500 CAT adapter `/dev/ttyUSB3`; in `openpulse
# devices` this appears as `sysdefault:CARD=Device_1`.
export LOCAL_AUDIO_DEVICE="sysdefault:CARD=Device"
export PI_AUDIO_DEVICE="sysdefault:CARD=Device_1"

# ── Remote paths (Pi) ─────────────────────────────────────────────────────────
# Single-quoted so $HOME expands on the Pi, not on this machine.
export PI_REPO_DIR='${HOME}/git/OpenPulseHF'
export PI_LOG_DIR='${HOME}/var/log/openpulse/on-air'

# ── Timing ────────────────────────────────────────────────────────────────────
export IRS_STARTUP_WAIT=5       # seconds to wait for IRS TNC to become ready
export TX_TIMEOUT=120           # seconds before ISS transmit is declared failed

# ── Safety note ───────────────────────────────────────────────────────────────
export ON_AIR_FIRST_PASS_NOTE="low power, external attenuation, close spacing"
