#!/usr/bin/env bash
# On-air station pair configuration.
# Copy to config/onair-stations.sh and fill in your values before running
# scripts/deploy-rpi-pair.sh or scripts/run-onair-tests.sh.
# This file is NOT committed (add config/onair-stations.sh to .gitignore).

# SSH target for station A (ISS — initiating station).
export STATION_A="dc0sk@192.168.1.10"

# SSH target for station B (IRS — responding station).
export STATION_B="dc0sk@192.168.1.11"

# Extra SSH options (e.g. non-default port, identity file).
# Examples:
#   export SSH_OPTS="-p 2222"
#   export SSH_OPTS="-i ~/.ssh/id_rpi_openpulse -p 22"
export SSH_OPTS=""

# Amateur callsigns.
export CALLSIGN_A="K1ABC"
export CALLSIGN_B="K2DEF"

# PTT backend: rts | dtr | vox | rigctld | none
export PTT_BACKEND="rts"

# Serial port for PTT (used by rts/dtr backends).
export PTT_SERIAL_A="/dev/ttyUSB0"
export PTT_SERIAL_B="/dev/ttyUSB0"

# rigctld address:port (used by rigctld backend only).
export RIGCTLD_ADDR_A="127.0.0.1:4532"
export RIGCTLD_ADDR_B="127.0.0.1:4532"

# Audio device names (leave empty for system default).
export AUDIO_DEVICE_A=""
export AUDIO_DEVICE_B=""
