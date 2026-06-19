#!/usr/bin/env bash
# Idempotent setup for the single-clock virtual audio loopback (snd-aloop).
#
# Creates a virtual ALSA loopback so the modem can be driven TX->RX through the
# real cpal+ALSA path on one host, with a SHARED clock and no analog cable.
# This is the "virtual" transport rung (see docs/dev/virtual-loopback.md):
# it isolates DSP/code behaviour from the two-independent-clock + analog effects
# of the hardware loopback rig.
#
# snd-aloop cross-links PCM device 0 <-> device 1: audio played on (dev0, subN)
# is captured on (dev1, subN). cpal matches --device by exact enumerated name and
# ALSA namehints only expose DEV=0, so we publish two named plug PCMs (aloop_tx /
# aloop_rx) with hint blocks pointing at hw:Loopback,0,0 and hw:Loopback,1,0.
set -euo pipefail

ASOUNDRC="${HOME}/.asoundrc"
BEGIN="# >>> OpenPulseHF virtual-loopback (snd-aloop) >>>"
END="# <<< OpenPulseHF virtual-loopback (snd-aloop) <<<"

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

# 2) Named PCMs in ~/.asoundrc (idempotent: replace our managed block).
if [[ -f "$ASOUNDRC" ]] && grep -qF "$BEGIN" "$ASOUNDRC"; then
    # Strip the previous managed block.
    sed -i "/$(printf '%s' "$BEGIN" | sed 's/[][\.*^$/]/\\&/g')/,/$(printf '%s' "$END" | sed 's/[][\.*^$/]/\\&/g')/d" "$ASOUNDRC"
fi
cat >> "$ASOUNDRC" <<EOF
$BEGIN
pcm.aloop_tx {
    type plug
    slave.pcm "hw:Loopback,0,0"
    hint { show on description "OpenPulse virtual-loop TX (Loopback dev0)" }
}
pcm.aloop_rx {
    type plug
    slave.pcm "hw:Loopback,1,0"
    hint { show on description "OpenPulse virtual-loop RX (Loopback dev1)" }
}
$END
EOF
echo "==> wrote aloop_tx / aloop_rx PCMs to $ASOUNDRC"

# 3) Verify ALSA enumerates them.
if aplay -L 2>/dev/null | grep -q '^aloop_tx' && arecord -L 2>/dev/null | grep -q '^aloop_rx'; then
    echo "==> OK: aloop_tx (playback) and aloop_rx (capture) are enumerable"
else
    echo "WARN: aloop PCMs not visible in aplay -L / arecord -L; check ALSA config" >&2
fi
echo "Done. Build the cpal CLI ('cargo build --release -p openpulse-cli') then run scripts/run-loopback-virtual.sh"
