#!/usr/bin/env bash
# audio-device-sweep-a.sh — play a 1 kHz tone to every ALSA playback device on
# Station A in turn, asserting CAT PTT for each attempt, so you can watch the
# radio for ALC/RF movement and find which device reaches the TX audio chain.
#
# Usage:
#   source docs/config/onair-ic9700-ft991a.example.sh
#   ./scripts/audio-device-sweep-a.sh
#
# Press Ctrl-C at any time to stop; PTT is always released on exit.

set -euo pipefail

A_SSH="${A_SSH:-dc0sk@dc0sk-rpi51}"
SSH_OPTS="${SSH_OPTS:--o BatchMode=yes -o ConnectTimeout=10}"
A_RIGCTLD_ADDR="${A_RIGCTLD_ADDR:-127.0.0.1}"
A_RIGCTLD_PORT="${A_RIGCTLD_PORT:-4532}"
TONE_DURATION="${TONE_DURATION:-3}"   # seconds of audio per device
PAUSE_BETWEEN="${PAUSE_BETWEEN:-2}"   # seconds between devices

ssh_a() {
    # shellcheck disable=SC2086
    ssh ${SSH_OPTS} "${A_SSH}" "$@"
}

release_ptt() {
    ssh_a "rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} T 0 >/dev/null 2>&1 || true" || true
    echo ""
    echo "[sweep] PTT released (cleanup)"
}
trap release_ptt EXIT

# Verify rigctld is reachable.
echo "[sweep] checking rigctld on ${A_SSH} …"
if ! ssh_a "rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} _ >/dev/null 2>&1"; then
    echo "ERROR: rigctld not responding on ${A_SSH}:${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT}" >&2
    echo "       Run setup or start rigctld manually before sweeping." >&2
    exit 1
fi
echo "[sweep] rigctld OK"

# Generate a 1 kHz sine wave WAV on the remote host once; reuse for all devices.
echo "[sweep] generating tone on ${A_SSH} …"
ssh_a "python3 - <<'PYEOF'
import struct, math, sys
rate = 8000
dur  = ${TONE_DURATION}
freq = 1000
n    = rate * dur
samples = [int(32767 * math.sin(2 * math.pi * freq * i / rate)) for i in range(n)]
data   = struct.pack('<' + 'h' * n, *samples)
header = struct.pack('<4sI4s4sIHHIIHH4sI',
    b'RIFF', 36 + len(data), b'WAVE',
    b'fmt ', 16, 1, 1, rate, rate * 2, 2, 16,
    b'data', len(data))
with open('/tmp/openpulse-sweep-tone.wav', 'wb') as f:
    f.write(header + data)
print('tone written: /tmp/openpulse-sweep-tone.wav (%d bytes)' % (len(header) + len(data)))
PYEOF
"

# Enumerate all hw:N,M ALSA playback devices.
echo "[sweep] enumerating ALSA playback devices …"
devices_raw="$(ssh_a "aplay -l 2>/dev/null | grep '^card [0-9]'" || true)"
if [[ -z "$devices_raw" ]]; then
    echo "ERROR: no ALSA playback devices found on ${A_SSH}" >&2
    exit 1
fi

# Parse lines like: "card 1: CODEC [USB Audio CODEC], device 0: USB Audio [USB Audio]"
mapfile -t device_lines < <(echo "$devices_raw")

echo ""
echo "[sweep] found ${#device_lines[@]} device(s):"
for line in "${device_lines[@]}"; do
    echo "         $line"
done
echo ""
echo "[sweep] starting sweep — tone_duration=${TONE_DURATION}s, pause=${PAUSE_BETWEEN}s"
echo "        Watch the radio ALC/RF-POWER meter for each attempt."
echo ""

idx=0
for line in "${device_lines[@]}"; do
    # Extract card number and device number.
    card_num="$(echo "$line" | grep -oP '(?<=^card )\d+')"
    dev_num="$(echo "$line" | grep -oP '(?<=, device )\d+')"
    card_label="$(echo "$line" | grep -oP '(?<=: )[^[]+(?=\[)' | head -n1 | tr -d ' ')"
    hw_dev="hw:${card_num},${dev_num}"
    idx=$(( idx + 1 ))

    echo "──────────────────────────────────────────────────────"
    echo "[${idx}/${#device_lines[@]}] device: ${hw_dev}   (${card_label})"
    echo "          PTT on → playing ${TONE_DURATION}s tone …"

    # Assert PTT.
    ssh_a "rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} T 1 >/dev/null 2>&1 || true"
    sleep 0.3

    # Play tone; suppress aplay errors (device busy / wrong type) gracefully.
    play_result="ok"
    ssh_a "aplay -D '${hw_dev}' -q /tmp/openpulse-sweep-tone.wav 2>/tmp/openpulse-sweep-aplay.err || true"
    aplay_err="$(ssh_a "cat /tmp/openpulse-sweep-aplay.err 2>/dev/null || true" || true)"
    if [[ -n "$aplay_err" ]]; then
        play_result="aplay error: ${aplay_err}"
    fi

    # Release PTT.
    ssh_a "rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} T 0 >/dev/null 2>&1 || true"

    if [[ "$play_result" == "ok" ]]; then
        echo "          PTT off — audio sent without error"
    else
        echo "          PTT off — ${play_result}"
    fi

    if (( idx < ${#device_lines[@]} )); then
        echo "          (waiting ${PAUSE_BETWEEN}s before next device)"
        sleep "${PAUSE_BETWEEN}"
    fi
done

echo ""
echo "──────────────────────────────────────────────────────"
echo "[sweep] all ${#device_lines[@]} device(s) tested."
echo "        If ALC/RF moved on one attempt, that device is your TX audio path."
echo ""
echo "        IMPORTANT: if the Pi runs PulseAudio/PipeWire (pactl info),"
echo "        PulseAudio holds the USB CODEC exclusively. Direct hw: writes"
echo "        appear to succeed but produce no RF — use A_AUDIO_DEVICE=pulse"
echo "        in the profile instead, and confirm the correct default sink:"
echo "          pactl info | grep 'Default Sink'"
