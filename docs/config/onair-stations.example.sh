#!/usr/bin/env bash
# Example on-air station pair configuration.

# SSH target for station A (ISS).
export STATION_A="dc0sk@192.168.1.10"

# SSH target for station B (IRS).
export STATION_B="dc0sk@192.168.1.11"

# Optional SSH args.
export SSH_OPTS=""

# Amateur callsigns.
export CALLSIGN_A="K1ABC"
export CALLSIGN_B="K2DEF"

# PTT backend: rts | dtr | vox | rigctld | none
export PTT_BACKEND="rts"

# Serial ports for RTS/DTR backends.
export PTT_SERIAL_A="/dev/ttyUSB0"
export PTT_SERIAL_B="/dev/ttyUSB0"

# rigctld endpoints for rigctld backend.
export RIGCTLD_ADDR_A="127.0.0.1:4532"
export RIGCTLD_ADDR_B="127.0.0.1:4532"

# Optional audio device names.
export AUDIO_DEVICE_A=""
export AUDIO_DEVICE_B=""
