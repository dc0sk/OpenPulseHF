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
RETRIES="${RETRIES:-3}"                      # absorb transient wideband acquisition flakiness in long sweeps (matches run-loopback-virtual.sh)
FEC_EXPLICIT="${FEC+set}"
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
# `--full` is derived from the plugin registry at run time, NOT from a hand-maintained list.
#
# The previous FULL_CASES array froze 14 cases and silently drifted as the registry grew: MFSK16
# (hpx_hf's SL1 rung) and the differential QPSK250-D/QPSK500-D modes were never in it, so a --full
# run reported 14/14 while covering none of them. A hardcoded list standing in for an enumeration
# reads as coverage; that is the same defect the audit found in CRATES_TESTED and the plugin
# registrars (audit 2026-07-19, loopback-revalidation-plan task A).
enumerate_registry_modes() {
    "$BIN" modes 2>/dev/null \
        | grep -oE '\(([^)]+)\)$' \
        | tr -d '()' | tr ',' '\n' | sed 's/^ *//;s/ *$//' | grep -E '^[A-Z0-9]' | sort -u
}

# Payload size for a mode, by symbol rate: slow rungs stay short so a case does not run for minutes.
payload_for() {
    case "$1" in
        MFSK16*)                       echo 32 ;;   # 31.25 baud, ~17 s per frame
        BPSK31|BPSK63)                 echo 32 ;;
        *9600*|*2000*|*1000*)          echo 128 ;;
        *500*)                         echo 128 ;;
        *)                             echo 64 ;;
    esac
}

# Some modes cannot decode without FEC, so a no-FEC sweep would report a FALSE failure.
#
# Differential (`-D`) encodes each dibit as a phase increment: a fade rotation cancels symbol to
# symbol, but every slip costs a dibit that only FEC can repair. Uncoded differential measures 0.00
# BY DESIGN (see CLAUDE.md, #923). Running it with FEC=none and recording "fail" would manufacture a
# regression that is really a configuration error.
fec_for() {
    # An explicitly-set FEC= wins: the per-mode value is a DEFAULT that stops a naive sweep recording
    # a false failure, not a mandate. Hardcoding it made deliberate experiments (e.g. trying LDPC on a
    # differential rung) silently run the wrong FEC.
    if [[ -n "${FEC_EXPLICIT:-}" ]]; then echo "$FEC"; return; fi
    case "$1" in
        *-D)      echo "rs" ;;
        MFSK16*)  echo "rs" ;;   # sub-floor rung is always FEC-protected in the ladder
        *)        echo "$FEC" ;;
    esac
}

# Modes that cannot run on this rig, with the reason. Reported as SKIP, never silently dropped.
skip_reason_for() {
    case "$1" in
        *9600*) echo "needs Fs >= 38.4 kHz (>=4 samples/symbol); this path is 8 kHz" ;;
        FSK4-ACK|MFSK16-ACK) echo "ACK-channel waveform, exercised by the ARQ tests not a data sweep" ;;
        *) echo "" ;;
    esac
}

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
    CASES=()
    SKIPPED=()
    if [[ -n "${MODES:-}" ]]; then
        read -r -a _reg <<< "$MODES"
    else
        mapfile -t _reg < <(enumerate_registry_modes)
    fi
    if [[ ${#_reg[@]} -eq 0 ]]; then
        echo "ERROR: could not enumerate modes from '$BIN modes'" >&2
        exit 1
    fi
    for m in "${_reg[@]}"; do
        reason="$(skip_reason_for "$m")"
        if [[ -n "$reason" ]]; then
            SKIPPED+=("${m}|${reason}")
        else
            CASES+=("${m}|$(payload_for "$m")")
        fi
    done
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
if [[ ${#SKIPPED[@]:-0} -gt 0 ]]; then
    echo "    skipped (${#SKIPPED[@]}):"
    for sk in "${SKIPPED[@]}"; do
        IFS='|' read -r sm sr <<< "$sk"
        printf "      %-16s %s\n" "$sm" "$sr"
    done
fi
echo ""

# One TX->RX attempt. Sets REASON (empty on success); returns 0 pass / 1 fail.
run_once() {  # mode payload-size payload-text
    local MODE="$1" PAYLOAD_SIZE="$2" payload="$3"
    local rxlog="/tmp/openpulse-dualcard-rx-${MODE}.log" txlog="/tmp/openpulse-dualcard-tx-${MODE}.log"
    local MODE_FEC; MODE_FEC="$(fec_for "$MODE")"
    _normalise
    pkill -x openpulse 2>/dev/null; sleep 0.3
    "$BIN" --backend cpal --log debug --ptt none receive --mode "$MODE" \
        --fec "$MODE_FEC" --listen-ms "$IRS_LISTEN_MS" --device "$RX_DEVICE" --no-afc >"$rxlog" 2>&1 &
    local rxpid=$!
    sleep "$IRS_STARTUP_WAIT"
    if ! kill -0 $rxpid 2>/dev/null; then REASON="RX not running"; return 1; fi
    local iss_exit=0
    timeout "$TX_TIMEOUT" "$BIN" --backend cpal --log info --ptt none transmit --mode "$MODE" \
        --fec "$MODE_FEC" --device "$TX_DEVICE" "$payload" >"$txlog" 2>&1 || iss_exit=$?
    sleep "$KILL_WAIT"
    kill $rxpid 2>/dev/null; wait $rxpid 2>/dev/null
    if [[ $iss_exit -ne 0 ]]; then
        REASON="TX exit ${iss_exit}: $(grep -iE 'error|too low|unsupported' "$txlog" | grep -v 'ALSA lib' | head -1 | sed 's/^ *//')"
        return 1
    fi
    if grep -Fq "$payload" "$rxlog"; then REASON=""; return 0; fi
    REASON="payload not decoded"
    return 1
}

for case_spec in "${CASES[@]}"; do
    IFS='|' read -r MODE PAYLOAD_SIZE <<< "$case_spec"
    payload="$(python3 -c "import secrets,string;print(''.join(secrets.choice(string.ascii_letters+string.digits) for _ in range(${PAYLOAD_SIZE})))")"
    CASE_FEC="$(fec_for "$MODE")"
    printf "  [%-14s payload=%4sB fec=%-16s] ... " "$MODE" "$PAYLOAD_SIZE" "$CASE_FEC"

    ok=1; REASON=""; attempts=0
    for ((r=1; r<=RETRIES; r++)); do
        attempts=$r
        if run_once "$MODE" "$PAYLOAD_SIZE" "$payload"; then ok=0; break; fi
    done

    if [[ $ok -eq 0 ]]; then
        [[ $attempts -gt 1 ]] && echo "PASS (attempt ${attempts})" || echo "PASS"
        pass=$((pass+1))
        results+=("{\"mode\":\"$MODE\",\"payload_bytes\":$PAYLOAD_SIZE,\"fec\":\"$CASE_FEC\",\"result\":\"pass\",\"attempts\":$attempts,\"fail_reason\":\"\"}")
    else
        echo "FAIL (${REASON}) after ${attempts} attempt(s)"
        fail=$((fail+1))
        results+=("{\"mode\":\"$MODE\",\"payload_bytes\":$PAYLOAD_SIZE,\"fec\":\"$CASE_FEC\",\"result\":\"fail\",\"attempts\":$attempts,\"fail_reason\":\"$REASON\"}")
        rxlog="/tmp/openpulse-dualcard-rx-${MODE}.log"
        afc="$(grep -E 'AFC settling|AFC decode|AFC:' "$rxlog" 2>/dev/null | head -3 || true)"
        [[ -n "$afc" ]] && { echo "    RX AFC:"; echo "$afc" | sed 's/^/      /'; }
        echo "    RX tail:"; tail -n 12 "$rxlog" 2>/dev/null | sed 's/^/      /'
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
