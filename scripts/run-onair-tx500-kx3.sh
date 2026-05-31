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
KX3_HAMLIB_MODEL="${KX3_HAMLIB_MODEL:-2045}"
KX3_CAT_PORT="${KX3_CAT_PORT:-/dev/ttyUSB0}"
KX3_CAT_BAUD="${KX3_CAT_BAUD:-38400}"
KX3_PTT_PORT="${KX3_PTT_PORT:-/dev/ttyUSB0}"
KX3_PTT_TYPE="${KX3_PTT_TYPE:-RTS}"
KX3_RIGCTLD_ADDR="${KX3_RIGCTLD_ADDR:-127.0.0.1}"
KX3_RIGCTLD_PORT="${KX3_RIGCTLD_PORT:-4532}"
TX500_HAMLIB_MODEL="${TX500_HAMLIB_MODEL:-2050}"
TX500_CAT_PORT="${TX500_CAT_PORT:-/dev/ttyUSB0}"
TX500_CAT_BAUD="${TX500_CAT_BAUD:-19200}"
TX500_PTT_PORT="${TX500_PTT_PORT:-/dev/ttyUSB0}"
TX500_PTT_TYPE="${TX500_PTT_TYPE:-RTS}"
TX500_RIGCTLD_ADDR="${TX500_RIGCTLD_ADDR:-127.0.0.1}"
TX500_RIGCTLD_PORT="${TX500_RIGCTLD_PORT:-4532}"
TEST_FREQ_HZ="${TEST_FREQ_HZ:-14070000}"
TEST_MODE_RIG="${TEST_MODE_RIG:-USB}"
LOCAL_AUDIO_DEVICE="${LOCAL_AUDIO_DEVICE:-}"
PI_AUDIO_DEVICE="${PI_AUDIO_DEVICE:-}"
if [[ -z "${PI_REPO_DIR:-}" ]]; then
    PI_REPO_DIR='${HOME}/git/OpenPulseHF'
fi
if [[ -z "${PI_LOG_DIR:-}" ]]; then
    PI_LOG_DIR='${HOME}/var/log/openpulse/on-air'
fi
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
pi_cargo() {
    ssh_pi "if command -v cargo >/dev/null 2>&1; then command -v cargo; elif [[ -x \\$HOME/.cargo/bin/cargo ]]; then echo \\$HOME/.cargo/bin/cargo; else echo ''; fi"
}

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
    echo "  [kx3 rigctld] skipped (safety mode: local KX3 uses direct one-shot CAT only)"
}

stop_rigctld_local() {
    if pgrep -f "rigctld.*${KX3_CAT_PORT}" >/dev/null 2>&1; then
        echo "  [kx3 rigctld] stopping stray local rigctld on ${KX3_CAT_PORT}"
        pkill -f "rigctld.*${KX3_CAT_PORT}" 2>/dev/null || true
    fi
    KX3_RIGCTLD_PID=""
}

start_rigctld_pi() {
    echo "  [tx500 rigctld] starting on Pi (model=${TX500_HAMLIB_MODEL} port=${TX500_CAT_PORT})"
    ssh_pi "pkill -x rigctld 2>/dev/null || true; sleep 0.5; \
        nohup rigctld \
            -m ${TX500_HAMLIB_MODEL} \
            -r ${TX500_CAT_PORT} \
            -s ${TX500_CAT_BAUD} \
            -p ${TX500_PTT_PORT} \
            -P ${TX500_PTT_TYPE} \
            -t ${TX500_RIGCTLD_PORT} \
            </dev/null >/tmp/tx500-rigctld.log 2>&1 &
        sleep 1
        pgrep -x rigctld >/dev/null && echo 'ok' || { echo 'fail'; exit 1; }"
}

stop_rigctld_pi() {
    echo "  [tx500 rigctld] stopping on Pi"
    ssh_pi "pkill -x rigctld 2>/dev/null || true" || true
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
    echo "  [tune] KX3 (local) direct CAT → ${TEST_FREQ_HZ} Hz ${TEST_MODE_RIG}"
    rigctl -m "${KX3_HAMLIB_MODEL}" -r "${KX3_CAT_PORT}" -s "${KX3_CAT_BAUD}" F "${TEST_FREQ_HZ}" 2>/dev/null || \
        echo "  [tune] WARNING: could not set frequency on KX3 via direct CAT"
    rigctl -m "${KX3_HAMLIB_MODEL}" -r "${KX3_CAT_PORT}" -s "${KX3_CAT_BAUD}" M "${TEST_MODE_RIG}" 2400 2>/dev/null || \
        echo "  [tune] WARNING: could not set mode on KX3 via direct CAT"
}

tune_tx500_pi() {
    echo "  [tune] TX500 (Pi) → ${TEST_FREQ_HZ} Hz ${TEST_MODE_RIG}"
    ssh_pi "rigctl -m 2 -r ${TX500_RIGCTLD_ADDR}:${TX500_RIGCTLD_PORT} F ${TEST_FREQ_HZ} 2>/dev/null || true; \
            rigctl -m 2 -r ${TX500_RIGCTLD_ADDR}:${TX500_RIGCTLD_PORT} M ${TEST_MODE_RIG} 2400 2>/dev/null || true"
}

# ── Binary build ──────────────────────────────────────────────────────────────
build_local() {
    echo "==> Building cpal-enabled binaries (local / KX3)"
    cargo build --release -p openpulse-cli --features cpal-backend 2>&1 | tail -3
    cargo build --release -p openpulse-ardop --features cpal 2>&1 | tail -3
    cargo build --release -p openpulse-kiss --features cpal 2>&1 | tail -3
    echo "  [build] local binaries ready in ${LOCAL_BIN_DIR}"
}

build_pi() {
    echo "==> Building cpal-enabled binaries on Pi (TX500)"
    local pi_repo_dir
    local pi_cargo_bin
    pi_repo_dir="$(pi_repo)"
    pi_cargo_bin="$(pi_cargo)"
    if [[ -z "${pi_cargo_bin}" ]]; then
        echo "ERROR: cargo missing on Pi" >&2
        exit 1
    fi
    ssh_pi "cd '${pi_repo_dir}' && \
        '${pi_cargo_bin}' build --release -p openpulse-cli --features cpal-backend >/tmp/openpulse-cli-build.log 2>&1 || { tail -3 /tmp/openpulse-cli-build.log; exit 1; } && \
        tail -3 /tmp/openpulse-cli-build.log && \
        '${pi_cargo_bin}' build --release -p openpulse-ardop --features cpal >/tmp/openpulse-ardop-build.log 2>&1 || { tail -3 /tmp/openpulse-ardop-build.log; exit 1; } && \
        tail -3 /tmp/openpulse-ardop-build.log && \
        '${pi_cargo_bin}' build --release -p openpulse-kiss --features cpal >/tmp/openpulse-kiss-build.log 2>&1 || { tail -3 /tmp/openpulse-kiss-build.log; exit 1; } && \
        tail -3 /tmp/openpulse-kiss-build.log && \
        echo '[build] Pi binaries ready'"
}

# ── Status check ──────────────────────────────────────────────────────────────
status_local() {
    echo "── Local (KX3 / IRS) ────────────────────────────────────────────────────────"
    printf "  openpulse:       %s\n" "$(test -x "${LOCAL_BIN_DIR}/openpulse" && echo present || echo MISSING)"
    printf "  openpulse-tnc:   %s\n" "$(test -x "${LOCAL_BIN_DIR}/openpulse-tnc" && echo present || echo MISSING)"
    printf "  rigctld running: %s\n" "$(pgrep -f "rigctld.*${KX3_CAT_PORT}" >/dev/null 2>&1 && echo yes \(unexpected\) || echo no \(expected\))"
    printf "  KX3 serial port: %s\n" "$(test -e "${KX3_CAT_PORT}" && echo present || echo MISSING)"
    printf "  ssh-agent:       %s\n" "$(ssh-add -l >/dev/null 2>&1 && echo loaded || echo NOT LOADED)"
}

status_pi() {
    echo "── Pi (TX500 / ISS) ─────────────────────────────────────────────────────────"
    local pi_repo_dir
    local pi_log_dir
    pi_repo_dir="$(pi_repo)"
    pi_log_dir="$(pi_logdir)"
    ssh_pi "
        printf '  openpulse:       '; test -x '${pi_repo_dir}/target/release/openpulse' && echo present || echo MISSING
        printf '  openpulse-tnc:   '; test -x '${pi_repo_dir}/target/release/openpulse-tnc' && echo present || echo MISSING
        printf '  rigctld running: '; pgrep -x rigctld >/dev/null 2>&1 && echo yes || echo no
        printf '  TX500 serial:    '; test -e '${TX500_CAT_PORT}' && echo present || echo MISSING
        printf '  log dir:         '; test -d '${pi_log_dir}' && echo present || echo missing
    " || echo "  (SSH connection failed)"
}

# ── Cleanup ───────────────────────────────────────────────────────────────────
cleanup_all() {
    echo "==> Cleanup"
    stop_rigctld_local
    stop_rigctld_pi
    pkill -f "${LOCAL_BIN_DIR}/openpulse-tnc" 2>/dev/null || true
    ssh_pi "pkill -f 'openpulse-tnc|openpulse send|openpulse-kisstnc' 2>/dev/null || true" || true
    echo "  done"
}

# ── Setup ─────────────────────────────────────────────────────────────────────
setup() {
    check_ssh_agent
    local pi_repo_dir
    local pi_log_dir
    local pi_cargo_bin
    pi_repo_dir="$(pi_repo)"
    pi_log_dir="$(pi_logdir)"
    pi_cargo_bin="$(pi_cargo)"

    # Create log directories
    mkdir -p "$OUTPUT_DIR"
    ssh_pi "mkdir -p '${pi_log_dir}'"

    # Check/build local binaries
    if [[ ! -x "${LOCAL_BIN_DIR}/openpulse" ]] || \
       [[ ! -x "${LOCAL_BIN_DIR}/openpulse-tnc" ]]; then
        build_local
    else
        echo "==> Local binaries already present"
    fi

    # Check/build Pi binaries
    PI_HAVE_BINS=$(ssh_pi "test -x '${pi_repo_dir}/target/release/openpulse' && \
        test -x '${pi_repo_dir}/target/release/openpulse-tnc' && echo yes || echo no")
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
    if [[ -z "${pi_cargo_bin}" ]]; then
        echo "ERROR: cargo missing on Pi" >&2
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
    "8PSK1000|none|255"
    "64QAM500|none|128"
    "64QAM1000|none|255"
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
    echo "    Note: current openpulse CLI does not expose FEC selection; requested FEC labels are recorded, but the live test path exercises modem transmit/receive only."
    echo "    Audio devices: local='${LOCAL_AUDIO_DEVICE:-default}' pi='${PI_AUDIO_DEVICE:-default}'"
    echo ""

    # Fetch expanded Pi paths once
    local pi_bin_dir
    pi_bin_dir="$(pi_repo)/target/release"
    local pi_log_dir
    pi_log_dir="$(pi_logdir)"

    for case_spec in "${cases[@]}"; do
        IFS='|' read -r MODE FEC PAYLOAD_SIZE <<< "$case_spec"

        printf "  [%-22s fec=%-18s payload=%4sB] ... " "${MODE}" "${FEC}" "${PAYLOAD_SIZE}"

        local iss_log="/tmp/openpulse-iss-${MODE}-${FEC}.log"
        local irs_log="/tmp/openpulse-irs-${MODE}-${FEC}.log"
        local pi_iss_log="${pi_log_dir}/iss-${MODE}-${FEC}.log"
        local listen_ms=$(( (IRS_STARTUP_WAIT + TX_TIMEOUT + 5) * 1000 ))
        local local_device_args=()
        local pi_device_arg=""
        if [[ -n "${LOCAL_AUDIO_DEVICE}" ]]; then
            local_device_args=(--device "${LOCAL_AUDIO_DEVICE}")
        fi
        if [[ -n "${PI_AUDIO_DEVICE}" ]]; then
            pi_device_arg="--device '${PI_AUDIO_DEVICE}'"
        fi

        # Generate a fixed-length ASCII payload so transmit length equals PAYLOAD_SIZE.
        local payload_text
        payload_text=$(python3 -c \
            "import secrets, string, sys; a = string.ascii_letters + string.digits; sys.stdout.write(''.join(secrets.choice(a) for _ in range(${PAYLOAD_SIZE})))")

        # 1. Start IRS receiver locally (KX3, background)
        pkill -f "${LOCAL_BIN_DIR}/openpulse receive" 2>/dev/null || true
        "${LOCAL_BIN_DIR}/openpulse" \
            --backend cpal \
            --log info \
            --ptt none \
            receive \
            --mode "${MODE}" \
            --listen-ms "${listen_ms}" \
            "${local_device_args[@]}" \
            >"${irs_log}" 2>&1 &
        local irs_pid=$!

        sleep "${IRS_STARTUP_WAIT}"

        # 2. Send from ISS (TX500 on Pi)
        local iss_exit=0
        # timeout executes a program, not a shell function, so invoke ssh directly here.
        timeout "${TX_TIMEOUT}" ssh ${SSH_OPTS} "${PI_SSH}" \
            "${pi_bin_dir}/openpulse \
                    --backend cpal \
                    --log info \
                    --ptt rigctld \
                    --rig ${TX500_RIGCTLD_ADDR}:${TX500_RIGCTLD_PORT} \
                    transmit \
                    --mode ${MODE} \
                    ${pi_device_arg} \
                    '${payload_text}' \
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
        elif ! echo "${irs_content}" | grep -Fq "${payload_text}"; then
            fail_reason="IRS: payload not observed in receiver output"
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
case "$ACTION" in
    setup)
        setup
        ;;
    run)
        trap 'cleanup_all' EXIT
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
        trap 'cleanup_all' EXIT
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
