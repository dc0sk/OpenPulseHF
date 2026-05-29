#!/usr/bin/env bash
# Automated on-air test orchestrator for:
#   Local station:  Elecraft KX3 via Digirig  — IRS (responding)
#   Remote station: Lab599 TX500 on Raspberry Pi 5 via Digirig and SSH — ISS (initiating)
#
# The script manages rigctld on both sides, optionally builds the cpal-enabled
# binaries, tunes both rigs to the test frequency, runs the requested test matrix,
# collects pass/fail results, and generates a JSON report + evidence bundle.
#
# Prerequisites:
#   - ssh-agent loaded with a key for the Pi
#   - rigctld and rigctl installed on both this laptop and the Pi
#   - Digirig connected and serial ports known (set in profile)
#   - OpenPulseHF repo at ~/git/OpenPulseHF on both machines
#
# Usage:
#   source docs/config/onair-tx500-kx3-local.example.sh
#   ./scripts/run-onair-tx500-kx3.sh <setup|run|supervise|status|cleanup> [options]
#   ./scripts/run-onair-tx500-kx3.sh --help

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# ── Defaults (overridden by profile) ─────────────────────────────────────────
PI_SSH="${PI_SSH:-}"
SSH_OPTS="${SSH_OPTS:--o BatchMode=yes -o ConnectTimeout=10}"
CALLSIGN_A="${CALLSIGN_A:-N0CALL}"
CALLSIGN_B="${CALLSIGN_B:-N0CALL}"
KX3_HAMLIB_MODEL="${KX3_HAMLIB_MODEL:-229}"
KX3_CAT_PORT="${KX3_CAT_PORT:-/dev/ttyUSB0}"
KX3_CAT_BAUD="${KX3_CAT_BAUD:-38400}"
KX3_PTT_PORT="${KX3_PTT_PORT:-/dev/ttyUSB0}"
KX3_PTT_TYPE="${KX3_PTT_TYPE:-RTS}"
KX3_RIGCTLD_ADDR="${KX3_RIGCTLD_ADDR:-127.0.0.1}"
KX3_RIGCTLD_PORT="${KX3_RIGCTLD_PORT:-4532}"
TX500_HAMLIB_MODEL="${TX500_HAMLIB_MODEL:-3020}"
TX500_CAT_PORT="${TX500_CAT_PORT:-/dev/ttyUSB0}"
TX500_CAT_BAUD="${TX500_CAT_BAUD:-19200}"
TX500_PTT_PORT="${TX500_PTT_PORT:-/dev/ttyUSB0}"
TX500_PTT_TYPE="${TX500_PTT_TYPE:-RTS}"
TX500_RIGCTLD_ADDR="${TX500_RIGCTLD_ADDR:-127.0.0.1}"
TX500_RIGCTLD_PORT="${TX500_RIGCTLD_PORT:-4532}"
TEST_FREQ_HZ="${TEST_FREQ_HZ:-14070000}"
TEST_MODE_RIG="${TEST_MODE_RIG:-USB}"
PI_REPO_DIR="${PI_REPO_DIR:-\${HOME}/git/OpenPulseHF}"
PI_LOG_DIR="${PI_LOG_DIR:-\${HOME}/var/log/openpulse/on-air}"
IRS_STARTUP_WAIT="${IRS_STARTUP_WAIT:-5}"
TX_TIMEOUT="${TX_TIMEOUT:-120}"

LOCAL_BIN_DIR="${REPO_ROOT}/target/release"
OUTPUT_DIR="docs/dev/test-reports"
TIER="quick"
LABEL="tx500-kx3"
NOTES_FILE=""
ACTION="supervise"
PROFILE_FILE=""

# PIDs of processes we started (so cleanup() can target them)
KX3_RIGCTLD_PID=""

usage() {
    cat <<'EOF'
Usage:
  source docs/config/onair-tx500-kx3-local.example.sh
  ./scripts/run-onair-tx500-kx3.sh <ACTION> [options]

Actions:
  setup       Build cpal-enabled binaries on both machines, verify rigctld,
              create log directories, set rig frequencies.
  run         Start rigctld, execute test matrix, stop rigctld, write report.
  supervise   setup + run in one command (default when no action given).
  status      Show process and config status on both machines.
  cleanup     Kill rigctld and TNC processes on both machines.

Options:
  --profile FILE   Source this profile before running (overrides env vars).
  --quick          Run the short test matrix (default).
  --full           Run the extended test matrix.
  --label NAME     Label for evidence bundles and reports (default: tx500-kx3).
  --output DIR     Report output directory (default: docs/dev/test-reports).
  --notes FILE     Operator notes file passed to the evidence bundle.
  --help           Show this help.

The script expects the profile variables set by:
  docs/config/onair-tx500-kx3-local.example.sh
EOF
}

# ── Argument parsing ──────────────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
    case "$1" in
        setup|run|supervise|status|cleanup)
            ACTION="$1" ;;
        --profile)
            PROFILE_FILE="$2"; shift ;;
        --quick)
            TIER="quick" ;;
        --full)
            TIER="full" ;;
        --label)
            LABEL="$2"; shift ;;
        --output)
            OUTPUT_DIR="$2"; shift ;;
        --notes)
            NOTES_FILE="$2"; shift ;;
        --help|-h)
            usage; exit 0 ;;
        *)
            echo "Unknown argument: $1" >&2
            usage >&2
            exit 1 ;;
    esac
    shift
done

if [[ -n "$PROFILE_FILE" ]]; then
    if [[ ! -f "$PROFILE_FILE" ]]; then
        echo "Profile file not found: $PROFILE_FILE" >&2
        exit 1
    fi
    # shellcheck disable=SC1090
    source "$PROFILE_FILE"
fi

# ── Validation ────────────────────────────────────────────────────────────────
if [[ -z "$PI_SSH" ]]; then
    echo "PI_SSH is not set. Source the profile first:" >&2
    echo "  source docs/config/onair-tx500-kx3-local.example.sh" >&2
    exit 1
fi

if [[ "$CALLSIGN_A" == "N0CALL" || "$CALLSIGN_B" == "N0CALL" ]]; then
    echo "ERROR: CALLSIGN_A and CALLSIGN_B must be set to valid callsigns, not N0CALL." >&2
    exit 1
fi

# ── Helpers ───────────────────────────────────────────────────────────────────
ssh_pi() {
    # shellcheck disable=SC2086
    ssh ${SSH_OPTS} "${PI_SSH}" "$@"
}

# Expand PI_REPO_DIR and PI_LOG_DIR on the Pi.
pi_repo()  { ssh_pi "echo ${PI_REPO_DIR}"; }
pi_logdir(){ ssh_pi "echo ${PI_LOG_DIR}"; }

json_escape() {
    printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

check_ssh_agent() {
    if ! ssh-add -l >/dev/null 2>&1; then
        echo "ssh-agent has no identities loaded; run: ssh-add" >&2
        exit 1
    fi
}

# ── rigctld lifecycle ─────────────────────────────────────────────────────────
start_rigctld_local() {
    if pgrep -f "rigctld.*${KX3_CAT_PORT}" >/dev/null 2>&1; then
        echo "  [kx3 rigctld] already running on port ${KX3_RIGCTLD_PORT}"
        return
    fi
    echo "  [kx3 rigctld] starting (model=${KX3_HAMLIB_MODEL} port=${KX3_CAT_PORT} baud=${KX3_CAT_BAUD})"
    rigctld \
        -m "${KX3_HAMLIB_MODEL}" \
        -r "${KX3_CAT_PORT}" \
        -s "${KX3_CAT_BAUD}" \
        -p "${KX3_PTT_PORT}" \
        -P "${KX3_PTT_TYPE}" \
        -t "${KX3_RIGCTLD_PORT}" \
        >/tmp/kx3-rigctld.log 2>&1 &
    KX3_RIGCTLD_PID=$!
    sleep 1
    if ! kill -0 "${KX3_RIGCTLD_PID}" 2>/dev/null; then
        echo "  [kx3 rigctld] failed to start; check /tmp/kx3-rigctld.log" >&2
        exit 1
    fi
    echo "  [kx3 rigctld] started (pid=${KX3_RIGCTLD_PID})"
}

stop_rigctld_local() {
    if [[ -n "$KX3_RIGCTLD_PID" ]] && kill -0 "${KX3_RIGCTLD_PID}" 2>/dev/null; then
        echo "  [kx3 rigctld] stopping (pid=${KX3_RIGCTLD_PID})"
        kill "${KX3_RIGCTLD_PID}" || true
        KX3_RIGCTLD_PID=""
    fi
}

start_rigctld_pi() {
    echo "  [tx500 rigctld] starting on Pi (model=${TX500_HAMLIB_MODEL} port=${TX500_CAT_PORT})"
    ssh_pi "pkill -f 'rigctld.*${TX500_CAT_PORT}' 2>/dev/null || true; sleep 0.5; \
        rigctld \
            -m ${TX500_HAMLIB_MODEL} \
            -r ${TX500_CAT_PORT} \
            -s ${TX500_CAT_BAUD} \
            -p ${TX500_PTT_PORT} \
            -P ${TX500_PTT_TYPE} \
            -t ${TX500_RIGCTLD_PORT} \
            >/tmp/tx500-rigctld.log 2>&1 &
        sleep 1
        pgrep -f 'rigctld.*${TX500_CAT_PORT}' >/dev/null && echo 'ok' || { echo 'fail'; exit 1; }"
}

stop_rigctld_pi() {
    echo "  [tx500 rigctld] stopping on Pi"
    ssh_pi "pkill -f 'rigctld.*${TX500_CAT_PORT}' 2>/dev/null || true" || true
}

# ── Frequency / mode tuning ───────────────────────────────────────────────────
tune_rig() {
    local label="$1"
    local addr="$2"
    local port="$3"
    echo "  [tune] ${label} → ${TEST_FREQ_HZ} Hz ${TEST_MODE_RIG}"
    rigctl -m 2 -r "${addr}:${port}" F "${TEST_FREQ_HZ}" 2>/dev/null || \
        echo "  [tune] WARNING: could not set frequency on ${label} (rig may not support CAT freq set)"
    rigctl -m 2 -r "${addr}:${port}" M "${TEST_MODE_RIG}" 2400 2>/dev/null || \
        echo "  [tune] WARNING: could not set mode on ${label}"
}

tune_kx3_local() {
    tune_rig "KX3 (local)" "${KX3_RIGCTLD_ADDR}" "${KX3_RIGCTLD_PORT}"
}

tune_tx500_pi() {
    echo "  [tune] TX500 (Pi) → ${TEST_FREQ_HZ} Hz ${TEST_MODE_RIG}"
    ssh_pi "rigctl -m 2 -r ${TX500_RIGCTLD_ADDR}:${TX500_RIGCTLD_PORT} F ${TEST_FREQ_HZ} 2>/dev/null || true; \
            rigctl -m 2 -r ${TX500_RIGCTLD_ADDR}:${TX500_RIGCTLD_PORT} M ${TEST_MODE_RIG} 2400 2>/dev/null || true"
}

# ── Binary build ──────────────────────────────────────────────────────────────
build_local() {
    echo "==> Building cpal-enabled binaries (local / KX3)"
    cargo build --release \
        -p openpulse-cli \
        -p openpulse-ardop \
        -p openpulse-kiss \
        --features cpal-backend 2>&1 | tail -5
    echo "  [build] local binaries ready in ${LOCAL_BIN_DIR}"
}

build_pi() {
    echo "==> Building cpal-enabled binaries on Pi (TX500)"
    ssh_pi "cd ${PI_REPO_DIR} && \
        cargo build --release \
            -p openpulse-cli \
            -p openpulse-ardop \
            -p openpulse-kiss \
            --features cpal-backend 2>&1 | tail -5 && \
        echo '[build] Pi binaries ready'"
}

# ── Status check ──────────────────────────────────────────────────────────────
status_local() {
    echo "── Local (KX3 / IRS) ────────────────────────────────────────────────────────"
    printf "  openpulse:       %s\n" "$(test -x "${LOCAL_BIN_DIR}/openpulse" && echo present || echo MISSING)"
    printf "  openpulse-tnc:   %s\n" "$(test -x "${LOCAL_BIN_DIR}/openpulse-tnc" && echo present || echo MISSING)"
    printf "  rigctld running: %s\n" "$(pgrep -f "rigctld.*${KX3_CAT_PORT}" >/dev/null 2>&1 && echo yes || echo no)"
    printf "  KX3 serial port: %s\n" "$(test -e "${KX3_CAT_PORT}" && echo present || echo MISSING)"
    printf "  ssh-agent:       %s\n" "$(ssh-add -l >/dev/null 2>&1 && echo loaded || echo NOT LOADED)"
}

status_pi() {
    echo "── Pi (TX500 / ISS) ─────────────────────────────────────────────────────────"
    ssh_pi "
        printf '  openpulse:       '; test -x \${HOME}/git/OpenPulseHF/target/release/openpulse && echo present || echo MISSING
        printf '  openpulse-tnc:   '; test -x \${HOME}/git/OpenPulseHF/target/release/openpulse-tnc && echo present || echo MISSING
        printf '  rigctld running: '; pgrep -f 'rigctld.*${TX500_CAT_PORT}' >/dev/null 2>&1 && echo yes || echo no
        printf '  TX500 serial:    '; test -e '${TX500_CAT_PORT}' && echo present || echo MISSING
        printf '  log dir:         '; test -d \${HOME}/var/log/openpulse/on-air && echo present || echo missing
    " || echo "  (SSH connection failed)"
}

# ── Cleanup ───────────────────────────────────────────────────────────────────
cleanup_all() {
    echo "==> Cleanup"
    stop_rigctld_local
    stop_rigctld_pi
    pkill -f "${LOCAL_BIN_DIR}/openpulse-tnc" 2>/dev/null || true
    ssh_pi "pkill -f 'openpulse-tnc\|openpulse send\|openpulse-kisstnc' 2>/dev/null || true" || true
    echo "  done"
}

# ── Setup ─────────────────────────────────────────────────────────────────────
setup() {
    check_ssh_agent

    # Create log directories
    mkdir -p "$OUTPUT_DIR"
    ssh_pi "mkdir -p ${PI_LOG_DIR}"

    # Check/build local binaries
    if [[ ! -x "${LOCAL_BIN_DIR}/openpulse" ]] || \
       [[ ! -x "${LOCAL_BIN_DIR}/openpulse-tnc" ]]; then
        build_local
    else
        echo "==> Local binaries already present (use --build to force rebuild)"
    fi

    # Check/build Pi binaries
    PI_HAVE_BINS=$(ssh_pi "test -x \${HOME}/git/OpenPulseHF/target/release/openpulse && \
        test -x \${HOME}/git/OpenPulseHF/target/release/openpulse-tnc && echo yes || echo no")
    if [[ "$PI_HAVE_BINS" != "yes" ]]; then
        build_pi
    else
        echo "==> Pi binaries already present"
    fi

    # Verify rigctld is installed
    if ! command -v rigctld >/dev/null 2>&1; then
        echo "ERROR: rigctld not found locally; install hamlib (e.g. sudo pacman -S hamlib)" >&2
        exit 1
    fi
    ssh_pi "command -v rigctld >/dev/null || { echo 'ERROR: rigctld missing on Pi'; exit 1; }"

    # Start rigctld and tune both rigs
    start_rigctld_local
    start_rigctld_pi
    sleep 1
    tune_kx3_local
    tune_tx500_pi

    echo "==> Setup complete"
    status_local
    status_pi
}

# ── Test matrix ───────────────────────────────────────────────────────────────
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

run_matrix() {
    local ts
    ts=$(date -u +%Y-%m-%dT%H%M%S)
    local git_sha
    git_sha=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
    local report="${OUTPUT_DIR}/onair-${ts}.json"
    mkdir -p "$OUTPUT_DIR"

    local cases=()
    if [[ "$TIER" == "quick" ]]; then
        cases=("${QUICK_CASES[@]}")
    else
        cases=("${FULL_CASES[@]}")
    fi

    local total=${#cases[@]}
    local pass=0
    local fail=0
    local results=()

    echo "==> On-air test matrix (tier=${TIER}, ${total} cases)"
    echo "    ISS: TX500 on Pi (${PI_SSH})  callsign=${CALLSIGN_A}"
    echo "    IRS: KX3 local               callsign=${CALLSIGN_B}"
    echo "    Freq: ${TEST_FREQ_HZ} Hz ${TEST_MODE_RIG}"
    echo "    Report: ${report}"
    echo ""

    # Fetch expanded Pi paths once
    local pi_bin_dir
    pi_bin_dir=$(ssh_pi "echo \${HOME}/git/OpenPulseHF/target/release")
    local pi_log_dir
    pi_log_dir=$(ssh_pi "echo ${PI_LOG_DIR}")

    for case_spec in "${cases[@]}"; do
        IFS='|' read -r MODE FEC PAYLOAD_SIZE <<< "$case_spec"

        printf "  [%-22s fec=%-18s payload=%4sB] ... " "${MODE}" "${FEC}" "${PAYLOAD_SIZE}"

        local iss_log="/tmp/openpulse-iss-${MODE}-${FEC}.log"
        local irs_log="/tmp/openpulse-irs-${MODE}-${FEC}.log"
        local pi_iss_log="${pi_log_dir}/iss-${MODE}-${FEC}.log"

        # Generate random hex payload
        local payload_hex
        payload_hex=$(python3 -c \
            "import os,sys; sys.stdout.write(os.urandom(${PAYLOAD_SIZE}).hex())")

        # 1. Start IRS TNC locally (KX3, background)
        pkill -f "${LOCAL_BIN_DIR}/openpulse-tnc" 2>/dev/null || true
        RUST_LOG=info "${LOCAL_BIN_DIR}/openpulse-tnc" \
            --mode "${MODE}" \
            --callsign "${CALLSIGN_B}" \
            --ptt rigctld \
            --listen \
            >"${irs_log}" 2>&1 &
        local irs_pid=$!

        sleep "${IRS_STARTUP_WAIT}"

        # 2. Send from ISS (TX500 on Pi)
        local iss_exit=0
        timeout "${TX_TIMEOUT}" ssh_pi \
            "echo '${payload_hex}' | \
                ${pi_bin_dir}/openpulse send \
                    --mode ${MODE} \
                    --fec ${FEC} \
                    --callsign ${CALLSIGN_A} \
                    --to ${CALLSIGN_B} \
                    --ptt rigctld \
                    --hex \
                >${pi_iss_log} 2>&1" \
            || iss_exit=$?

        # 3. Let IRS finish, then collect logs
        sleep 2
        kill "${irs_pid}" 2>/dev/null || true
        wait "${irs_pid}" 2>/dev/null || true
        local irs_content
        irs_content=$(cat "${irs_log}" 2>/dev/null || true)
        local iss_content
        iss_content=$(ssh_pi "cat '${pi_iss_log}' 2>/dev/null || true" || true)

        # 4. Pass/fail
        local test_pass=false
        local fail_reason=""
        if [[ $iss_exit -ne 0 ]]; then
            fail_reason="ISS exit ${iss_exit}"
        elif ! echo "${irs_content}" | grep -qi "frame received"; then
            fail_reason="IRS: no 'frame received' in log"
        else
            test_pass=true
        fi

        if $test_pass; then
            echo "PASS"
            pass=$(( pass + 1 ))
        else
            echo "FAIL (${fail_reason})"
            fail=$(( fail + 1 ))
        fi

        results+=("{\"mode\":\"$(json_escape "${MODE}")\",\
\"fec\":\"$(json_escape "${FEC}")\",\
\"payload_bytes\":${PAYLOAD_SIZE},\
\"result\":\"$(${test_pass} && echo pass || echo fail)\",\
\"fail_reason\":\"$(json_escape "${fail_reason:-}")\",\
\"iss_exit\":${iss_exit}}")
    done

    # Write JSON report
    local results_json
    results_json=$(IFS=,; echo "${results[*]}")
    cat > "${report}" <<JSON
{
  "timestamp": "${ts}",
  "git_sha": "${git_sha}",
  "tier": "${TIER}",
  "iss_station": "$(json_escape "${PI_SSH}")",
  "irs_station": "local",
  "callsign_iss": "$(json_escape "${CALLSIGN_A}")",
  "callsign_irs": "$(json_escape "${CALLSIGN_B}")",
  "freq_hz": ${TEST_FREQ_HZ},
  "rig_mode": "$(json_escape "${TEST_MODE_RIG}")",
  "first_pass_note": "$(json_escape "${ON_AIR_FIRST_PASS_NOTE:-}")",
  "total": ${total},
  "pass": ${pass},
  "fail": ${fail},
  "cases": [${results_json}]
}
JSON

    echo ""
    echo "==> Results: ${pass}/${total} passed, ${fail} failed."
    echo "    Report: ${report}"

    # Evidence bundle
    local bundle_args=(
        "./scripts/onair-bundle-evidence.sh"
        "--report" "${report}"
        "--label" "${LABEL}-${TIER}"
    )
    if [[ -n "$NOTES_FILE" ]]; then
        bundle_args+=("--notes" "$NOTES_FILE")
    fi
    if [[ -x "./scripts/onair-bundle-evidence.sh" ]]; then
        echo ""
        "${bundle_args[@]}"
    fi

    [[ $fail -eq 0 ]]
}

# ── Main ─────────────────────────────────────────────────────────────────────
trap 'cleanup_all' EXIT

case "$ACTION" in
    setup)
        setup
        ;;
    run)
        check_ssh_agent
        start_rigctld_local
        start_rigctld_pi
        tune_kx3_local
        tune_tx500_pi
        run_matrix
        stop_rigctld_local
        stop_rigctld_pi
        ;;
    supervise)
        check_ssh_agent
        setup
        run_matrix
        ;;
    status)
        status_local
        status_pi
        ;;
    cleanup)
        cleanup_all
        ;;
    *)
        usage
        exit 1
        ;;
esac
