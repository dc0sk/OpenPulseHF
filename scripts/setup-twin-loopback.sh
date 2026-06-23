#!/usr/bin/env bash
# Idempotent setup for the BIDIRECTIONAL virtual audio loopback (snd-aloop) used
# by the real-audio twin-station rig.
#
# The in-process twin rig (openpulse_daemon::twin) bridges two daemons through a
# channel model in memory. THIS rig instead routes two REAL openpulse-server
# daemons through the real cpal+ALSA+resampler path, so it also exercises the
# audio stack where on-air-specific bugs live (resampler, format, dual-clock when
# run on hardware). One shared kernel clock here (snd-aloop); use the dual-card
# rig for a true two-clock test.
#
# snd-aloop cross-links PCM device 0 <-> device 1 per subdevice: a full-duplex
# endpoint on hw:Loopback,0,0 talks to a full-duplex endpoint on hw:Loopback,1,0.
#   station A = aloop_a (hw:Loopback,0,0): A.play -> B.capture, A.capture <- B.play
#   station B = aloop_b (hw:Loopback,1,0): B.play -> A.capture, B.capture <- A.play
# Each named plug PCM has no IOID restriction, so cpal enumerates it for both
# capture and playback (full duplex), unlike the one-way aloop_tx/aloop_rx pair.
set -euo pipefail

ASOUNDRC="${HOME}/.asoundrc"
BEGIN="# >>> OpenPulseHF twin-loopback (snd-aloop) >>>"
END="# <<< OpenPulseHF twin-loopback (snd-aloop) <<<"

# 1) Kernel module.
if ! lsmod | grep -q '^snd_aloop'; then
    echo "==> loading snd-aloop (needs sudo)"
    sudo modprobe snd-aloop
else
    echo "==> snd-aloop already loaded"
fi
if ! aplay -l 2>/dev/null | grep -q 'card .*Loopback'; then
    echo "ERROR: Loopback card not present after modprobe snd-aloop" >&2
    exit 1
fi

# 2) Named full-duplex PCMs in ~/.asoundrc (idempotent: replace our managed block).
if [[ -f "$ASOUNDRC" ]] && grep -qF "$BEGIN" "$ASOUNDRC"; then
    sed -i "/$(printf '%s' "$BEGIN" | sed 's/[][\.*^$/]/\\&/g')/,/$(printf '%s' "$END" | sed 's/[][\.*^$/]/\\&/g')/d" "$ASOUNDRC"
fi
cat >> "$ASOUNDRC" <<EOF
$BEGIN
pcm.aloop_a {
    type plug
    slave.pcm "hw:Loopback,0,0"
    hint { show on description "OpenPulse twin station A (Loopback dev0, full duplex)" }
}
pcm.aloop_b {
    type plug
    slave.pcm "hw:Loopback,1,0"
    hint { show on description "OpenPulse twin station B (Loopback dev1, full duplex)" }
}
$END
EOF
echo "==> wrote aloop_a / aloop_b full-duplex PCMs to $ASOUNDRC"

# 3) Verify ALSA enumerates them for BOTH directions.
ok=1
for dir_cmd in "aplay -L" "arecord -L"; do
    for pcm in aloop_a aloop_b; do
        if ! $dir_cmd 2>/dev/null | grep -q "^${pcm}$"; then
            echo "WARN: '$pcm' not visible in '$dir_cmd'" >&2
            ok=0
        fi
    done
done
if [[ "$ok" == 1 ]]; then
    echo "==> OK: aloop_a and aloop_b are enumerable for capture and playback"
else
    echo "WARN: some PCMs not visible for both directions; cpal may not open them full-duplex" >&2
fi
echo "Done. Run scripts/run-twin-station-audio.sh to launch two daemons over this loopback."
