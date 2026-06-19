#!/usr/bin/env bash
# Hardware audio loopback on a SINGLE host through two USB soundcards.
#
#   modem TX --(hwloop_tx, USB card A out)--> analog cable --> (hwloop_rx, USB
#   card B in) --> modem RX
#
# Both ends run locally as two cpal CLI processes -- no SSH, no two Raspberry
# Pis. This is the hardware/dual-clock rung of the loopback ladder (see
# docs/dev/dualcard-loopback.md): unlike the single-clock virtual rig
# (run-loopback-virtual.sh), the two cards have independent sample clocks, so it
# reproduces the sample-rate-offset (SRO) and analog-cable effects that broke the
# wideband multicarrier / dense-QAM modes on the two-Pi rig -- the missing rung
# that previously required two machines.
#
# Prereq: scripts/setup-dualcard-loopback.sh (hwloop_tx / hwloop_rx PCMs +
# mixer normalisation) and a cpal CLI build (cargo build --release -p openpulse-cli).
#
# Usage:
#   scripts/run-loopback-dualcard.sh --quick
#   scripts/run-loopback-dualcard.sh --full
#   scripts/run-loopback-dualcard.sh --single-case "BPSK250|64"
#   scripts/run-loopback-dualcard.sh --level-check        # verify cable direction
#   FEC=soft-concatenated scripts/run-loopback-dualcard.sh --single-case "SCFDMA26-16QAM|64"
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

BIN="${OPENPULSE_BIN:-$REPO_ROOT/target/release/openpulse}"
TX_DEVICE="${TX_DEVICE:-hwloop_tx}"
RX_DEVICE="${RX_DEVICE:-hwloop_rx}"

# USB card indices for per-case mixer re-normalisation (the C-Media hardware AGC
# drifts capture gain down after strong frames). Auto-resolved from the hwloop
# PCM slaves in ~/.asoundrc; override if needed.
_slave_card() {  # pcm-name -> card index from ~/.asoundrc "hw:N,0"
    awk -v p="pcm.$1" '$0 ~ p {f=1} f && /slave.pcm/ {match($0,/hw:([0-9]+)/,m); print m[1]; exit}' "${HOME}/.asoundrc" 2>/dev/null
}
TX_CARD="${TX_CARD:-$(_slave_card hwloop_tx)}"
RX_CARD="${RX_CARD:-$(_slave_card hwloop_rx)}"

IRS_STARTUP_WAIT="${IRS_STARTUP_WAIT:-10}"  # RX AFC settle (~6.4 s) + margin before TX
IRS_LISTEN_MS="${IRS_LISTEN_MS:-45000}"
TX_TIMEOUT="${TX_TIMEOUT:-60}"
KILL_WAIT="${KILL_WAIT:-12}"
FEC="${FEC:-none}"                          # none|rs|rs-interleaved|soft-concatenated|ldpc
CAPTURE_GAIN="${CAPTURE_GAIN:-16}"          # moderate mic-capture gain; max clips a line->mic cable
OUTPUT_DIR="${OUTPUT_DIR:-docs/dev/test-reports}"

# Quick tier: one case per mode family validated on the two-Pi matched-card rig
# (see memory project-loopback-mode-matrix). Single-clock-tolerant modes.
QUICK_CASES=(
    "BPSK100|64"
    "BPSK250|64"
    "QPSK125|64"
    "QPSK250|64"
    "QPSK500|128"
    "8PSK500|128"
    "SCFDMA16|64"
)
FULL_CASES=(
    "BPSK31|32"
    "BPSK63|32"
    "BPSK100|64"
    "BPSK250|64"
    "QPSK125|64"
    "QPSK250|64"
    "QPSK500|128"
    "QPSK1000|128"
    "8PSK500|128"
    "8PSK1000|128"
    "OFDM16|64"
    "OFDM52|64"
    "SCFDMA16|64"
    "SCFDMA52|64"
)

TIER="${TIER:-quick}"
SINGLE_CASE=""
LEVEL_CHECK=0
while [[ $# -gt 0 ]]; do
    case "$1" in
        --quick)       TIER="quick";     shift ;;
        --full)        TIER="full";      shift ;;
        --single-case) SINGLE_CASE="$2"; shift 2 ;;
        --output)      OUTPUT_DIR="$2";  shift 2 ;;
        --level-check) LEVEL_CHECK=1;    shift ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
done

if [[ ! -x "$BIN" ]]; then
    echo "ERROR: cpal CLI not found at $BIN -- run: cargo build --release -p openpulse-cli" >&2
    exit 1
fi
if ! "$BIN" --backend cpal devices 2>/dev/null | grep -q "^${TX_DEVICE}\b" \
|| ! "$BIN" --backend cpal devices 2>/dev/null | grep -q "^${RX_DEVICE}\b"; then
    echo "ERROR: ${TX_DEVICE}/${RX_DEVICE} not enumerated -- run scripts/setup-dualcard-loopback.sh" >&2
    exit 1
fi

_normalise() {  # disable AGC, restore moderate capture gain on both cards
    for c in "$TX_CARD" "$RX_CARD"; do
        [[ -z "$c" ]] && continue
        amixer -c "$c" cset name='Auto Gain Control' 0 >/dev/null 2>&1 || true
        amixer -c "$c" cset name='Mic Capture Switch' 1 >/dev/null 2>&1 || true
        amixer -c "$c" cset name='Mic Capture Volume' "${CAPTURE_GAIN}" >/dev/null 2>&1 || true
    done
}

# ── Level check: confirm the analog cable carries TX -> RX at a sane level ────
# Measures the actual captured RMS/peak with arecord (reliable; the modem build
# logs no energy line) while the modem transmits, so it catches both a missing
# cable (no signal) and an over-hot mic input (clipping).
if [[ "$LEVEL_CHECK" -eq 1 ]]; then
    echo "==> Level check: modem TX on ${TX_DEVICE} (card ${TX_CARD:-?}), capture on RX card ${RX_CARD:-?}"
    _normalise
    pkill -x openpulse 2>/dev/null; sleep 0.3
    cap="/tmp/openpulse-dualcard-levelcheck.wav"
    arecord -D "plughw:${RX_CARD},0" -f S16_LE -r 48000 -c 1 -d 5 "$cap" >/dev/null 2>&1 &
    recpid=$!
    sleep 0.6
    "$BIN" --backend cpal --log error --ptt none transmit --mode BPSK250 \
        --device "$TX_DEVICE" "LEVELCHECK0123456789" >/dev/null 2>&1
    wait $recpid
    python3 - "$cap" <<'PY'
import wave, struct, math, sys
w = wave.open(sys.argv[1]); n = w.getnframes()
s = struct.unpack('<%dh' % n, w.readframes(n))
act = [x for x in s if abs(x) > 200]
rms = math.sqrt(sum(x*x for x in act)/len(act))/32768 if act else 0.0
peak = max((abs(x) for x in s), default=0)/32768
clip = sum(1 for x in s if abs(x) >= 32767)
print(f"    captured rms={rms:.4f}  peak={peak:.4f}  clipped_samples={clip}")
if rms < 0.01:
    print("    NO SIGNAL -- cable missing or running the other way; swap TX/RX or check jacks.")
elif clip > 0 or peak > 0.95:
    print("    CLIPPING -- lower CAPTURE_GAIN (re-run setup with e.g. CAPTURE_GAIN=10).")
else:
    print("    OK -- signal present and unclipped; safe to run the matrix.")
PY
    exit 0
fi

if [[ -n "$SINGLE_CASE" ]]; then
    CASES=("$SINGLE_CASE")
elif [[ "$TIER" == "full" ]]; then
    CASES=("${FULL_CASES[@]}")
else
    CASES=("${QUICK_CASES[@]}")
fi

ts="$(date -u +%Y-%m-%dT%H%M%SZ)"
mkdir -p "$OUTPUT_DIR"
report="${OUTPUT_DIR}/loopback-dualcard-${TIER}-${ts}.json"
pass=0; fail=0; total="${#CASES[@]}"; results=()

echo "==> Dual-card hardware loopback (${ts})  tier=${TIER}  cases=${total}"
echo "    TX: ${TX_DEVICE} (card ${TX_CARD:-?})   RX: ${RX_DEVICE} (card ${RX_CARD:-?})   FEC=${FEC}"
echo "    listen=${IRS_LISTEN_MS}ms  report=${report}"
echo ""

for case_spec in "${CASES[@]}"; do
    IFS='|' read -r MODE PAYLOAD_SIZE <<< "$case_spec"
    payload="$(python3 -c "import secrets,string;print(''.join(secrets.choice(string.ascii_letters+string.digits) for _ in range(${PAYLOAD_SIZE})))")"
    rxlog="/tmp/openpulse-dualcard-rx-${MODE}.log"; txlog="/tmp/openpulse-dualcard-tx-${MODE}.log"
    printf "  [%-12s payload=%4sB] ... " "$MODE" "$PAYLOAD_SIZE"

    _normalise
    pkill -x openpulse 2>/dev/null; sleep 0.3
    "$BIN" --backend cpal --log debug --ptt none receive --mode "$MODE" \
        --fec "$FEC" --listen-ms "$IRS_LISTEN_MS" --device "$RX_DEVICE" --no-afc >"$rxlog" 2>&1 &
    rxpid=$!
    sleep "$IRS_STARTUP_WAIT"
    if ! kill -0 $rxpid 2>/dev/null; then
        echo "FAIL (RX not running)"; fail=$((fail+1))
        results+=("{\"mode\":\"$MODE\",\"payload_bytes\":$PAYLOAD_SIZE,\"result\":\"fail\",\"fail_reason\":\"RX not running\"}")
        sed 's/^/      /' <(tail -n 8 "$rxlog"); continue
    fi

    iss_exit=0
    timeout "$TX_TIMEOUT" "$BIN" --backend cpal --log info --ptt none transmit --mode "$MODE" \
        --fec "$FEC" --device "$TX_DEVICE" "$payload" >"$txlog" 2>&1 || iss_exit=$?
    sleep "$KILL_WAIT"
    kill $rxpid 2>/dev/null; wait $rxpid 2>/dev/null

    if [[ $iss_exit -ne 0 ]]; then
        reason="TX exit ${iss_exit}: $(grep -iE 'error|too low|unsupported' "$txlog" | grep -v 'ALSA lib' | head -1 | sed 's/^ *//')"
        echo "FAIL (${reason})"; fail=$((fail+1))
        results+=("{\"mode\":\"$MODE\",\"payload_bytes\":$PAYLOAD_SIZE,\"result\":\"fail\",\"fail_reason\":\"$reason\"}")
        sed 's/^/      /' <(tail -n 5 "$txlog"); continue
    fi
    if grep -Fq "$payload" "$rxlog"; then
        echo "PASS"; pass=$((pass+1))
        results+=("{\"mode\":\"$MODE\",\"payload_bytes\":$PAYLOAD_SIZE,\"result\":\"pass\",\"fail_reason\":\"\"}")
    else
        echo "FAIL (payload not decoded)"; fail=$((fail+1))
        results+=("{\"mode\":\"$MODE\",\"payload_bytes\":$PAYLOAD_SIZE,\"result\":\"fail\",\"fail_reason\":\"payload not decoded\"}")
        afc="$(grep -E 'AFC settling|AFC decode|AFC:' "$rxlog" | head -3 || true)"
        [[ -n "$afc" ]] && { echo "    RX AFC:"; echo "$afc" | sed 's/^/      /'; }
        echo "    RX tail:"; tail -n 12 "$rxlog" | sed 's/^/      /'
    fi
done

pkill -x openpulse 2>/dev/null
echo ""
echo "==> Results: ${pass}/${total} passed, ${fail} failed."
results_json="$(IFS=,; echo "${results[*]}")"
printf '{"timestamp":"%s","transport":"hardware-dualcard","tx_device":"%s","rx_device":"%s","fec":"%s","total":%d,"pass":%d,"fail":%d,"cases":[%s]}\n' \
    "$ts" "$TX_DEVICE" "$RX_DEVICE" "$FEC" "$total" "$pass" "$fail" "$results_json" > "$report"
echo "    Report: ${report}"
[[ $fail -eq 0 ]]
