#!/usr/bin/env bash
# Record a rig's receive audio and prepare it for the replay corpus.
#
# WHY: the emulated impairments in the test harness are MODELS of a radio, and a model can be wrong
# in exactly the way that hides a bug. A recorded capture cannot be — it is what the rig actually
# produced. `crates/openpulse-modem/tests/captures/` is the corpus; its README lists provenance.
#
# The corpus currently has idle floors and a received tone. The highest-value slot is still EMPTY:
# a real modem FRAME, which would let the suite assert an end-to-end decode against real audio.
# Capture one by running this while the far station transmits a known payload.
#
# Usage:
#   scripts/onair-record-capture.sh <name> [seconds]
#   scripts/onair-record-capture.sh frame-bpsk250-rs 25
#
# Env:
#   SSH        run the capture on a remote host (e.g. SSH="dc0sk@dc0sk-rpi51"); local if unset
#   DEVICE     PulseAudio source name; auto-detected from the rig CODEC if unset
#   OUT_DIR    where to write the prepared 8 kHz mono file (default: the corpus directory)
#
# Records at the card's native 48 kHz, then decimates to the modem's 8 kHz with a proper
# anti-aliasing filter. Naive decimation would ALIAS the noise floor and quietly destroy the very
# property a capture exists to preserve.
#
# Exit: 0 = capture prepared, 1 = capture failed, 2 = usage/tooling error.
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
NAME="${1:-}"
SECONDS_TO_RECORD="${2:-10}"
OUT_DIR="${OUT_DIR:-${REPO_ROOT}/crates/openpulse-modem/tests/captures}"
SSH_TARGET="${SSH:-}"

if [[ -z "$NAME" ]]; then
    echo "usage: $0 <name> [seconds]   (name becomes <name>.wav in the corpus)" >&2
    exit 2
fi
command -v python3 >/dev/null 2>&1 || { echo "ERROR: python3 required" >&2; exit 2; }
python3 -c 'import scipy, numpy' 2>/dev/null || {
    echo "ERROR: python3 with numpy and scipy required (for anti-aliased decimation)" >&2
    exit 2
}

RAW_REMOTE="/tmp/onair-capture-${NAME}-48k.wav"
RAW_LOCAL="/tmp/onair-capture-${NAME}-48k.wav"

# Capture at the card's native rate. parecord where available (it resumes a suspended node);
# pw-record as the fallback on hosts without the PulseAudio tools.
read -r -d '' REMOTE_CMD <<REMOTE
export XDG_RUNTIME_DIR=/run/user/\$(id -u)
DEV="${DEVICE:-}"
if command -v parecord >/dev/null 2>&1; then
    [ -n "\$DEV" ] || DEV="\$(pactl list sources short 2>/dev/null | grep -i alsa_input | grep -i codec | awk '{print \$2}' | head -n1)"
    [ -n "\$DEV" ] || { echo "NO_DEVICE"; exit 1; }
    timeout ${SECONDS_TO_RECORD} parecord --device="\$DEV" --channels=2 --rate=48000 \
        --format=s16le --file-format=wav "${RAW_REMOTE}" >/dev/null 2>&1
elif command -v pw-record >/dev/null 2>&1; then
    SID="\$(wpctl status 2>/dev/null | awk '/Sources:/{s=1;next} /Filters:/{s=0} s' | grep -i codec | grep -oE '[0-9]+\.' | head -n1 | tr -d '.')"
    [ -n "\$SID" ] || { echo "NO_DEVICE"; exit 1; }
    timeout --signal=INT ${SECONDS_TO_RECORD} pw-record --target="\$SID" --channels=2 \
        --rate=48000 --format=s16 "${RAW_REMOTE}" >/dev/null 2>&1
else
    echo "NO_TOOL"; exit 1
fi
[ -s "${RAW_REMOTE}" ] && echo "OK \$(stat -c%s "${RAW_REMOTE}")" || echo "EMPTY"
REMOTE

echo "==> Recording ${SECONDS_TO_RECORD}s at 48 kHz${SSH_TARGET:+ on ${SSH_TARGET}} ..."
if [[ -n "$SSH_TARGET" ]]; then
    # shellcheck disable=SC2086
    result="$(ssh -o BatchMode=yes -o ConnectTimeout=12 "$SSH_TARGET" "$REMOTE_CMD" 2>/dev/null | tail -n1)"
else
    result="$(bash -c "$REMOTE_CMD" 2>/dev/null | tail -n1)"
fi

case "$result" in
    OK*) echo "    captured (${result#OK })" ;;
    NO_DEVICE) echo "ERROR: no rig CODEC source found; set DEVICE explicitly" >&2; exit 1 ;;
    NO_TOOL)   echo "ERROR: neither parecord nor pw-record available on the capture host" >&2; exit 1 ;;
    *)         echo "ERROR: capture produced no audio (result: '${result:-none}')." >&2
               echo "       A zero-length capture usually means the rig is not outputting audio —" >&2
               echo "       check AF gain and squelch on the radio." >&2
               exit 1 ;;
esac

if [[ -n "$SSH_TARGET" ]]; then
    scp -q -o BatchMode=yes "${SSH_TARGET}:${RAW_REMOTE}" "$RAW_LOCAL" || {
        echo "ERROR: could not copy the capture back" >&2; exit 1; }
fi

mkdir -p "$OUT_DIR"
echo "==> Decimating 48 kHz -> 8 kHz mono (anti-aliased) ..."
python3 - "$RAW_LOCAL" "${OUT_DIR}/${NAME}.wav" <<'PY'
import sys, wave, struct
import numpy as np
from scipy.signal import resample_poly

src, dst = sys.argv[1], sys.argv[2]
w = wave.open(src, "rb")
n, ch, fs = w.getnframes(), w.getnchannels(), w.getframerate()
raw = w.readframes(n); w.close()
x = np.frombuffer(raw, dtype="<i2").astype(np.float64)[0::ch] / 32768.0
if fs % 8000:
    raise SystemExit(f"capture rate {fs} is not a multiple of 8000; re-record at 48 kHz")
y = resample_poly(x, 1, fs // 8000)
o = wave.open(dst, "wb"); o.setnchannels(1); o.setsampwidth(2); o.setframerate(8000)
o.writeframes(struct.pack("<%dh" % len(y), *np.clip(y * 32768, -32768, 32767).astype(int)))
o.close()
print(f"    {dst}: {len(y)/8000:.1f}s  mean_sq={float(np.mean(y**2)):.6f}  peak={float(np.max(np.abs(y))):.3f}")
PY
rc=$?

echo ""
if [[ $rc -eq 0 ]]; then
    echo "Prepared ${OUT_DIR}/${NAME}.wav"
    echo "NEXT: add a row to the corpus README recording PROVENANCE (rig, frequency, mode, level"
    echo "      settings, date) and the MEASURED property this file is meant to preserve. A capture"
    echo "      with no recorded expectation cannot be asserted against and will rot."
else
    echo "ERROR: decimation failed" >&2
fi
exit $rc
