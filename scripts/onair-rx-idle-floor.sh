#!/usr/bin/env bash
# Phase G0 gate: capture a rig's RECEIVE USB-audio idle floor and check it is clean.
#
# The on-air rig-to-rig blocker is computer-borne RFI conducted into each rig's USB-audio
# capture — narrow birdies 30-40 dB over the noise floor, in the modem passband. This
# records a short IDLE capture (no TX anywhere, SDR stopped) and runs onair-rx-idle-floor.py
# on it. It is the check that Phase G0's galvanic-USB-isolation fix actually worked; run it
# until it PASSES on both rigs before any modem run (see docs/dev/onair-execution-plan.md).
#
# Usage:
#   scripts/onair-rx-idle-floor.sh <alsa-capture-device>
#   scripts/onair-rx-idle-floor.sh plughw:CARD=CODEC,DEV=0
#   DURATION=8 RATE=48000 scripts/onair-rx-idle-floor.sh hw:2,0
#
# Run it OVER SSH for a remote rig:
#   ssh dc0sk@dc0sk-rpi53 'cd ~/git/OpenPulseHF && scripts/onair-rx-idle-floor.sh plughw:CARD=CODEC,DEV=0'
#
# Env:
#   DURATION   capture seconds (default 5)
#   RATE       capture sample rate, Hz — the card's NATIVE rate avoids resampler artefacts
#              (default 48000; USB CODECs are 48 kHz native)
#   OUT        wav path (default a temp file)
#   Plus every OPHF_* knob of onair-rx-idle-floor.py (band, prominence, abs level, max birdies).
#
# Exit: 0 = clean (PASS), 1 = birdies (FAIL), 2 = capture/tool error.
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

DEVICE="${1:-}"
if [[ -z "$DEVICE" ]]; then
    echo "usage: $0 <alsa-capture-device>   (e.g. plughw:CARD=CODEC,DEV=0)" >&2
    echo "list devices with: arecord -L   or   arecord -l" >&2
    exit 2
fi

DURATION="${DURATION:-5}"
RATE="${RATE:-48000}"
OUT="${OUT:-/tmp/onair-rx-idle-${RATE}.wav}"

for t in arecord python3; do
    command -v "$t" >/dev/null 2>&1 || { echo "ERROR: missing required tool: $t" >&2; exit 2; }
done

# Loud warning if anything that could put energy on the band is running — the whole point is
# an IDLE floor. This does not kill anything (it might be the operator's session); it warns.
if pgrep -x openpulse >/dev/null 2>&1 || pgrep -x openpulse-server >/dev/null 2>&1 \
    || pgrep -f 'sdr_capture' >/dev/null 2>&1 || pgrep -x aplay >/dev/null 2>&1; then
    echo "WARNING: an openpulse / SDR / aplay process is running. This capture must be IDLE" >&2
    echo "         (no TX anywhere, SDR stopped) or the floor reading is meaningless." >&2
fi

echo "==> Capturing ${DURATION}s of idle RX audio from '${DEVICE}' at ${RATE} Hz ..."
if ! arecord -D "$DEVICE" -f S16_LE -r "$RATE" -c 1 -d "$DURATION" -t wav "$OUT" 2>/tmp/onair-arecord.err; then
    echo "ERROR: arecord failed on device '${DEVICE}':" >&2
    sed 's/^/    /' /tmp/onair-arecord.err >&2
    echo "    Check the device name (arecord -L) and that no other process holds it." >&2
    exit 2
fi

echo "==> Analysing idle floor ..."
python3 "${REPO_ROOT}/scripts/onair-rx-idle-floor.py" "$OUT"
rc=$?

echo ""
if [[ $rc -eq 0 ]]; then
    echo "G0 PASS for '${DEVICE}'. This rig's receive path can hear a signal in the passband."
else
    echo "G0 FAIL for '${DEVICE}'. Do NOT run a modem matrix on this rig — it cannot hear a" >&2
    echo "signal buried under those lines. Apply galvanic USB isolation and re-run this gate." >&2
fi
exit $rc
