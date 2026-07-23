#!/usr/bin/env bash
# SSH-orchestrated on-air test matrix for a two-station RPi setup.
#
# Usage:
#   source config/onair-stations.sh   # set STATION_A, STATION_B, etc.
#   ./scripts/run-onair-tests.sh [--quick | --full] [--output DIR] [--no-preflight]
#
# Prerequisites on each station:
#   - A cpal-enabled `openpulse` built ON each station (OPENPULSE_BIN); the
#     cross-compiled binaries from deploy-rpi-pair.sh have NO audio backend and
#     will transmit nothing — build with `--features cpal-backend` on the Pi.
#   - A transceiver connected; set PTT_BACKEND (e.g. rigctld) + A_/B_RIGCTLD and
#     A_/B_AUDIO_DEVICE, or run PTT_BACKEND=none for an audio-cable loopback.
#
# Results are written as JSON to OUTPUT_DIR/onair-TIMESTAMP.json.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

usage() {
        cat <<'EOF'
Usage:
    source config/onair-stations.sh
    ./scripts/run-onair-tests.sh [--quick | --full] [--output DIR] [--no-preflight] [--help]

Options:
    --quick         Run the short matrix (default).
    --full          Run the extended matrix.
    --output DIR    Report output directory (default: docs/dev/test-reports).
    --no-preflight  Skip local strict preflight.
    --help          Show this help text.
EOF
}

# ── Config defaults ───────────────────────────────────────────────────────────
STATION_A="${STATION_A:-dc0sk@192.168.1.10}"
STATION_B="${STATION_B:-dc0sk@192.168.1.11}"
SSH_OPTS="${SSH_OPTS:-}"
CALLSIGN_A="${CALLSIGN_A:-K1ABC}"
CALLSIGN_B="${CALLSIGN_B:-K2DEF}"
PTT_BACKEND="${PTT_BACKEND:-none}"        # none | rigctld | rts | dtr | vox | cm108 | gpio
IRS_STARTUP_WAIT="${IRS_STARTUP_WAIT:-10}" # RX AFC settle ~6.4 s + margin (was 5, too short)
KILL_WAIT="${KILL_WAIT:-12}"               # let the scanning decode finish before stopping IRS
TX_TIMEOUT="${TX_TIMEOUT:-90}"             # seconds before ISS transmit is declared failed

# Path to the cpal-enabled openpulse CLI on each station. This MUST be a binary built
# WITH the cpal feature (`cargo build --release -p openpulse-cli --features cpal-backend`),
# built on the Pi — the cross-compiled `~/bin/openpulse` from deploy-rpi-pair.sh has NO
# audio backend and transmits nothing (see that script's header). Default assumes an
# in-repo build on each station.
OPENPULSE_BIN="${OPENPULSE_BIN:-\$HOME/git/OpenPulseHF/target/release/openpulse}"
# Per-station capture/playback device by cpal name (empty = backend default).
A_AUDIO_DEVICE="${A_AUDIO_DEVICE:-}"
B_AUDIO_DEVICE="${B_AUDIO_DEVICE:-}"
# rigctld address:port for --ptt rigctld (only used when PTT_BACKEND=rigctld).
A_RIGCTLD="${A_RIGCTLD:-127.0.0.1:4532}"
B_RIGCTLD="${B_RIGCTLD:-127.0.0.1:4532}"

TIER="quick"
OUTPUT_DIR="docs/dev/test-reports"
RUN_PREFLIGHT=1

while [[ $# -gt 0 ]]; do
    case "$1" in
        --quick) TIER="quick" ;;
        --full)  TIER="full"  ;;
        --output) OUTPUT_DIR="$2"; shift ;;
        --no-preflight) RUN_PREFLIGHT=0 ;;
        --help|-h) usage; exit 0 ;;
        *)
            echo "Unknown argument: $1" >&2
            usage >&2
            exit 1
            ;;
    esac
    shift
done

TS=$(date -u +%Y-%m-%dT%H%M%S)
GIT_SHA=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
REPORT_FILE="${OUTPUT_DIR}/onair-${TS}.json"
mkdir -p "$OUTPUT_DIR"
PREFLIGHT_RAN=false
PREFLIGHT_MODE="skipped"

if [[ "$RUN_PREFLIGHT" -eq 1 ]]; then
    echo "==> Running local preflight gate"
    ./scripts/onair-preflight.sh --strict
    PREFLIGHT_RAN=true
    PREFLIGHT_MODE="strict"
    echo
fi

# ── Test matrix ───────────────────────────────────────────────────────────────
# Format: "mode|fec|payload_size_bytes"
QUICK_CASES=(
    "BPSK250|none|64"
    "BPSK250|rs|64"
    "BPSK250|soft_concatenated|64"
)

FULL_CASES=(
    "BPSK31|none|32"
    "BPSK100|none|64"
    "BPSK250|none|64"
    "BPSK250|rs|64"
    "BPSK250|soft_concatenated|64"
    "MFSK16|rs|32"
    "QPSK250-D|rs|128"
    "QPSK250|none|128"
    "QPSK500|none|128"
    "QPSK500|rs|128"
    "QPSK500|soft_concatenated|128"
    "8PSK500|none|128"
    "8PSK1000|none|256"
    "64QAM500|none|128"
    "64QAM1000|none|256"
)

if [[ "$TIER" == "quick" ]]; then
    CASES=("${QUICK_CASES[@]}")
else
    CASES=("${FULL_CASES[@]}")
fi

# ── Helpers ───────────────────────────────────────────────────────────────────
ssh_a() {
    # shellcheck disable=SC2086
    ssh ${SSH_OPTS} "${STATION_A}" "$@"
}

ssh_b() {
    # shellcheck disable=SC2086
    ssh ${SSH_OPTS} "${STATION_B}" "$@"
}

json_escape() {
    # Minimal JSON string escaping for single-line values.
    printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

# ── Main test loop ────────────────────────────────────────────────────────────
RESULTS=()
PASS=0
FAIL=0
TOTAL=${#CASES[@]}

echo "==> On-air test matrix (tier=${TIER}, ${TOTAL} cases)"
echo "    Station A (ISS): ${STATION_A}  callsign=${CALLSIGN_A}"
echo "    Station B (IRS): ${STATION_B}  callsign=${CALLSIGN_B}"
echo "    Report: ${REPORT_FILE}"
echo ""

for case_spec in "${CASES[@]}"; do
    IFS='|' read -r MODE FEC PAYLOAD_SIZE <<< "$case_spec"

    # Normalise the case FEC token to the CLI vocabulary (soft_concatenated -> soft-concatenated).
    CLI_FEC="${FEC//_/-}"

    echo -n "  [${MODE}] fec=${CLI_FEC} payload=${PAYLOAD_SIZE}B ... "

    IRS_LOG="/tmp/openpulse-irs-${MODE}-${FEC}.log"
    ISS_LOG="/tmp/openpulse-iss-${MODE}-${FEC}.log"

    # A fixed, printable payload the IRS can be checked against verbatim.
    PAYLOAD="ONAIR ${CALLSIGN_A} DE ${CALLSIGN_B} ${MODE} ${CLI_FEC} $(printf 'X%.0s' $(seq 1 "${PAYLOAD_SIZE}"))"
    PAYLOAD="${PAYLOAD:0:${PAYLOAD_SIZE}}"

    # Per-station PTT/device flags.
    a_ptt_args="--ptt ${PTT_BACKEND}"; b_ptt_args="--ptt ${PTT_BACKEND}"
    if [[ "${PTT_BACKEND}" == "rigctld" ]]; then
        a_ptt_args="--ptt rigctld --rig ${A_RIGCTLD}"
        b_ptt_args="--ptt rigctld --rig ${B_RIGCTLD}"
    fi
    a_dev_arg=""; [[ -n "${A_AUDIO_DEVICE}" ]] && a_dev_arg="--device '${A_AUDIO_DEVICE}'"
    b_dev_arg=""; [[ -n "${B_AUDIO_DEVICE}" ]] && b_dev_arg="--device '${B_AUDIO_DEVICE}'"

    irs_listen_ms=$(( (IRS_STARTUP_WAIT + TX_TIMEOUT + 30) * 1000 ))

    # 1. Start the IRS receiver on station B (background). `receive` is the real CLI surface;
    #    the old `openpulse-tnc --listen` / `openpulse send --hex` path no longer exists.
    ssh_b "pkill -f 'openpulse .*receive' 2>/dev/null || true; \
        nohup ${OPENPULSE_BIN} \
            --backend cpal \
            --log debug \
            ${b_ptt_args} \
            receive \
            --mode ${MODE} \
            --fec ${CLI_FEC} \
            --listen-ms ${irs_listen_ms} \
            ${b_dev_arg} \
        >${IRS_LOG} 2>&1 </dev/null &"

    sleep "${IRS_STARTUP_WAIT}"

    # 2. Transmit from station A (ISS), wait for TX to complete.
    ISS_EXIT=0
    timeout "${TX_TIMEOUT}" ssh_a \
        "${OPENPULSE_BIN} \
            --backend cpal \
            --log info \
            ${a_ptt_args} \
            transmit \
            --mode ${MODE} \
            --fec ${CLI_FEC} \
            ${a_dev_arg} \
            '${PAYLOAD}' \
        >${ISS_LOG} 2>&1" || ISS_EXIT=$?

    # 3. Give IRS time to finish the scanning decode, then stop it and fetch logs.
    sleep "${KILL_WAIT}"
    IRS_LOG_CONTENT=$(ssh_b "cat ${IRS_LOG} 2>/dev/null || true; pkill -f 'openpulse .*receive' 2>/dev/null || true")

    # 4. Fetch ISS log.
    ISS_LOG_CONTENT=$(ssh_a "cat ${ISS_LOG} 2>/dev/null || true")

    # 5. Determine pass/fail: ISS must exit 0; the IRS must have printed the payload it decoded.
    TEST_PASS=false
    FAIL_REASON=""
    if [[ $ISS_EXIT -ne 0 ]]; then
        FAIL_REASON="ISS exit code ${ISS_EXIT}"
    elif ! echo "${IRS_LOG_CONTENT}" | grep -qF "${PAYLOAD}"; then
        FAIL_REASON="IRS did not decode the transmitted payload"
    else
        TEST_PASS=true
    fi

    if $TEST_PASS; then
        echo "PASS"
        PASS=$(( PASS + 1 ))
        RESULT_STR="pass"
    else
        echo "FAIL (${FAIL_REASON})"
        FAIL=$(( FAIL + 1 ))
        RESULT_STR="fail"
    fi

    RESULTS+=("{\"mode\":\"$(json_escape "${MODE}")\",\"fec\":\"$(json_escape "${FEC}")\",\"payload_bytes\":${PAYLOAD_SIZE},\"result\":\"${RESULT_STR}\",\"fail_reason\":\"$(json_escape "${FAIL_REASON:-}")\",\"iss_exit\":${ISS_EXIT}}")
done

# ── Write JSON report ─────────────────────────────────────────────────────────
RESULTS_JSON=$(IFS=,; echo "${RESULTS[*]}")
cat > "${REPORT_FILE}" <<JSON
{
  "timestamp": "${TS}",
  "git_sha": "${GIT_SHA}",
    "preflight": {
        "ran": ${PREFLIGHT_RAN},
        "mode": "${PREFLIGHT_MODE}"
    },
  "tier": "${TIER}",
  "station_a": "$(json_escape "${STATION_A}")",
  "station_b": "$(json_escape "${STATION_B}")",
  "callsign_a": "$(json_escape "${CALLSIGN_A}")",
  "callsign_b": "$(json_escape "${CALLSIGN_B}")",
  "ptt_backend": "$(json_escape "${PTT_BACKEND}")",
  "total": ${TOTAL},
  "pass": ${PASS},
  "fail": ${FAIL},
  "cases": [${RESULTS_JSON}]
}
JSON

echo ""
echo "==> Results: ${PASS}/${TOTAL} passed, ${FAIL} failed."
echo "    Report written to: ${REPORT_FILE}"

[[ $FAIL -eq 0 ]]
