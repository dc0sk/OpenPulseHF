#!/usr/bin/env bash
# Capture ONE on-air modem transmission two ways at once: the SDR (independent, off-air) and the
# receiving rig's own USB audio (the chain the modem actually decodes).
#
# WHY BOTH. When the coded rungs failed on air (issue #1021) there was no way to tell
# "the transmitter emitted something wrong" from "the receive chain mangled it" — the only evidence
# was AFC log lines, and two plausible hypotheses (rig frequency drift, then capture level) were
# both wrong. Two simultaneous captures make that a direct observation instead of an inference:
#
#   SDR shows a clean frame + rig audio decodes    -> working
#   SDR shows a clean frame + rig audio does NOT   -> RECEIVE CHAIN (the #1021 class, with evidence)
#   SDR shows nothing/garbage                      -> transmit side
#
# The SDR alone cannot replace the rig capture: it bypasses the rig's USB CODEC, its AGC and the
# host mixer, which is exactly where every capture-context defect this project has hit actually
# lives. The rig capture alone cannot replace the SDR: it cannot prove what was on the air.
#
# It also fills both slots of the replay corpus in one keyed run
# (crates/openpulse-modem/tests/captures/ — see its README).
#
# ⚠ THIS KEYS A TRANSMITTER. It refuses to key outside the configured band, verifies PTT release,
# and releases PTT on any exit path. Run it only in an agreed on-air window.
#
# Usage:
#   source docs/config/onair-ic9700-ft991a.example.sh
#   scripts/onair-dual-capture.sh <name> [mode] [fec] [payload]
#   scripts/onair-dual-capture.sh frame-bpsk250-rs BPSK250 rs 'DUALCAP TEST 1'
#
# Config (from the on-air profile, plus SDR knobs):
#   TX_SSH/TX_RIGCTLD_*   station that transmits   (default: Station B / the FT-991A side)
#   RX_SSH                station whose USB audio is captured (default: Station A / IC-9700)
#   TEST_FREQ_HZ, BAND2M_MIN_HZ, BAND2M_MAX_HZ   the band guard
#   OPHF_SDR_ANT          RSPdx antenna: "Antenna A"/"B" for VHF ("C" is the HF-optimised BNC)
#   SDR_RFGR              RF gain REDUCTION 0..27 on the RSPdx (higher = more attenuation).
#                         NOTE the RSP2 range was 0..8 — do not copy old numbers across.
#   SDR_IFGR              IF gain reduction 20..59 (20 = most IF gain)
#   SDR_FS                SDR sample rate, Hz (default 192000)
#   OUT_DIR               where captures land (default docs/dev/test-reports/on-air/captures)
#
# Exit: 0 = both captures produced audio, 1 = a capture failed, 2 = usage/tooling/safety error.
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

NAME="${1:-}"
MODE="${2:-BPSK250}"
FEC="${3:-rs}"
PAYLOAD="${4:-OPENPULSE DUAL CAPTURE}"

if [[ -z "$NAME" ]]; then
    echo "usage: $0 <name> [mode] [fec] [payload]" >&2
    exit 2
fi

# Default roles: side B transmits, side A receives — the direction the on-air matrix runs in
# reverse mode, and the one where the coded failure was observed.
TX_SSH="${TX_SSH:-${B_SSH:-}}"
RX_SSH="${RX_SSH:-${A_SSH:-}}"
TX_RIGCTLD_ADDR="${TX_RIGCTLD_ADDR:-${B_RIGCTLD_ADDR:-127.0.0.1}}"
TX_RIGCTLD_PORT="${TX_RIGCTLD_PORT:-${B_RIGCTLD_PORT:-4532}}"
TX_REPO_DIR="${TX_REPO_DIR:-${B_REPO_DIR:-\$HOME/openpulse/OpenPulseHF}}"
TEST_FREQ_HZ="${TEST_FREQ_HZ:-}"
BAND_MIN="${BAND2M_MIN_HZ:-144500000}"
BAND_MAX="${BAND2M_MAX_HZ:-144750000}"
SSH_OPTS="${SSH_OPTS:--o BatchMode=yes -o ConnectTimeout=12}"

SDR_FS="${SDR_FS:-192000}"
SDR_RFGR="${SDR_RFGR:-12}"     # RSPdx 0..27; a nearby transmitter saturates a wide-open front end
SDR_IFGR="${SDR_IFGR:-40}"
SDR_ANT="${OPHF_SDR_ANT:-Antenna A}"
OUT_DIR="${OUT_DIR:-${REPO_ROOT}/docs/dev/test-reports/on-air/captures}"

if [[ -z "$TX_SSH" || -z "$RX_SSH" ]]; then
    echo "ERROR: TX_SSH and RX_SSH must resolve (source the on-air profile first)." >&2
    exit 2
fi
command -v python3 >/dev/null 2>&1 || { echo "ERROR: python3 required" >&2; exit 2; }

_ssh() { local t="$1"; shift; ssh $SSH_OPTS "$t" "$@"; }
_rig() { echo "rigctl -m 2 -r ${TX_RIGCTLD_ADDR}:${TX_RIGCTLD_PORT}"; }

# ── Safety preflight ─────────────────────────────────────────────────────────
echo "==> Preflight"
tx_freq="$(_ssh "$TX_SSH" "$(_rig) f 2>/dev/null | grep -v Hamlib | grep -E '^[0-9]+$' | head -n1" || true)"
if [[ -z "$tx_freq" ]]; then
    echo "ERROR: could not read the transmitting rig's frequency — is rigctld running on ${TX_SSH}?" >&2
    exit 2
fi
if (( tx_freq < BAND_MIN || tx_freq > BAND_MAX )); then
    echo "ERROR: TX rig is on ${tx_freq} Hz, outside the allowed ${BAND_MIN}-${BAND_MAX}. Not keying." >&2
    exit 2
fi
tx_ptt="$(_ssh "$TX_SSH" "$(_rig) t 2>/dev/null | grep -v Hamlib | grep -E '^[01]$' | head -n1" || true)"
if [[ "$tx_ptt" != "0" ]]; then
    echo "ERROR: TX rig PTT reads '${tx_ptt}' before we start. Refusing to key on top of it." >&2
    exit 2
fi
echo "    TX ${TX_SSH} on ${tx_freq} Hz, PTT released"

SDR_CENTER="${SDR_CENTER:-$tx_freq}"
if ! python3 -c "import SoapySDR" 2>/dev/null; then
    echo "ERROR: SoapySDR python bindings not available on this host" >&2
    exit 2
fi
sdr_hw="$(python3 -c "
import SoapySDR
try:
    d=SoapySDR.Device(); print(d.getHardwareKey())
except Exception as e: print('NONE')
" 2>/dev/null | tail -n1)"
if [[ "$sdr_hw" == "NONE" || -z "$sdr_hw" ]]; then
    echo "ERROR: no SDR could be opened on this host" >&2
    exit 2
fi
echo "    SDR ${sdr_hw}, ${SDR_ANT}, centre ${SDR_CENTER} Hz, fs ${SDR_FS}, RFGR ${SDR_RFGR} IFGR ${SDR_IFGR}"

mkdir -p "$OUT_DIR"
IQ_OUT="${OUT_DIR}/${NAME}.cf32"
RIG_RAW_REMOTE="/tmp/dualcap-${NAME}-48k.wav"
RIG_OUT="${OUT_DIR}/${NAME}-rig48k.wav"
TX_LOG="${OUT_DIR}/${NAME}-tx.log"

# The SDR and rig captures must both span the transmission with margin at each end: the frame's
# position inside the capture is exactly what the receiver has to find.
CAP_SECS="${CAP_SECS:-30}"

# ── Release PTT on ANY exit ──────────────────────────────────────────────────
release_ptt() {
    _ssh "$TX_SSH" "$(_rig) T 0 >/dev/null 2>&1 || true" >/dev/null 2>&1 || true
}
trap release_ptt EXIT INT TERM

# ── Antenna present? ─────────────────────────────────────────────────────────
# SWR only reads meaningfully while keyed, so this keys BRIEFLY at the configured (low) power and
# reads the meter. Transmitting into a disconnected antenna is the one failure that damages
# hardware rather than just wasting a run, and it also guarantees a worthless capture. Set
# SKIP_SWR_CHECK=1 only for a known-good dummy load.
SWR_LIMIT="${SWR_LIMIT:-3.0}"
check_antenna() {
    if [[ "${SKIP_SWR_CHECK:-0}" == "1" ]]; then
        echo "    SWR check SKIPPED (SKIP_SWR_CHECK=1)"
        return 0
    fi
    echo "==> Antenna check: brief keyed SWR reading"
    local swr
    swr="$(_ssh "$TX_SSH" "
        $(_rig) T 1 >/dev/null 2>&1
        sleep 0.4
        s=\$($(_rig) l SWR 2>/dev/null | grep -v Hamlib | tail -n1)
        $(_rig) T 0 >/dev/null 2>&1
        echo \"\${s:-na}\"
    " 2>/dev/null | tail -n1)"
    _ssh "$TX_SSH" "$(_rig) T 0 >/dev/null 2>&1 || true" >/dev/null 2>&1 || true

    if [[ -z "$swr" || "$swr" == "na" ]]; then
        echo "    WARNING: the rig does not report SWR over CAT; cannot verify an antenna is" >&2
        echo "             connected. Confirm it manually before proceeding." >&2
        return 0
    fi
    echo "    SWR = ${swr} (limit ${SWR_LIMIT})"
    if awk -v s="$swr" -v l="$SWR_LIMIT" 'BEGIN{exit !(s+0 > l+0)}'; then
        echo "ERROR: SWR ${swr} exceeds ${SWR_LIMIT} — the antenna is probably not connected." >&2
        echo "       Refusing to transmit. Connect the antenna (or set SKIP_SWR_CHECK=1 for a" >&2
        echo "       known-good dummy load) and re-run." >&2
        return 1
    fi
    return 0
}
check_antenna || exit 2

# ── Start both captures ──────────────────────────────────────────────────────
echo "==> Starting SDR capture (${CAP_SECS}s)"
python3 "${REPO_ROOT}/scripts/onair-sdr/sdr_capture.py" \
    "$SDR_CENTER" "$SDR_FS" "$CAP_SECS" "$SDR_RFGR" "$SDR_IFGR" "$IQ_OUT" \
    >"${IQ_OUT}.log" 2>&1 &
SDR_PID=$!

echo "==> Starting rig USB-audio capture on ${RX_SSH} (${CAP_SECS}s)"
read -r -d '' RIG_CMD <<REMOTE
export XDG_RUNTIME_DIR=/run/user/\$(id -u)
rm -f "${RIG_RAW_REMOTE}"
if command -v parecord >/dev/null 2>&1; then
    DEV="\$(pactl list sources short 2>/dev/null | grep -i alsa_input | grep -i codec | awk '{print \$2}' | head -n1)"
    [ -n "\$DEV" ] && timeout ${CAP_SECS} parecord --device="\$DEV" --channels=2 --rate=48000 \
        --format=s16le --file-format=wav "${RIG_RAW_REMOTE}" >/dev/null 2>&1
elif command -v pw-record >/dev/null 2>&1; then
    SID="\$(wpctl status 2>/dev/null | awk '/Sources:/{s=1;next} /Filters:/{s=0} s' | grep -i codec | grep -oE '[0-9]+\.' | head -n1 | tr -d '.')"
    [ -n "\$SID" ] && timeout --signal=INT ${CAP_SECS} pw-record --target="\$SID" --channels=2 \
        --rate=48000 --format=s16 "${RIG_RAW_REMOTE}" >/dev/null 2>&1
fi
[ -s "${RIG_RAW_REMOTE}" ] && echo "OK" || echo "EMPTY"
REMOTE
( _ssh "$RX_SSH" "$RIG_CMD" >"${RIG_OUT}.status" 2>/dev/null ) &
RIG_PID=$!

# Let both captures settle so the frame lands well inside them, not at an edge.
sleep 4

# ── Transmit one frame ───────────────────────────────────────────────────────
echo "==> Transmitting: mode=${MODE} fec=${FEC} payload='${PAYLOAD}'"
fec_arg="${FEC//_/-}"
_ssh "$TX_SSH" "
    B=\"${TX_REPO_DIR}/target/release/openpulse\"
    export XDG_RUNTIME_DIR=/run/user/\$(id -u)
    \"\$B\" --backend cpal --log info --ptt rigctld --rig ${TX_RIGCTLD_ADDR}:${TX_RIGCTLD_PORT} \
        transmit --mode '${MODE}' --fec '${fec_arg}' --device pulse --max-power 5 '${PAYLOAD}'
    sleep 0.5
    echo -n 'PTT after TX: '; $(_rig) t 2>/dev/null | grep -v Hamlib | grep -E '^[01]\$' | head -n1
" >"$TX_LOG" 2>&1
tx_rc=$?
grep -E 'Transmitted|PTT after TX' "$TX_LOG" | sed 's/^/    /'

echo "==> Waiting for captures to finish"
wait "$SDR_PID" 2>/dev/null; sdr_rc=$?
wait "$RIG_PID" 2>/dev/null || true

# ── Collect + report ─────────────────────────────────────────────────────────
rc=0
echo ""
echo "==> Results"

if [[ -s "$IQ_OUT" ]]; then
    echo "    SDR IQ : $IQ_OUT ($(stat -c%s "$IQ_OUT") bytes)"
    grep -E 'cfg:|captured' "${IQ_OUT}.log" 2>/dev/null | sed 's/^/             /'
else
    echo "    SDR IQ : FAILED (exit ${sdr_rc}) — see ${IQ_OUT}.log" >&2
    rc=1
fi

if grep -q OK "${RIG_OUT}.status" 2>/dev/null; then
    scp -q $SSH_OPTS "${RX_SSH}:${RIG_RAW_REMOTE}" "$RIG_OUT" 2>/dev/null || true
fi
if [[ -s "$RIG_OUT" ]]; then
    python3 - "$RIG_OUT" <<'PY'
import sys, wave, struct
w = wave.open(sys.argv[1], "rb"); n, ch, fs = w.getnframes(), w.getnchannels(), w.getframerate()
raw = w.readframes(n); w.close()
s = struct.unpack("<%dh" % (len(raw)//2), raw)
L = s[0::ch]
msq = sum(x*x for x in L)/max(1, len(L))/32768**2
peak = max(abs(min(L)), abs(max(L)))/32768 if L else 0
print(f"    rig aud: {sys.argv[1]} ({n/fs:.1f}s @ {fs}Hz)  mean_sq={msq:.6f}  peak={peak:.3f}")
# The number that decides whether the modem's energy gate can discriminate at all.
if msq > 0.00107:
    print(f"             WARNING idle+signal mean_sq {msq:.5f} is above MAX_THRESHOLD/3 (0.00107):")
    print( "             the energy gate may be saturated open — see issue #1020.")
PY
else
    echo "    rig aud: FAILED — no audio captured from ${RX_SSH}" >&2
    echo "             A zero-length capture usually means the rig is not outputting audio;" >&2
    echo "             check AF gain and squelch on the radio." >&2
    rc=1
fi

echo ""
if [[ $rc -eq 0 ]]; then
    cat <<NEXT
Both captures succeeded. To turn them into corpus entries:

  1. Prepare the rig audio for the corpus (48k stereo -> 8k mono, anti-aliased).
     This capture is ALREADY on disk, so decimate it in place — do NOT call
     onair-record-capture.sh, which always records a NEW live capture (passing it 0
     seconds records nothing at all):
       python3 - '${RIG_OUT}' crates/openpulse-modem/tests/captures/<rig>-${NAME}.wav <<'EOP'
       import sys, wave, struct, numpy as np
       from scipy.signal import resample_poly
       src, dst = sys.argv[1], sys.argv[2]
       w = wave.open(src, "rb"); n, ch, fs = w.getnframes(), w.getnchannels(), w.getframerate()
       x = np.frombuffer(w.readframes(n), dtype="<i2").astype(np.float64)[0::ch] / 32768.0; w.close()
       y = resample_poly(x, 1, fs // 8000)
       o = wave.open(dst, "wb"); o.setnchannels(1); o.setsampwidth(2); o.setframerate(8000)
       o.writeframes(struct.pack("<%dh" % len(y), *np.clip(y*32768, -32768, 32767).astype(int))); o.close()
       EOP

  2. Add a row to crates/openpulse-modem/tests/captures/README.md recording PROVENANCE
     (rig, frequency, mode, FEC, payload, level settings, date) and the property the file
     is meant to preserve. A capture with no recorded expectation cannot be asserted against.

  3. The payload transmitted was: '${PAYLOAD}'
     A rig capture WITH a known payload is the corpus's missing piece: it is the only thing
     that can assert an end-to-end DECODE against real audio.
NEXT
else
    echo "One or both captures failed; nothing was added to the corpus." >&2
fi
exit $rc
