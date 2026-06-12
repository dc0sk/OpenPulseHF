#!/usr/bin/env bash
# Identify loopback USB audio devices by plug/unplug snapshot diff.
#
# Usage:
#   Step 1 — with loopback USB audio dongles UNPLUGGED on both RPis:
#     ./scripts/identify-loopback-audio.sh before
#
#   Step 2 — plug the USB audio dongles into rpi51 and rpi52
#
#   Step 3 — identify and write config:
#     ./scripts/identify-loopback-audio.sh after
#
#   The 'after' step writes docs/config/loopback-audio-devices.sh which
#   sets ISS_DEVICE, ISS_DEVICE_BYPATH, IRS_DEVICE, IRS_DEVICE_BYPATH.
#   Source it in your profile or the loopback script to pin the devices.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

ISS_SSH="${ISS_SSH:-dc0sk@dc0sk-rpi51}"
IRS_SSH="${IRS_SSH:-dc0sk@dc0sk-rpi52}"
SSH_OPTS="${SSH_OPTS:--o BatchMode=yes -o ConnectTimeout=10}"
SNAP_DIR="/tmp/audio-identify"
OUTFILE="docs/config/loopback-audio-devices.sh"

ssh_iss() { # shellcheck disable=SC2086
    ssh ${SSH_OPTS} "${ISS_SSH}" "$@"; }
ssh_irs() { # shellcheck disable=SC2086
    ssh ${SSH_OPTS} "${IRS_SSH}" "$@"; }

# ── Remote snapshot script (runs on each RPi) ─────────────────────────────────
# Outputs one line per ALSA card:
#   CARD <number> <alsa_name> <has_playback> <has_capture> <usb_vid_pid> <by_id> <by_path> <serial>
#
# No set -e: individual card queries must not abort the whole loop if a
# device lacks udev info or /dev/snd/by-id/ does not yet exist (e.g. when
# no USB audio devices are connected and the kernel has not created the dir).
REMOTE_SNAP='
while IFS= read -r line; do
    num=$(printf "%s" "$line" | grep -o "^ *[0-9]*" | tr -d " ")
    [ -z "$num" ] && continue
    name=$(printf "%s" "$line" | sed "s/^ *[0-9]* \[//;s/ *\].*//")
    ctrl="/dev/snd/controlC${num}"
    [ ! -e "$ctrl" ] && continue

    has_pb=no; [ -e "/dev/snd/pcmC${num}D0p" ] && has_pb=yes
    has_cap=no; [ -e "/dev/snd/pcmC${num}D0c" ] && has_cap=yes

    vid_pid=$(udevadm info --query=all -n "$ctrl" 2>/dev/null \
        | awk -F= "/^E: ID_VENDOR_ID/{v=\$2} /^E: ID_MODEL_ID/{m=\$2} END{if(v) print v\":\"m; else print \"none\"}" \
        || echo none)
    [ -z "$vid_pid" ] && vid_pid=none

    by_id=none
    if [ -d /dev/snd/by-id ]; then
        by_id=$(ls -la /dev/snd/by-id/ 2>/dev/null \
            | awk -v n="controlC${num}" "\$NF ~ n {f=\$(NF-2); sub(\".*/\",\"\",f); print f; exit}" \
            || true)
        [ -z "$by_id" ] && by_id=none
    fi

    by_path=none
    if [ -d /dev/snd/by-path ]; then
        by_path=$(ls -la /dev/snd/by-path/ 2>/dev/null \
            | awk -v n="controlC${num}" "\$NF ~ n {f=\$(NF-2); sub(\".*/\",\"\",f); print f; exit}" \
            || true)
        [ -z "$by_path" ] && by_path=none
    fi

    serial=$(udevadm info --query=all -n "$ctrl" 2>/dev/null \
        | awk -F= "/^E: ID_SERIAL_SHORT/{print \$2; exit}" \
        || true)
    [ -z "$serial" ] && serial=none

    printf "CARD %s %s %s %s %s %s %s %s\n" \
        "$num" "$name" "$has_pb" "$has_cap" "$vid_pid" "$by_id" "$by_path" "$serial"
done < /proc/asound/cards
'

# ── Take snapshot on one host ─────────────────────────────────────────────────
take_snapshot() {
    local host="$1" snapfile="$2"
    echo "  [${host}] taking audio device snapshot..."
    # Use 'bash -s' so the multiline script is fed via stdin, avoiding any
    # quoting issues that arise when passing it as a command argument.
    # shellcheck disable=SC2086
    printf '%s\n' "$REMOTE_SNAP" \
        | ssh ${SSH_OPTS} "${host}" 'bash -s' 2>/dev/null \
        > "$snapfile" || true
    local count
    count=$(grep -c '^CARD' "$snapfile" 2>/dev/null || echo 0)
    if [[ "$count" -eq 0 ]]; then
        echo "  [${host}] WARNING: snapshot is empty (SSH failed or no ALSA cards found)" >&2
    else
        echo "  [${host}] ${count} ALSA card(s) recorded → ${snapfile}"
    fi
}

# ── Diff two snapshots, return lines in 'after' but not 'before' ──────────────
# If the before file is empty (SSH failed during 'before'), fall back to
# returning only USB audio cards from 'after' (vid_pid field != "none").
# This correctly identifies USB dongles even when the before baseline is absent.
new_cards() {
    local before="$1" after="$2"
    local before_count
    before_count=$(grep -c '^CARD' "$before" 2>/dev/null || echo 0)

    if [[ "$before_count" -eq 0 ]]; then
        # No before baseline — return only USB audio cards (have a VID:PID).
        # Built-in audio (HDMI, HiFiBerry, etc.) all have vid_pid=none.
        awk '$6 != "none"' "$after" 2>/dev/null
        return
    fi

    # Normal diff: card is new if its name was not in the before snapshot.
    while IFS= read -r line; do
        local name
        name=$(printf '%s' "$line" | awk '{print $3}')
        if ! grep -qF " ${name} " "$before" 2>/dev/null; then
            printf '%s\n' "$line"
        fi
    done < "$after"
}

# ── Print one card's details ──────────────────────────────────────────────────
print_card() {
    local host="$1" line="$2"
    local num name has_pb has_cap vid_pid by_id by_path serial
    read -r _ num name has_pb has_cap vid_pid by_id by_path serial <<< "$line"
    echo "    Card ${num}: ${name}"
    echo "      USB VID:PID  : ${vid_pid}"
    echo "      Serial       : ${serial}"
    echo "      by-id        : ${by_id}"
    echo "      by-path      : ${by_path}"
    echo "      Playback     : ${has_pb}   Capture: ${has_cap}"
    echo "      ALSA device  : plughw:CARD=${name},DEV=0"
}

# ── Write config output file ──────────────────────────────────────────────────
write_config() {
    local iss_name="$1" iss_bypath="$2" iss_byid="$3"
    local irs_name="$4" irs_bypath="$5" irs_byid="$6"
    mkdir -p "$(dirname "$OUTFILE")"
    cat > "$OUTFILE" <<CONF
#!/usr/bin/env bash
# Loopback audio device identifiers.
# Generated by scripts/identify-loopback-audio.sh on $(date -u +%Y-%m-%dT%H:%M:%SZ)
# Source this file or paste into your profile to pin the loopback devices.
#
# Verification: before a loopback test, check that by-path symlinks still exist:
#   ssh ${ISS_SSH} "ls /dev/snd/by-path/ | grep -Fq '${iss_bypath#*by-path/}' && echo ok || echo MISSING"
#   ssh ${IRS_SSH} "ls /dev/snd/by-path/ | grep -Fq '${irs_bypath#*by-path/}' && echo ok || echo MISSING"

# rpi51 (ISS, loopback TX — line-out of USB dongle → cable → rpi52 line-in)
export ISS_DEVICE="plughw:CARD=${iss_name},DEV=0"
export ISS_DEVICE_BYID="${iss_byid}"
export ISS_DEVICE_BYPATH="${iss_bypath}"

# rpi52 (IRS, loopback RX — cable from rpi51 → line-in of USB dongle)
export IRS_DEVICE="plughw:CARD=${irs_name},DEV=0"
export IRS_DEVICE_BYID="${irs_byid}"
export IRS_DEVICE_BYPATH="${irs_bypath}"
CONF
    echo "  Config written to ${OUTFILE}"
}

# ── Main ──────────────────────────────────────────────────────────────────────
ACTION="${1:-}"
if [[ -z "$ACTION" || ("$ACTION" != "before" && "$ACTION" != "after") ]]; then
    echo "Usage: $0 before | after" >&2
    exit 1
fi

mkdir -p "$SNAP_DIR"

SNAP_ISS_BEFORE="${SNAP_DIR}/iss-before.txt"
SNAP_IRS_BEFORE="${SNAP_DIR}/irs-before.txt"
SNAP_ISS_AFTER="${SNAP_DIR}/iss-after.txt"
SNAP_IRS_AFTER="${SNAP_DIR}/irs-after.txt"

if [[ "$ACTION" == "before" ]]; then
    echo "==> Snapshot BEFORE plugging in loopback devices"
    echo "    SSH targets: ISS=${ISS_SSH}  IRS=${IRS_SSH}"
    echo ""
    take_snapshot "$ISS_SSH" "$SNAP_ISS_BEFORE"
    take_snapshot "$IRS_SSH" "$SNAP_IRS_BEFORE"
    echo ""
    echo "Now plug the USB audio dongles into both RPis, then run:"
    echo "  $0 after"
    exit 0
fi

# ── 'after' phase ─────────────────────────────────────────────────────────────
echo "==> Snapshot AFTER plugging in loopback devices"
echo ""

for f in "$SNAP_ISS_BEFORE" "$SNAP_IRS_BEFORE"; do
    if [[ ! -f "$f" ]]; then
        echo "ERROR: before-snapshot not found: ${f}" >&2
        echo "  Run '$0 before' first (with devices unplugged)." >&2
        exit 1
    fi
done

take_snapshot "$ISS_SSH" "$SNAP_ISS_AFTER"
take_snapshot "$IRS_SSH" "$SNAP_IRS_AFTER"
echo ""

echo "==> New devices on ISS (${ISS_SSH}):"
iss_new="$(new_cards "$SNAP_ISS_BEFORE" "$SNAP_ISS_AFTER")"
if [[ -z "$iss_new" ]]; then
    echo "  (none — no new ALSA cards detected)"
else
    while IFS= read -r line; do print_card "$ISS_SSH" "$line"; done <<< "$iss_new"
fi

echo ""
echo "==> New devices on IRS (${IRS_SSH}):"
irs_new="$(new_cards "$SNAP_IRS_BEFORE" "$SNAP_IRS_AFTER")"
if [[ -z "$irs_new" ]]; then
    echo "  (none — no new ALSA cards detected)"
else
    while IFS= read -r line; do print_card "$IRS_SSH" "$line"; done <<< "$irs_new"
fi
echo ""

# ── Config generation ─────────────────────────────────────────────────────────
iss_count=$(printf '%s\n' "$iss_new" | grep -c '^CARD' 2>/dev/null || echo 0)
irs_count=$(printf '%s\n' "$irs_new" | grep -c '^CARD' 2>/dev/null || echo 0)

if [[ "$iss_count" -eq 0 || "$irs_count" -eq 0 ]]; then
    echo "WARN: could not auto-generate config (expected exactly 1 new card per side)."
    echo "      ISS new cards: ${iss_count}   IRS new cards: ${irs_count}"
    if [[ "$iss_count" -gt 1 ]]; then
        echo ""
        echo "Multiple new ISS cards — plug in ONE device at a time and re-run."
    fi
    exit 0
fi

if [[ "$iss_count" -gt 1 ]]; then
    echo "WARN: ${iss_count} new ISS cards. Plug in ONE device at a time for unambiguous identification."
    exit 1
fi
if [[ "$irs_count" -gt 1 ]]; then
    echo "WARN: ${irs_count} new IRS cards. Plug in ONE device at a time."
    exit 1
fi

read -r _ iss_num iss_name iss_pb iss_cap iss_vid iss_byid iss_bypath iss_serial <<< "$iss_new"
read -r _ irs_num irs_name irs_pb irs_cap irs_vid irs_byid irs_bypath irs_serial <<< "$irs_new"

# Warn if ISS card has no playback or IRS card has no capture.
if [[ "$iss_pb" != "yes" ]]; then
    echo "WARN: ISS card ${iss_name} has no playback device — is the cable in the right jack?"
fi
if [[ "$irs_cap" != "yes" ]]; then
    echo "WARN: IRS card ${irs_name} has no capture device — is the cable in the right jack?"
fi

echo "==> Writing config: ${OUTFILE}"
write_config "$iss_name" "$iss_bypath" "$iss_byid" \
             "$irs_name" "$irs_bypath" "$irs_byid"

echo ""
echo "==> Quick cable sanity check (plays 1 kHz tone, measures level on IRS):"
echo "    Expected: mean_sq >> 0.001 with cable connected."
echo "    Running..."

# Max out ISS playback volume
ssh_iss "amixer -c '${iss_name}' set 'PCM Playback Volume' 100% 2>/dev/null || \
         amixer -c '${iss_name}' set 'PCM Playback Volume' -- -0dB 2>/dev/null || true" 2>/dev/null || true

# Capture on IRS while playing tone on ISS
ssh_irs "arecord -D 'plughw:CARD=${irs_name},DEV=0' -f S16_LE -r 8000 -c 1 \
    -d 4 /tmp/cable-sanity.raw 2>/dev/null" &
CAP_PID=$!
sleep 0.5

python3 -c "
import math, struct, sys
sr=8000; dur=3; f=1000; amp=0.5
s=[int(amp*32767*math.sin(2*math.pi*f*i/sr)) for i in range(sr*dur)]
sys.stdout.buffer.write(struct.pack('<'+'h'*len(s),*s))
" | ssh_iss "aplay -D 'plughw:CARD=${iss_name},DEV=0' -f S16_LE -r 8000 -c 1 2>/dev/null"

wait "$CAP_PID" || true
sleep 0.3

SANITY=$(ssh_irs "python3 -c \"
import struct, math
try:
    d = open('/tmp/cable-sanity.raw','rb').read()
    s = [x for (x,) in struct.iter_unpack('<h', d)]
    msq = sum(x*x for x in s)/len(s)/32768**2
    status = 'PASS' if msq > 0.001 else 'FAIL (noise floor only)'
    print(f'{status}  mean_sq={msq:.6f}  rms={math.sqrt(msq):.4f}')
except Exception as e:
    print(f'FAIL ({e})')
\"" 2>/dev/null || echo "FAIL (SSH error)")

echo "    Cable test: ${SANITY}"
echo ""
echo "==> Done. Source the config in your loopback profile:"
echo "    source ${OUTFILE}"
echo "    ./scripts/run-loopback-rpi51-rpi52.sh --quick"
