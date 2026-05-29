#!/usr/bin/env bash
# Combined SSH profile for a two-station on-air run:
# - Station A: Lab599 TX500 on a Raspberry Pi via Digirig
# - Station B: Elecraft KX3 on a Linux laptop via Digirig
#
# Copy this file to config/onair-tx500-kx3.sh, fill in the hostnames/callsigns,
# and source it before running scripts/onair-tx500-kx3-supervisor.sh.

export STATION_A="tx500-pi@example.net"
export STATION_B="kx3-laptop@example.net"

# Keep ssh-agent loaded and use BatchMode so the run fails fast if a key is missing.
export SSH_OPTS="-o BatchMode=yes"

export CALLSIGN_A="TX500TEST"
export CALLSIGN_B="KX3TEST"

export TX500_CONFIG_FILE="docs/config/openpulse-tx500.toml"
export KX3_CONFIG_FILE="docs/config/openpulse-kx3.toml"

# Remote defaults use $HOME so the same profile works on both hosts.
export TX500_REMOTE_CONFIG='${HOME}/.config/openpulse/config.toml'
export KX3_REMOTE_CONFIG='${HOME}/.config/openpulse/config.toml'
export TX500_REMOTE_BIN_DIR='${HOME}/bin'
export KX3_REMOTE_BIN_DIR='${HOME}/bin'
export TX500_REMOTE_LOG_DIR='${HOME}/var/log/openpulse/on-air'
export KX3_REMOTE_LOG_DIR='${HOME}/var/log/openpulse/on-air'

# Preserve the close-range first-pass assumption.
export ON_AIR_FIRST_PASS_NOTE="low power, external attenuation, close spacing"