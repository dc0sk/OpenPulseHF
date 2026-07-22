#!/usr/bin/env bash
# Idempotent setup for the two-soundcard hardware loopback on a SINGLE host.
#
# This is the "hardware" transport rung (see docs/dev/dualcard-loopback.md) made
# runnable on one PC: two USB soundcards, each with its OWN clock, joined by an
# analog cable (TX card line-out -> RX card mic-in). It adds exactly what the
# single-clock virtual rig (snd-aloop) cannot: two independent sample clocks
# (sample-rate offset/drift) and the analog cable. No SSH, no two Raspberry Pis.
#
# It publishes two named plug PCMs (hwloop_tx / hwloop_rx) in ~/.asoundrc that
# resample 8 kHz <-> the cards' native 48 kHz and carry hint blocks so the cpal
# CLI enumerates them by exact name (cpal matches --device on the enumerated
# name; bare plughw:CARD=... and DEV=1 are not enumerated -- same constraint the
# virtual rig hit).
#
# The two cards are pinned by their physical USB path (stable across replug),
# not by the volatile ALSA card name (Device / Device_1) or index, which the
# kernel assigns in enumeration order.
set -euo pipefail

ASOUNDRC="${HOME}/.asoundrc"
BEGIN="# >>> OpenPulseHF dualcard-loopback (two USB soundcards) >>>"
END="# <<< OpenPulseHF dualcard-loopback (two USB soundcards) <<<"

# Physical USB ports of the two cards (from /dev/snd/by-path). Override if your
# adapters live on different ports -- run `ls /dev/snd/by-path` to find them.
TX_BYPATH="${TX_BYPATH:-pci-0000:07:00.3}"   # card whose OUTPUT drives the cable
RX_BYPATH="${RX_BYPATH:-pci-0000:07:00.4}"   # card whose INPUT receives the cable

# RX capture gain (raw mic-capture units, 0..max). NOT the card max: these C-Media
# adapters expose a MIC input with a +23 dB preamp, and a line-level output drives
# it into hard clipping at full gain (measured: modem TX peaks ~0.79 FS at gain 16,
# would clip well above ~20). 16 gives a strong, unclipped signal on this cable.
CAPTURE_GAIN="${CAPTURE_GAIN:-16}"

# Resolve a by-path prefix to an ALSA card index via the controlCN symlink.
_card_index_for() {  # by-path-prefix -> card index (or empty)
    local prefix="$1" link target
    link="$(ls /dev/snd/by-path/ 2>/dev/null | grep -F "${prefix}" | grep -- '-usb-' | head -1)"
    [[ -z "$link" ]] && return 1
    target="$(readlink "/dev/snd/by-path/${link}")"   # e.g. ../controlC3
    [[ "$target" =~ controlC([0-9]+) ]] && echo "${BASH_REMATCH[1]}"
}

TX_IDX="$(_card_index_for "$TX_BYPATH" || true)"
RX_IDX="$(_card_index_for "$RX_BYPATH" || true)"

if [[ -z "$TX_IDX" || -z "$RX_IDX" ]]; then
    echo "ERROR: could not resolve both USB cards by path." >&2
    echo "  TX_BYPATH=${TX_BYPATH} -> card '${TX_IDX:-<none>}'" >&2
    echo "  RX_BYPATH=${RX_BYPATH} -> card '${RX_IDX:-<none>}'" >&2
    echo "  Available USB audio paths:" >&2
    ls /dev/snd/by-path/ 2>/dev/null | grep -- '-usb-' | sed 's/^/    /' >&2
    exit 1
fi
if [[ "$TX_IDX" == "$RX_IDX" ]]; then
    echo "ERROR: TX and RX resolved to the same card ($TX_IDX). Check TX_BYPATH/RX_BYPATH." >&2
    exit 1
fi

TX_NAME="$(cat /proc/asound/card${TX_IDX}/id 2>/dev/null || echo "card${TX_IDX}")"
RX_NAME="$(cat /proc/asound/card${RX_IDX}/id 2>/dev/null || echo "card${RX_IDX}")"
echo "==> TX card: index ${TX_IDX} ('${TX_NAME}')  on ${TX_BYPATH}"
echo "==> RX card: index ${RX_IDX} ('${RX_NAME}')  on ${RX_BYPATH}"

# 1) Named PCMs in ~/.asoundrc (idempotent: replace our managed block).
if [[ -f "$ASOUNDRC" ]] && grep -qF "$BEGIN" "$ASOUNDRC"; then
    sed -i "/$(printf '%s' "$BEGIN" | sed 's/[][\.*^$/]/\\&/g')/,/$(printf '%s' "$END" | sed 's/[][\.*^$/]/\\&/g')/d" "$ASOUNDRC"
fi
cat >> "$ASOUNDRC" <<EOF
$BEGIN
pcm.hwloop_tx {
    type plug
    slave.pcm "hw:${TX_IDX},0"
    hint { show on description "OpenPulse dualcard-loop TX (USB card ${TX_IDX} ${TX_NAME})" }
}
pcm.hwloop_rx {
    type plug
    slave.pcm "hw:${RX_IDX},0"
    hint { show on description "OpenPulse dualcard-loop RX (USB card ${RX_IDX} ${RX_NAME})" }
}
$END
EOF
echo "==> wrote hwloop_tx (card ${TX_IDX}) / hwloop_rx (card ${RX_IDX}) PCMs to $ASOUNDRC"

# 2) Normalise mixers on BOTH cards: disable hardware AGC, max capture gain,
#    enable + max the speaker (PCM playback) output, mute mic sidetone so the
#    RX card does not loop its own input back out. Names are identical across
#    this C-Media model; tolerate absent controls.
_normalise_card() {  # card-index
    local c="$1" maxspk
    maxspk="$(amixer -c "$c" cget numid=6 2>/dev/null | grep -oE 'max=[0-9]+' | head -1 | cut -d= -f2)"
    amixer -c "$c" cset name='Auto Gain Control' 0          >/dev/null 2>&1 || true
    amixer -c "$c" cset name='Mic Capture Switch' 1          >/dev/null 2>&1 || true
    amixer -c "$c" cset name='Mic Capture Volume' "${CAPTURE_GAIN}" >/dev/null 2>&1 || true
    amixer -c "$c" cset name='Speaker Playback Switch' 1     >/dev/null 2>&1 || true
    amixer -c "$c" cset name='Speaker Playback Volume' "${maxspk:-151}" >/dev/null 2>&1 || true
    amixer -c "$c" cset name='Mic Playback Switch' 0         >/dev/null 2>&1 || true
}
_normalise_card "$TX_IDX"
_normalise_card "$RX_IDX"

# Read the AGC back rather than announcing it. Every `amixer` call above ends in `|| true`, so a
# renamed control or a card that has since moved leaves this claiming a state it did not set — and a
# live capture AGC moves the level *during* a frame, which reads downstream as the amplitude-carrying
# modes (64QAM, the dense SC-FDMA QAMs) failing on the analog path. That misclassified eight modes
# until the rig was re-normalised and six of them passed unchanged (2026-07-22).
_agc_is_on() {  # card-index
    local v
    v="$(amixer -c "$1" cget name='Auto Gain Control' 2>/dev/null | sed -n 's/^[[:space:]]*:[[:space:]]*values=//p')"
    [[ "${v%%,*}" == "on" || "${v%%,*}" == "1" ]]
}
_agc_bad=0
for _i in "$TX_IDX" "$RX_IDX"; do
    if _agc_is_on "$_i"; then
        echo "ERROR: capture AGC is still ON for card ${_i} after normalisation." >&2
        echo "  amixer -c ${_i} cset name='Auto Gain Control' off" >&2
        _agc_bad=1
    fi
done
if [[ $_agc_bad -ne 0 ]]; then
    echo "Sweep results from this rig would not be attributable to the modem." >&2
    exit 1
fi
echo "==> normalised mixers (AGC off [verified], capture=${CAPTURE_GAIN}, speaker max, sidetone off) on both cards"

# 3) Verify cpal enumerates them.
BIN="${OPENPULSE_BIN:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/target/release/openpulse}"
if [[ -x "$BIN" ]]; then
    if "$BIN" --backend cpal devices 2>/dev/null | grep -q '^hwloop_tx' \
    && "$BIN" --backend cpal devices 2>/dev/null | grep -q '^hwloop_rx'; then
        echo "==> OK: hwloop_tx / hwloop_rx are enumerable by the cpal CLI"
    else
        echo "WARN: hwloop PCMs not visible in '$BIN --backend cpal devices'; check ALSA config" >&2
    fi
else
    echo "NOTE: cpal CLI not built yet (build: cargo build --release -p openpulse-cli)"
fi

cat <<EOF

Wiring expected by this setup:
    USB card ${TX_IDX} (${TX_NAME})  line/speaker OUT  --analog cable-->  USB card ${RX_IDX} (${RX_NAME}) mic IN

Next:
    cargo build --release -p openpulse-cli      # cpal build (default features)
    scripts/run-loopback-dualcard.sh --quick    # run the matrix
If nothing decodes, confirm the cable direction with:
    scripts/run-loopback-dualcard.sh --level-check
EOF
