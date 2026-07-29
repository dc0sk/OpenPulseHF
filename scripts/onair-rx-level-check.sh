#!/usr/bin/env bash
# Check that each station's RX capture level lets the modem's energy gate DISCRIMINATE.
#
# THE RULE (measured on air 2026-07-28, and the cause of a full A1 failure)
#   openpulse's EnergyGate (crates/openpulse-modem/src/engine.rs) picks its threshold as
#       threshold = clamp(idle_floor * 3, ABS_THRESHOLD=0.0001, MAX_THRESHOLD=0.0032)
#   and only hands audio to the demodulator when a window's mean-square clears it. If the
#   IDLE noise floor itself is above MAX_THRESHOLD, the threshold clamps BELOW the noise and
#   the gate can never shut: it fires continuously on noise, the receiver settles AFC on that
#   noise, and the stale bogus correction wrecks the real frame that arrives seconds later —
#   "invalid magic" at every retry, even with the two rigs aligned to ~1 Hz.
#
#   Measured: IC-9700 at source volume 1.00 -> idle mean_sq 0.0154 (4.7x over the clamp),
#   BPSK250 64B on air FAILED. At 0.55 -> idle 0.00042, signal 0.0024, threshold ~0.0013
#   between them: the SAME case PASSED. The FT-991A needed no change (idle 0.000125 at 1.00)
#   because its USB AF output is simply quieter. So this is a PER-RIG measurement, never a
#   fixed volume to copy.
#
# PASS means: idle * 3 <= MAX_THRESHOLD, i.e. idle <= 0.00107 — the adaptive threshold is
# still free to sit above the noise. It also reports whether the threshold would sit on the
# absolute floor (idle very quiet), where the signal must clear 0.0001 instead.
#
# WHAT THIS CHECK DOES NOT COVER, and no longer needs to. It only bounds the idle floor from
# ABOVE. A floor between ABS_THRESHOLD (1e-4) and that 1.07e-3 ceiling passes here, and used
# to be a decode gate anyway: EnergyGate returned the fixed 1e-4 until it held 32 windows of
# history, so any floor above 1e-4 made the FIRST window of pure noise pass, and the receiver
# settled AFC on noise before the frame arrived. This station measures 4.1e-4 — inside that
# blind window — and that is what sank #1021 twice. The gate now derives its threshold from
# the first window it sees, so the blind window is closed in code; this script's ceiling check
# remains necessary and sufficient.
#
# This captures IDLE audio only. Run it with NO transmission anywhere. It keys nothing.
#
# Usage:
#   source docs/config/onair-ic9700-ft991a.example.sh
#   scripts/onair-rx-level-check.sh
#
# Env: A_SSH/B_SSH/SSH_OPTS, A_CODEC_MATCH/B_CODEC_MATCH, DURATION (default 5).
# Exit: 0 = both sides can discriminate, 1 = a side cannot, 2 = usage/tooling error.
set -uo pipefail

A_SSH="${A_SSH:-}"
B_SSH="${B_SSH:-}"
SSH_OPTS="${SSH_OPTS:--o BatchMode=yes -o ConnectTimeout=12}"
A_LABEL="${A_LABEL:-Station A}"
B_LABEL="${B_LABEL:-Station B}"
DURATION="${DURATION:-5}"

# Mirror of the engine constants — keep in sync with engine.rs EnergyGate.
MAX_THRESHOLD=0.0032
ABS_THRESHOLD=0.0001
IDLE_MAX=0.00107   # MAX_THRESHOLD / 3

if [[ -z "$A_SSH" || -z "$B_SSH" ]]; then
    echo "ERROR: A_SSH and B_SSH must be set (source the on-air profile first)." >&2
    exit 2
fi

# Remote: capture idle audio from the rig CODEC and print "MEANSQ <value>".
# Handles both tool families — rpi hosts have parecord, the laptop only has pw-record.
_remote_level() {
    cat <<'REMOTE'
export XDG_RUNTIME_DIR=/run/user/$(id -u)
OUT=/tmp/onair-rxlevel.wav
rm -f "$OUT"
if command -v parecord >/dev/null 2>&1; then
    SRC="$(pactl list sources short 2>/dev/null | grep -i alsa_input | grep -i codec | awk '{print $2}' | head -n1)"
    [ -n "$SRC" ] && timeout __DUR__ parecord --device="$SRC" --channels=2 --rate=48000 \
        --format=s16le --file-format=wav "$OUT" >/dev/null 2>&1
fi
if [ ! -s "$OUT" ] && command -v pw-record >/dev/null 2>&1; then
    SID="$(wpctl status 2>/dev/null | awk '/Sources:/{s=1;next} /Filters:/{s=0} s' \
           | grep -i -- 'codec' | grep -oE '[0-9]+\.' | head -n1 | tr -d '.')"
    [ -n "$SID" ] && timeout --signal=INT __DUR__ pw-record --target="$SID" --channels=2 \
        --rate=48000 --format=s16 "$OUT" >/dev/null 2>&1
fi
python3 -c "
import wave, struct, sys
try:
    w = wave.open('$OUT','rb'); n = w.getnframes(); ch = w.getnchannels()
    raw = w.readframes(n); s = struct.unpack('<%dh' % (len(raw)//2), raw)
    L = s[0::ch]
    if len(L) < 1000: print('MEANSQ none'); sys.exit(0)
    print('MEANSQ %.8f' % (sum(x*x for x in L)/len(L)/32768**2))
except Exception:
    print('MEANSQ none')
"
REMOTE
}

_check_side() {
    local ssh_target="$1" label="$2"
    local script; script="$(_remote_level | sed "s/__DUR__/${DURATION}/g")"
    local out meansq
    # shellcheck disable=SC2086
    out="$(ssh ${SSH_OPTS} "$ssh_target" "$script" 2>/dev/null)"
    meansq="$(printf '%s\n' "$out" | sed -n 's/^MEANSQ //p' | head -n1)"
    if [[ -z "$meansq" || "$meansq" == "none" ]]; then
        echo "  ${label}: FAIL — could not capture idle audio from the rig CODEC" >&2
        return 1
    fi
    awk -v m="$meansq" -v lim="$IDLE_MAX" -v maxt="$MAX_THRESHOLD" -v abst="$ABS_THRESHOLD" -v lbl="$label" '
    BEGIN {
        thr = m * 3.0
        clamped = 0
        if (thr > maxt) { thr = maxt; clamped = 1 }
        if (thr < abst) { thr = abst; floored = 1 }
        printf "  %s: idle mean_sq=%.6f  gate threshold=%.6f", lbl, m, thr
        if (clamped) {
            printf "  CLAMPED\n"
            printf "    FAIL: idle is above %.5f, so the threshold clamps BELOW the noise —\n", lim
            printf "          the gate can never shut and will settle AFC on noise. Lower the RX\n"
            printf "          capture level (PipeWire source volume is cubic: power ~ v^6).\n"
            exit 1
        }
        if (floored)
            printf "  (on the absolute floor — signal must clear %.4f)\n", abst
        else
            printf "  OK\n"
        exit 0
    }' || return 1
    return 0
}

echo "==> RX capture-level check (idle; no TX anywhere). Gate must be able to discriminate."
rc=0
_check_side "$A_SSH" "$A_LABEL" || rc=1
_check_side "$B_SSH" "$B_LABEL" || rc=1

echo ""
if [[ $rc -eq 0 ]]; then
    echo "PASS: both stations' idle floor leaves the energy gate free to discriminate."
else
    echo "FAIL: a station's RX level prevents the energy gate from discriminating." >&2
    echo "      Set the per-side RX level with scripts/onair-setup-audio-routing.sh" >&2
    echo "      (A_RX_SOURCE_VOLUME / B_RX_SOURCE_VOLUME) and re-run this check." >&2
fi
exit $rc
