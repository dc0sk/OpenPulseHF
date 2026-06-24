#!/usr/bin/env bash
# Capture IQ on the SDR host while the station transmits one keyed burst.
#
# Starts sdr_capture.py in the background, waits briefly, then triggers a gated
# keyed burst on the station over SSH (keyplay.py must already be on the station;
# see README.md), and reports the capture summary when both finish.
#
# Usage:
#   cap_during_tx.sh <center_hz> <fs_hz> <dur_s> <rfgr> <ifgr> <out.cf32> <wav_basename> <watts>
#
# Env:
#   STATION       ssh target of the station (default dc0sk-rpi53)
#   STATION_WAVDIR directory on the station holding the WAVs (default /tmp)
#   KEYPLAY       path to keyplay.py on the station (default /tmp/keyplay.py)
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
STATION="${STATION:-dc0sk-rpi53}"
WAVDIR="${STATION_WAVDIR:-/tmp}"
KEYPLAY="${KEYPLAY:-/tmp/keyplay.py}"

center=$1; fs=$2; dur=$3; rfgr=$4; ifgr=$5; out=$6; wav=$7; watts=$8

python "$HERE/sdr_capture.py" "$center" "$fs" "$dur" "$rfgr" "$ifgr" "$out" >"$out.log" 2>&1 &
cap=$!
sleep 1.2
ssh "$STATION" "timeout 20 python3 $KEYPLAY $WAVDIR/$wav $watts" 2>&1 | grep -E "TXSTART|UNKEY|ABORT"
wait "$cap"
grep -E "cfg:|captured" "$out.log"
