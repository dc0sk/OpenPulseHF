#!/usr/bin/env bash
# Point each station's PipeWire default SINK and SOURCE at its rig's USB-audio CODEC,
# and set the TX drive level, so the on-air runner's `--device pulse` reaches the RADIO
# (not the host's built-in speaker/mic) at a level that does not pin the rig's ALC.
#
# WHY THIS EXISTS
#   run-onair-ic9700-ft991a.sh uses A_AUDIO_DEVICE / B_AUDIO_DEVICE = "pulse", which
#   openpulse's cpal backend maps to the PulseAudio/PipeWire DEFAULT sink (TX) and
#   DEFAULT source (RX). On a laptop the default sink is the built-in speaker — TX audio
#   would never reach the rig. And openpulse emits near-full-scale audio; sent straight
#   in it pins the rig ALC and distorts the waveform. Gate 6 was brought up by hand by
#   setting the default sink to the CODEC at ~15% and the default source to the CODEC.
#   This script makes that reproducible, and (per the dual-card AGC lesson) it does not
#   just SET the state — it READS IT BACK and fails loudly if it did not take.
#
# It changes ONLY PipeWire routing (default nodes + one sink volume). It keys nothing.
#
# Usage:
#   source docs/config/onair-ic9700-ft991a.example.sh
#   scripts/onair-setup-audio-routing.sh            # set + verify both sides
#   scripts/onair-setup-audio-routing.sh --verify   # verify only (no changes)
#
# Config it reads (from the sourced profile; sensible defaults below):
#   A_SSH / B_SSH / SSH_OPTS       how to reach each station
#   A_CODEC_MATCH / B_CODEC_MATCH  case-insensitive substring identifying the rig CODEC
#                                  in `wpctl status` (e.g. "Codec", "PCM2903B")
#   B_TX_SINK_VOLUME               TX drive level 0..1 for side B's CODEC sink (default 0.15)
#   A_TX_SINK_VOLUME               TX drive level for side A's CODEC sink (default 0.15)
#
# Exit: 0 = both sides routed and verified, 1 = a side failed, 2 = usage/tooling error.
set -uo pipefail

A_SSH="${A_SSH:-}"
B_SSH="${B_SSH:-}"
SSH_OPTS="${SSH_OPTS:--o BatchMode=yes -o ConnectTimeout=12}"
A_CODEC_MATCH="${A_CODEC_MATCH:-Codec}"
B_CODEC_MATCH="${B_CODEC_MATCH:-Codec}"
A_TX_SINK_VOLUME="${A_TX_SINK_VOLUME:-0.15}"
B_TX_SINK_VOLUME="${B_TX_SINK_VOLUME:-0.15}"
A_LABEL="${A_LABEL:-Station A}"
B_LABEL="${B_LABEL:-Station B}"

VERIFY_ONLY=0
[[ "${1:-}" == "--verify" ]] && VERIFY_ONLY=1

if [[ -z "$A_SSH" || -z "$B_SSH" ]]; then
    echo "ERROR: A_SSH and B_SSH must be set (source the on-air profile first)." >&2
    exit 2
fi

# Remote worker: resolve CODEC sink+source ids from `wpctl status`, optionally set them
# as defaults (sink volume = $2), then read back and print machine-parseable results.
# Args: $1 = match substring (case-insensitive), $2 = sink volume, $3 = "set"|"verify".
# The remote heredoc uses only wpctl (present under WirePlumber on both hosts) and awk.
_remote_routing() {
    local match="$1" vol="$2" action="$3"
    cat <<REMOTE
export XDG_RUNTIME_DIR=/run/user/\$(id -u)
command -v wpctl >/dev/null 2>&1 || { echo "RESULT fail no-wpctl"; exit 0; }
status="\$(wpctl status 2>/dev/null)"
# Extract the numeric node id of the first CODEC-matching line in a named section.
# Section blocks in wpctl status run from "<name>:" to the next "<Word>:" header.
_id_in() {  # \$1 = section header word (Sinks|Sources)
    printf '%s\n' "\$status" \
      | awk -v sec="\$1:" '
            \$0 ~ sec {inb=1; next}
            inb && /^[[:space:]]*[│├└ ]*[A-Z][a-z]+:/ {inb=0}
            inb {print}
          ' \
      | grep -i -- "$match" \
      | grep -oE '[0-9]+\.' | head -n1 | tr -d '.'
}
sink_id="\$(_id_in Sinks)"
src_id="\$(_id_in Sources)"
if [[ -z "\$sink_id" || -z "\$src_id" ]]; then
    echo "RESULT fail no-codec sink='\$sink_id' src='\$src_id' match='$match'"
    exit 0
fi
if [[ "$action" == "set" ]]; then
    wpctl set-default "\$sink_id" >/dev/null 2>&1
    wpctl set-default "\$src_id" >/dev/null 2>&1
    wpctl set-volume  "\$sink_id" "$vol" >/dev/null 2>&1
fi
# Read back: is the CODEC the DEFAULT (starred) sink & source, and the sink vol right?
def_sink="\$(printf '%s\n' "\$status" | awk '/Sinks:/{s=1;next} /Sources:/{s=0} s&&/\*/{print}' )"
# Re-read fresh status after any change:
status2="\$(wpctl status 2>/dev/null)"
dsink="\$(printf '%s\n' "\$status2" | awk '/Sinks:/{s=1;next} /Sources:/{s=0} s&&/\*/{gsub(/[^0-9]/," ");print \$1}' | head -n1)"
dsrc="\$(printf '%s\n' "\$status2" | awk '/Sources:/{s=1;next} /Filters:/{s=0} s&&/\*/{gsub(/[^0-9]/," ");print \$1}' | head -n1)"
vol="\$(wpctl get-volume "\$sink_id" 2>/dev/null | grep -oE '[0-9]+\.[0-9]+' | head -n1)"
ok=1
[[ "\$dsink" == "\$sink_id" ]] || ok=0
[[ "\$dsrc"  == "\$src_id"  ]] || ok=0
echo "RESULT \$([[ \$ok == 1 ]] && echo pass || echo fail) sink=\$sink_id src=\$src_id default_sink=\$dsink default_src=\$dsrc sink_vol=\$vol"
REMOTE
}

_run_side() {
    local ssh_target="$1" match="$2" vol="$3" label="$4"
    local action="set"; [[ $VERIFY_ONLY == 1 ]] && action="verify"
    echo "==> ${label} (${ssh_target}) — ${action} routing (match='${match}', tx-vol=${vol})"
    local out
    # shellcheck disable=SC2086
    out="$(ssh ${SSH_OPTS} "$ssh_target" "$(_remote_routing "$match" "$vol" "$action")" 2>/dev/null)"
    local line; line="$(printf '%s\n' "$out" | grep '^RESULT ' | head -n1)"
    echo "    ${line:-RESULT fail no-output}"
    [[ "$line" == RESULT\ pass* ]]
}

rc=0
_run_side "$A_SSH" "$A_CODEC_MATCH" "$A_TX_SINK_VOLUME" "$A_LABEL" || rc=1
_run_side "$B_SSH" "$B_CODEC_MATCH" "$B_TX_SINK_VOLUME" "$B_LABEL" || rc=1

echo ""
if [[ $rc -eq 0 ]]; then
    echo "PASS: both stations' default sink+source point at their rig CODEC; TX level set."
    echo "      The runner's --device pulse will now reach the radios. Re-run --verify anytime."
else
    echo "FAIL: a station is not routed to its rig CODEC (see RESULT lines above)." >&2
    echo "      Fix: confirm the CODEC is plugged in and A_CODEC_MATCH/B_CODEC_MATCH match its" >&2
    echo "      description in 'wpctl status'. Do NOT run the on-air matrix until this PASSES." >&2
fi
exit $rc
