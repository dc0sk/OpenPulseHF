#!/usr/bin/env bash
# SSH-orchestrated on-air test matrix for a two-station RPi setup.
#
# Usage:
#   source config/onair-stations.sh   # set STATION_A, STATION_B, etc.
#   ./scripts/run-onair-tests.sh [--quick | --full] [--output DIR] [--no-preflight]
#
# Prerequisites on each station:
#   - ~/bin/openpulse-tnc  (deployed by deploy-rpi-pair.sh)
#   - ~/bin/openpulse      (deployed by deploy-rpi-pair.sh)
#   - Audio loopback or transceiver connected; ARDOP_CMD_PORT / ARDOP_DATA_PORT
#     environment set in ~/bin/openpulse-env.sh if non-default
#
# Results are written as JSON to OUTPUT_DIR/onair-TIMESTAMP.json.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# ── Config defaults ───────────────────────────────────────────────────────────
STATION_A="${STATION_A:-dc0sk@192.168.1.10}"
STATION_B="${STATION_B:-dc0sk@192.168.1.11}"
SSH_OPTS="${SSH_OPTS:-}"
CALLSIGN_A="${CALLSIGN_A:-K1ABC}"
CALLSIGN_B="${CALLSIGN_B:-K2DEF}"
PTT_BACKEND="${PTT_BACKEND:-none}"
IRS_STARTUP_WAIT="${IRS_STARTUP_WAIT:-5}"   # seconds to wait for IRS TNC to be ready
TX_TIMEOUT="${TX_TIMEOUT:-90}"              # seconds before ISS transmit is declared failed

TIER="quick"
OUTPUT_DIR="docs/test-reports"
RUN_PREFLIGHT=1

while [[ $# -gt 0 ]]; do
    case "$1" in
        --quick) TIER="quick" ;;
        --full)  TIER="full"  ;;
        --output) OUTPUT_DIR="$2"; shift ;;
        --no-preflight) RUN_PREFLIGHT=0 ;;
        *) echo "Unknown argument: $1" >&2; exit 1 ;;
    esac
    shift
done

TS=$(date -u +%Y-%m-%dT%H%M%S)
GIT_SHA=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
REPORT_FILE="${OUTPUT_DIR}/onair-${TS}.json"
mkdir -p "$OUTPUT_DIR"

if [[ "$RUN_PREFLIGHT" -eq 1 ]]; then
    echo "==> Running local preflight gate"
    ./scripts/onair-preflight.sh --strict
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
    "BPSK250|concatenated|64"
    "BPSK250|soft_concatenated|64"
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

    echo -n "  [${MODE}] fec=${FEC} payload=${PAYLOAD_SIZE}B ... "

    IRS_LOG="/tmp/openpulse-irs-${MODE}-${FEC}.log"
    ISS_LOG="/tmp/openpulse-iss-${MODE}-${FEC}.log"

    # Build a random payload of the requested size.
    PAYLOAD_HEX=$(python3 -c "import os,sys; sys.stdout.write(os.urandom(${PAYLOAD_SIZE}).hex())")

    # 1. Start IRS TNC on station B (background).
    ssh_b "pkill -f openpulse-tnc || true; \
        ~/bin/openpulse-tnc \
            --mode ${MODE} \
            --callsign ${CALLSIGN_B} \
            --ptt ${PTT_BACKEND} \
            --listen \
        >${IRS_LOG} 2>&1 &"

    sleep "${IRS_STARTUP_WAIT}"

    # 2. Transmit from station A (ISS), wait for TX to complete.
    ISS_EXIT=0
    timeout "${TX_TIMEOUT}" ssh_a \
        "echo '${PAYLOAD_HEX}' | \
            ~/bin/openpulse send \
                --mode ${MODE} \
                --fec ${FEC} \
                --callsign ${CALLSIGN_A} \
                --to ${CALLSIGN_B} \
                --hex \
        >${ISS_LOG} 2>&1" || ISS_EXIT=$?

    # 3. Give IRS a moment to finish writing, then stop it and fetch logs.
    sleep 2
    IRS_LOG_CONTENT=$(ssh_b "cat ${IRS_LOG} 2>/dev/null || true; pkill -f openpulse-tnc || true")

    # 4. Fetch ISS log.
    ISS_LOG_CONTENT=$(ssh_a "cat ${ISS_LOG} 2>/dev/null || true")

    # 5. Determine pass/fail.
    # ISS must exit 0; IRS log must contain "frame received" (case-insensitive).
    TEST_PASS=false
    FAIL_REASON=""
    if [[ $ISS_EXIT -ne 0 ]]; then
        FAIL_REASON="ISS exit code ${ISS_EXIT}"
    elif ! echo "${IRS_LOG_CONTENT}" | grep -qi "frame received"; then
        FAIL_REASON="IRS did not log 'frame received'"
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
