#!/usr/bin/env bash
# Automated on-air test orchestrator for:
#   Station A (ISS): IC-9700 on dc0sk-rpi51
#   Station B (IRS): FT-991A on dd2zm-landline
#
# This script clones the tx500/kx3 workflow for a dual-SSH setup:
# - Builds cpal-enabled binaries on Station A
# - Transfers binaries to Station B (no remote compile on B)
# - Preserves repo-like folder structure on Station B
# - Enforces 2m-only operating frequencies

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# Defaults (overridden by profile)
A_SSH="${A_SSH:-dc0sk@dc0sk-rpi51}"
B_SSH="${B_SSH:-dd2zm@dd2zm-landline}"
SSH_OPTS="${SSH_OPTS:--o BatchMode=yes -o ConnectTimeout=10}"
CALLSIGN_A="${CALLSIGN_A:-N0CALL}"
CALLSIGN_B="${CALLSIGN_B:-N0CALL}"
A_LABEL="${A_LABEL:-IC-9700}"
B_LABEL="${B_LABEL:-FT-991A}"
A_HAMLIB_MODEL="${A_HAMLIB_MODEL:-3085}"
A_CAT_PORT="${A_CAT_PORT:-/dev/ttyUSB0}"
A_CAT_BAUD="${A_CAT_BAUD:-115200}"
A_PTT_PORT="${A_PTT_PORT:-/dev/ttyUSB0}"
A_PTT_TYPE="${A_PTT_TYPE:-RTS}"
A_RIGCTLD_ADDR="${A_RIGCTLD_ADDR:-127.0.0.1}"
A_RIGCTLD_PORT="${A_RIGCTLD_PORT:-4532}"
B_HAMLIB_MODEL="${B_HAMLIB_MODEL:-1035}"
B_CAT_PORT="${B_CAT_PORT:-/dev/ttyUSB0}"
B_CAT_BAUD="${B_CAT_BAUD:-38400}"
B_PTT_PORT="${B_PTT_PORT:-/dev/ttyUSB0}"
B_PTT_TYPE="${B_PTT_TYPE:-RTS}"
B_RIGCTLD_ADDR="${B_RIGCTLD_ADDR:-127.0.0.1}"
B_RIGCTLD_PORT="${B_RIGCTLD_PORT:-4532}"
TEST_FREQ_HZ="${TEST_FREQ_HZ:-145650000}"
TEST_MODE_RIG_A="${TEST_MODE_RIG_A:-${TEST_MODE_RIG:-USB}}"
TEST_MODE_RIG_B="${TEST_MODE_RIG_B:-${TEST_MODE_RIG:-PKTUSB}}"
A_AUDIO_DEVICE="${A_AUDIO_DEVICE:-}"
B_AUDIO_DEVICE="${B_AUDIO_DEVICE:-}"
A_AUDIO_DEVICE_LABEL="${A_AUDIO_DEVICE_LABEL:-IC-9700 USB Audio CODEC}"
if [[ -z "${A_REPO_DIR:-}" ]]; then
    A_REPO_DIR='${HOME}/git/OpenPulseHF'
fi
if [[ -z "${B_REPO_DIR:-}" ]]; then
    # Repo-like layout even if B is not a git checkout.
    B_REPO_DIR='${HOME}/openpulse/OpenPulseHF'
fi
if [[ -z "${B_LOG_DIR:-}" ]]; then
    B_LOG_DIR='${HOME}/var/log/openpulse/on-air'
fi
IRS_STARTUP_WAIT="${IRS_STARTUP_WAIT:-10}"  # RX AFC settle ~6.4 s + margin (was 5, too short)
KILL_WAIT="${KILL_WAIT:-12}"                # let the scanning decode finish before killing IRS (was a bare sleep 2)
TX_TIMEOUT="${TX_TIMEOUT:-120}"
A_RFPOWER="${A_RFPOWER:-0.05}"
B_RFPOWER="${B_RFPOWER:-0.05}"
TELEMETRY_ENABLE="${TELEMETRY_ENABLE:-0}"
TELEMETRY_SAMPLES="${TELEMETRY_SAMPLES:-40}"
TELEMETRY_INTERVAL="${TELEMETRY_INTERVAL:-0.2}"
ALLOW_TUNER_ON_HIGH_SWR="${ALLOW_TUNER_ON_HIGH_SWR:-0}"
HIGH_SWR_THRESHOLD="${HIGH_SWR_THRESHOLD:-2.0}"
TUNER_TRIGGER_ON_QSY="${TUNER_TRIGGER_ON_QSY:-1}"
QSY_MODE_ENABLED="${QSY_MODE_ENABLED:-0}"
POWER_CYCLE_ENABLE="${POWER_CYCLE_ENABLE:-0}"
POWER_OFF_WAIT="${POWER_OFF_WAIT:-10}"
POWER_ON_WAIT="${POWER_ON_WAIT:-15}"
LOOPBACK_IRS_SSH="${LOOPBACK_IRS_SSH:-dc0sk@dc0sk-rpi52}"
LOOPBACK_IRS_BIN_DIR="${LOOPBACK_IRS_BIN_DIR:-/home/dc0sk/openpulse/bin}"
LOOPBACK_TIER="${LOOPBACK_TIER:-quick}"
LOOPBACK_REGRESSION_INTERVAL="${LOOPBACK_REGRESSION_INTERVAL:-0}"

A_SAVED_FREQ=""
A_SAVED_MODE=""
A_SAVED_RFPOWER=""
A_SAVED_MICGAIN=""
A_SAVED_COMP=""
B_SAVED_FREQ=""
B_SAVED_MODE=""
B_SAVED_RFPOWER=""
B_SAVED_MICGAIN=""
B_SAVED_COMP=""

OUTPUT_DIR="docs/dev/test-reports"
TIER="quick"
LABEL="ic9700-ft991a"
NOTES_FILE=""
ACTION="supervise"
PROFILE_FILE=""
SINGLE_CASE=""
REVERSE="0"
SIDE_A_SINGLE_CASE="${SIDE_A_SINGLE_CASE:-BPSK250|none|64}"

usage() {
    cat <<'EOF'
Usage:
  source docs/config/onair-ic9700-ft991a.example.sh
  ./scripts/run-onair-ic9700-ft991a.sh <ACTION> [options]

Actions:
  setup       Build on Station A, transfer binaries to Station B, verify rigctld,
              create logs, tune both rigs.
  run         Start rigctld on both stations, execute matrix, write report.
    sidea       Build on Station A and run a single transmit-only smoke test on
                            side-A using the IC-9700 USB audio path.
  supervise   setup + run in one command (default).
  status      Show process and config status on both stations.
  cleanup     Kill rigctld and TNC processes on both stations.

Options:
  --profile FILE   Source this profile before running (overrides env vars).
  --quick          Run short matrix (default).
  --full           Run extended matrix.
  --label NAME     Label for evidence bundles and reports.
  --output DIR     Report output directory.
  --notes FILE     Operator notes file passed to evidence bundle.
    --single-case X  Run only one case in format MODE|FEC|PAYLOAD_BYTES.
                                     Comma form MODE,FEC,PAYLOAD_BYTES is also accepted.
                                     Example: --single-case 'BPSK250|none|64'
    --reverse        Swap roles: side-B transmits (ISS), side-A receives (IRS).
  --help           Show this help.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        setup|run|sidea|supervise|status|cleanup)
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
        --single-case)
            SINGLE_CASE="$2"; shift ;;
        --reverse)
            REVERSE="1" ;;
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

if [[ "$CALLSIGN_A" == "N0CALL" || "$CALLSIGN_B" == "N0CALL" ]]; then
    echo "ERROR: CALLSIGN_A and CALLSIGN_B must be set to valid callsigns." >&2
    exit 1
fi

if [[ -z "$A_SSH" || -z "$B_SSH" ]]; then
    echo "ERROR: both A_SSH and B_SSH must be set." >&2
    exit 1
fi

# Enforce requested 2m test sub-band by default (Germany): 144.500-144.750 MHz.
BAND2M_MIN_HZ="${BAND2M_MIN_HZ:-144500000}"
BAND2M_MAX_HZ="${BAND2M_MAX_HZ:-144750000}"
if (( TEST_FREQ_HZ < BAND2M_MIN_HZ || TEST_FREQ_HZ > BAND2M_MAX_HZ )); then
    echo "ERROR: TEST_FREQ_HZ=${TEST_FREQ_HZ} is outside allowed test range ${BAND2M_MIN_HZ}-${BAND2M_MAX_HZ}." >&2
    exit 1
fi

ssh_a() {
    # shellcheck disable=SC2086
    ssh ${SSH_OPTS} "${A_SSH}" "$@"
}

ssh_b() {
    # shellcheck disable=SC2086
    ssh ${SSH_OPTS} "${B_SSH}" "$@"
}

a_repo() { ssh_a "echo ${A_REPO_DIR}"; }
b_repo() { ssh_b "echo ${B_REPO_DIR}"; }
b_logdir() { ssh_b "echo ${B_LOG_DIR}"; }

a_cargo() {
    ssh_a "if command -v cargo >/dev/null 2>&1; then command -v cargo; elif [[ -x \$HOME/.cargo/bin/cargo ]]; then echo \$HOME/.cargo/bin/cargo; else echo ''; fi"
}

json_escape() {
    printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

is_truthy() {
    local v
    v="$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]')"
    [[ "$v" == "1" || "$v" == "true" || "$v" == "yes" || "$v" == "on" ]]
}

verify_audio_codec_a() {
    local codec_line
    codec_line="$(ssh_a "aplay -l 2>/dev/null | grep -i -F '${A_AUDIO_DEVICE_LABEL}' | head -n1 || true")"
    if [[ -z "$codec_line" ]]; then
        codec_line="$(ssh_a "aplay -l 2>/dev/null | grep -i -F 'USB Audio CODEC' | head -n1 || true")"
    fi
    if [[ -z "$codec_line" ]]; then
        echo "ERROR: side-A audio codec '${A_AUDIO_DEVICE_LABEL}' (or USB Audio CODEC) not found" >&2
        ssh_a "aplay -l 2>/dev/null | sed -n '1,12p'" || true
        exit 1
    fi
    echo "  [${A_LABEL} audio] ${codec_line}"

    local pcm_line
    pcm_line="$(ssh_a "amixer -c CODEC get PCM 2>/dev/null | grep 'Front Left' | head -n1 || true")"
    if [[ -n "$pcm_line" ]]; then
        echo "  [${A_LABEL} audio] ${pcm_line}"
    fi
}

verify_audio_device_a() {
    local device_line
    device_line="$(ssh_a "aplay -L 2>/dev/null | grep -Fx '${A_AUDIO_DEVICE}' | head -n1 || true")"
    if [[ -z "$device_line" ]]; then
        echo "ERROR: side-A audio device '${A_AUDIO_DEVICE}' not found" >&2
        ssh_a "aplay -L 2>/dev/null | sed -n '1,20p'" || true
        exit 1
    fi
    echo "  [${A_LABEL} device] ${device_line}"
}

verify_audio_device_b() {
    if [[ -z "${B_AUDIO_DEVICE}" ]]; then
        echo "  [${B_LABEL} device] (default device)"
        return 0
    fi
    local device_line
    device_line="$(ssh_b "aplay -L 2>/dev/null | grep -Fx '${B_AUDIO_DEVICE}' | head -n1 || true")"
    if [[ -z "$device_line" ]]; then
        echo "ERROR: side-B audio device '${B_AUDIO_DEVICE}' not found" >&2
        ssh_b "aplay -L 2>/dev/null | sed -n '1,20p'" || true
        exit 1
    fi
    echo "  [${B_LABEL} device] ${device_line}"
}

verify_ptt_control_a() {
    echo "  [${A_LABEL} ptt] asserting rigctld PTT briefly"
    local ptt_on ptt_off
    ptt_on="$(ssh_a "rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} T 1 >/dev/null 2>&1 && sleep 1; rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} t 2>/dev/null | tail -n1 || echo na" || echo na)"
    ssh_a "rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} T 0 >/dev/null 2>&1 || true"
    ptt_off="$(ssh_a "rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} t 2>/dev/null | tail -n1 || echo na" || echo na)"
    echo "  [${A_LABEL} ptt] during=${ptt_on} after=${ptt_off}"
}

power_cycle_a() {
    echo "  [${A_LABEL}] power cycle via direct CAT (${A_CAT_PORT})"
    ssh_a "rigctl -m ${A_HAMLIB_MODEL} -r '${A_CAT_PORT}' -s ${A_CAT_BAUD} P 0 2>/dev/null || true"
    echo "  [${A_LABEL}] powering down (${POWER_OFF_WAIT}s)..."
    sleep "${POWER_OFF_WAIT}"
    ssh_a "rigctl -m ${A_HAMLIB_MODEL} -r '${A_CAT_PORT}' -s ${A_CAT_BAUD} P 1 2>/dev/null || true"
    echo "  [${A_LABEL}] powering up (${POWER_ON_WAIT}s)..."
    sleep "${POWER_ON_WAIT}"
    local freq
    freq="$(ssh_a "rigctl -m ${A_HAMLIB_MODEL} -r '${A_CAT_PORT}' -s ${A_CAT_BAUD} f 2>/dev/null | tail -n1 || echo na" || echo na)"
    if [[ "${freq}" == "na" || -z "${freq}" ]]; then
        echo "  ERROR: ${A_LABEL} not responding after power cycle" >&2
        exit 1
    fi
    echo "  [${A_LABEL}] power cycle OK (freq=${freq} Hz)"
}

power_cycle_b() {
    echo "  [${B_LABEL}] power cycle via direct CAT (${B_CAT_PORT})"
    ssh_b "rigctl -m ${B_HAMLIB_MODEL} -r '${B_CAT_PORT}' -s ${B_CAT_BAUD} P 0 2>/dev/null || true"
    echo "  [${B_LABEL}] powering down (${POWER_OFF_WAIT}s)..."
    sleep "${POWER_OFF_WAIT}"
    ssh_b "rigctl -m ${B_HAMLIB_MODEL} -r '${B_CAT_PORT}' -s ${B_CAT_BAUD} P 1 2>/dev/null || true"
    echo "  [${B_LABEL}] powering up (${POWER_ON_WAIT}s)..."
    sleep "${POWER_ON_WAIT}"
    local freq
    freq="$(ssh_b "rigctl -m ${B_HAMLIB_MODEL} -r '${B_CAT_PORT}' -s ${B_CAT_BAUD} f 2>/dev/null | tail -n1 || echo na" || echo na)"
    if [[ "${freq}" == "na" || -z "${freq}" ]]; then
        echo "  ERROR: ${B_LABEL} not responding after power cycle" >&2
        exit 1
    fi
    echo "  [${B_LABEL}] power cycle OK (freq=${freq} Hz)"
}

run_loopback_regression() {
    local tier="${1:-${LOOPBACK_TIER:-quick}}"
    local ar
    ar="$(a_repo)"

    # Stream the freshly-built binary from rpi51 to the loopback IRS (rpi52) so
    # both ends run identical code during the regression check.
    echo "  [loopback] deploying binary → ${LOOPBACK_IRS_SSH}:${LOOPBACK_IRS_BIN_DIR}"
    # shellcheck disable=SC2086
    ssh_a "tar -C '${ar}/target/release' -cf - openpulse" | \
        ssh ${SSH_OPTS} "${LOOPBACK_IRS_SSH}" \
            "mkdir -p '${LOOPBACK_IRS_BIN_DIR}' && \
             tar -C '${LOOPBACK_IRS_BIN_DIR}' -xf - && \
             chmod +x '${LOOPBACK_IRS_BIN_DIR}/openpulse'" \
        || { echo "  [loopback] FAIL: binary deploy to ${LOOPBACK_IRS_SSH} failed" >&2; return 1; }

    echo "  [loopback] rpi51↔rpi52 hardware loopback (${tier} tier)"
    local lb_exit=0
    ISS_BIN="${ar}/target/release/openpulse" \
    IRS_BIN="${LOOPBACK_IRS_BIN_DIR}/openpulse" \
    IRS_SSH="${LOOPBACK_IRS_SSH}" \
        "${REPO_ROOT}/scripts/run-loopback-rpi51-rpi52.sh" \
            "--${tier}" \
            --output "${OUTPUT_DIR}" \
        || lb_exit=$?
    if [[ ${lb_exit} -ne 0 ]]; then
        echo "  [loopback] FAIL (exit ${lb_exit}) — modem regression on rpi51↔rpi52" >&2
        return 1
    fi
}

read_swr_a() {
    ssh_a "rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} l SWR 2>/dev/null | tail -n1 || echo na"
}

read_swr_b() {
    ssh_b "rigctl -m 2 -r ${B_RIGCTLD_ADDR}:${B_RIGCTLD_PORT} l SWR 2>/dev/null | tail -n1 || echo na"
}

trigger_integrated_tuner_a() {
    local reason="$1"
    echo "  [${A_LABEL}] high SWR policy: attempting integrated tuner (${reason})"
    ssh_a "set +e; \
        rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} U TUNER 1 >/dev/null 2>&1 && exit 0; \
        rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} U TUNER ON >/dev/null 2>&1 && exit 0; \
        rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} U TUNER START >/dev/null 2>&1 && exit 0; \
        exit 1" >/dev/null 2>&1 || {
            echo "  [${A_LABEL}] tuner command not supported or failed"
            return 1
        }
    return 0
}

trigger_integrated_tuner_b() {
    local reason="$1"
    echo "  [${B_LABEL}] high SWR policy: attempting integrated tuner (${reason})"
    ssh_b "set +e; \
        rigctl -m 2 -r ${B_RIGCTLD_ADDR}:${B_RIGCTLD_PORT} U TUNER 1 >/dev/null 2>&1 && exit 0; \
        rigctl -m 2 -r ${B_RIGCTLD_ADDR}:${B_RIGCTLD_PORT} U TUNER ON >/dev/null 2>&1 && exit 0; \
        rigctl -m 2 -r ${B_RIGCTLD_ADDR}:${B_RIGCTLD_PORT} U TUNER START >/dev/null 2>&1 && exit 0; \
        exit 1" >/dev/null 2>&1 || {
            echo "  [${B_LABEL}] tuner command not supported or failed"
            return 1
        }
    return 0
}

maybe_tune_high_swr_a() {
    local reason="$1"
    if ! is_truthy "$ALLOW_TUNER_ON_HIGH_SWR"; then
        return 0
    fi
    if [[ "$reason" == "qsy" ]]; then
        if ! is_truthy "$QSY_MODE_ENABLED" || ! is_truthy "$TUNER_TRIGGER_ON_QSY"; then
            return 0
        fi
    fi

    local swr
    swr="$(read_swr_a || echo na)"
    if ! [[ "$swr" =~ ^[0-9]+([.][0-9]+)?$ ]]; then
        echo "  [${A_LABEL}] SWR unavailable; skipping tuner policy"
        return 0
    fi

    local high
    high="$(awk -v v="$swr" -v t="$HIGH_SWR_THRESHOLD" 'BEGIN{if (v>t) print 1; else print 0}')"
    if [[ "$high" == "1" ]]; then
        echo "  [${A_LABEL}] SWR=${swr} > ${HIGH_SWR_THRESHOLD}"
        trigger_integrated_tuner_a "$reason" || true
    else
        echo "  [${A_LABEL}] SWR=${swr} <= ${HIGH_SWR_THRESHOLD}; tuner not needed"
    fi
}

maybe_tune_high_swr_b() {
    local reason="$1"
    if ! is_truthy "$ALLOW_TUNER_ON_HIGH_SWR"; then
        return 0
    fi
    if [[ "$reason" == "qsy" ]]; then
        if ! is_truthy "$QSY_MODE_ENABLED" || ! is_truthy "$TUNER_TRIGGER_ON_QSY"; then
            return 0
        fi
    fi

    local swr
    swr="$(read_swr_b || echo na)"
    if ! [[ "$swr" =~ ^[0-9]+([.][0-9]+)?$ ]]; then
        echo "  [${B_LABEL}] SWR unavailable; skipping tuner policy"
        return 0
    fi

    local high
    high="$(awk -v v="$swr" -v t="$HIGH_SWR_THRESHOLD" 'BEGIN{if (v>t) print 1; else print 0}')"
    if [[ "$high" == "1" ]]; then
        echo "  [${B_LABEL}] SWR=${swr} > ${HIGH_SWR_THRESHOLD}"
        trigger_integrated_tuner_b "$reason" || true
    else
        echo "  [${B_LABEL}] SWR=${swr} <= ${HIGH_SWR_THRESHOLD}; tuner not needed"
    fi
}

check_ssh_agent() {
    if ! ssh-add -l >/dev/null 2>&1; then
        echo "ssh-agent has no identities loaded; run: ssh-add" >&2
        exit 1
    fi
}

start_rigctld_a() {
    echo "  [${A_LABEL} rigctld] starting on A"
    local a_ptt_type_upper
    a_ptt_type_upper="$(printf '%s' "${A_PTT_TYPE}" | tr '[:lower:]' '[:upper:]')"
    local a_ptt_args=""
    if [[ "${a_ptt_type_upper}" != "CAT" && "${a_ptt_type_upper}" != "NONE" ]]; then
        a_ptt_args="-p ${A_PTT_PORT} -P ${A_PTT_TYPE}"
    fi
    ssh_a "pkill -x rigctld 2>/dev/null || true; sleep 0.5; \
        nohup rigctld \
            -m ${A_HAMLIB_MODEL} \
            -r ${A_CAT_PORT} \
            -s ${A_CAT_BAUD} \
            ${a_ptt_args} \
            -t ${A_RIGCTLD_PORT} \
            </dev/null >/tmp/ic9700-rigctld.log 2>&1 &
        sleep 1
        pgrep -x rigctld >/dev/null && echo 'ok' || { echo 'fail'; exit 1; }"
}

stop_rigctld_a() {
    echo "  [${A_LABEL} rigctld] stopping on A"
    ssh_a "pkill -x rigctld 2>/dev/null || true" || true
}

start_rigctld_b() {
    echo "  [${B_LABEL} rigctld] starting on B"
    local b_ptt_type_upper
    b_ptt_type_upper="$(printf '%s' "${B_PTT_TYPE}" | tr '[:lower:]' '[:upper:]')"
    local b_ptt_args=""
    if [[ "${b_ptt_type_upper}" != "CAT" && "${b_ptt_type_upper}" != "NONE" ]]; then
        b_ptt_args="-p ${B_PTT_PORT} -P ${B_PTT_TYPE}"
    fi
    ssh_b "pkill -x rigctld 2>/dev/null || true; sleep 0.5; \
        nohup rigctld \
            -m ${B_HAMLIB_MODEL} \
            -r ${B_CAT_PORT} \
            -s ${B_CAT_BAUD} \
            ${b_ptt_args} \
            -t ${B_RIGCTLD_PORT} \
            </dev/null >/tmp/ft991a-rigctld.log 2>&1 &
        sleep 1
        pgrep -x rigctld >/dev/null && echo 'ok' || { echo 'fail'; exit 1; }"
}

stop_rigctld_b() {
    echo "  [${B_LABEL} rigctld] stopping on B"
    ssh_b "pkill -x rigctld 2>/dev/null || true" || true
}

tune_a() {
    echo "  [tune] ${A_LABEL} (A) -> ${TEST_FREQ_HZ} Hz ${TEST_MODE_RIG_A}"
    ssh_a "rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} F ${TEST_FREQ_HZ} 2>/dev/null || true; \
        rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} M ${TEST_MODE_RIG_A} 2400 2>/dev/null || true"
}

tune_b() {
    echo "  [tune] ${B_LABEL} (B) -> ${TEST_FREQ_HZ} Hz ${TEST_MODE_RIG_B}"
    ssh_b "rigctl -m 2 -r ${B_RIGCTLD_ADDR}:${B_RIGCTLD_PORT} F ${TEST_FREQ_HZ} 2>/dev/null || true; \
        rigctl -m 2 -r ${B_RIGCTLD_ADDR}:${B_RIGCTLD_PORT} M ${TEST_MODE_RIG_B} 2400 2>/dev/null || true"
}

save_rig_state_a() {
    A_SAVED_FREQ="$(ssh_a "rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} f 2>/dev/null | tail -n1 || echo na" || echo na)"
    A_SAVED_MODE="$(ssh_a "rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} m 2>/dev/null | sed -n '1p' || echo na" || echo na)"
    A_SAVED_RFPOWER="$(ssh_a "rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} l RFPOWER 2>/dev/null | tail -n1 || echo na" || echo na)"
    A_SAVED_MICGAIN="$(ssh_a "rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} l MICGAIN 2>/dev/null | tail -n1 || echo na" || echo na)"
    A_SAVED_COMP="$(ssh_a "rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} l COMP 2>/dev/null | tail -n1 || echo na" || echo na)"
}

save_rig_state_b() {
    B_SAVED_FREQ="$(ssh_b "rigctl -m 2 -r ${B_RIGCTLD_ADDR}:${B_RIGCTLD_PORT} f 2>/dev/null | tail -n1 || echo na" || echo na)"
    B_SAVED_MODE="$(ssh_b "rigctl -m 2 -r ${B_RIGCTLD_ADDR}:${B_RIGCTLD_PORT} m 2>/dev/null | sed -n '1p' || echo na" || echo na)"
    B_SAVED_RFPOWER="$(ssh_b "rigctl -m 2 -r ${B_RIGCTLD_ADDR}:${B_RIGCTLD_PORT} l RFPOWER 2>/dev/null | tail -n1 || echo na" || echo na)"
    B_SAVED_MICGAIN="$(ssh_b "rigctl -m 2 -r ${B_RIGCTLD_ADDR}:${B_RIGCTLD_PORT} l MICGAIN 2>/dev/null | tail -n1 || echo na" || echo na)"
    B_SAVED_COMP="$(ssh_b "rigctl -m 2 -r ${B_RIGCTLD_ADDR}:${B_RIGCTLD_PORT} l COMP 2>/dev/null | tail -n1 || echo na" || echo na)"
}

apply_known_good_settings_a() {
    ssh_a "rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} L RFPOWER ${A_RFPOWER} >/dev/null 2>&1 || true; \
        rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} L COMP 0 >/dev/null 2>&1 || true; \
        rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} L MICGAIN 0.75 >/dev/null 2>&1 || true"
}

apply_known_good_settings_b() {
    ssh_b "rigctl -m 2 -r ${B_RIGCTLD_ADDR}:${B_RIGCTLD_PORT} L RFPOWER ${B_RFPOWER} >/dev/null 2>&1 || true; \
        rigctl -m 2 -r ${B_RIGCTLD_ADDR}:${B_RIGCTLD_PORT} L COMP 0 >/dev/null 2>&1 || true; \
        rigctl -m 2 -r ${B_RIGCTLD_ADDR}:${B_RIGCTLD_PORT} L MICGAIN 0.75 >/dev/null 2>&1 || true"
}

restore_rig_state_a() {
    [[ -n "$A_SAVED_FREQ" && "$A_SAVED_FREQ" != "na" ]] && ssh_a "rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} F ${A_SAVED_FREQ} >/dev/null 2>&1 || true"
    [[ -n "$A_SAVED_MODE" && "$A_SAVED_MODE" != "na" ]] && ssh_a "rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} M ${A_SAVED_MODE} 2400 >/dev/null 2>&1 || true"
    [[ -n "$A_SAVED_RFPOWER" && "$A_SAVED_RFPOWER" != "na" ]] && ssh_a "rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} L RFPOWER ${A_SAVED_RFPOWER} >/dev/null 2>&1 || true"
    [[ -n "$A_SAVED_MICGAIN" && "$A_SAVED_MICGAIN" != "na" ]] && ssh_a "rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} L MICGAIN ${A_SAVED_MICGAIN} >/dev/null 2>&1 || true"
    [[ -n "$A_SAVED_COMP" && "$A_SAVED_COMP" != "na" ]] && ssh_a "rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} L COMP ${A_SAVED_COMP} >/dev/null 2>&1 || true"
}

restore_rig_state_b() {
    [[ -n "$B_SAVED_FREQ" && "$B_SAVED_FREQ" != "na" ]] && ssh_b "rigctl -m 2 -r ${B_RIGCTLD_ADDR}:${B_RIGCTLD_PORT} F ${B_SAVED_FREQ} >/dev/null 2>&1 || true"
    [[ -n "$B_SAVED_MODE" && "$B_SAVED_MODE" != "na" ]] && ssh_b "rigctl -m 2 -r ${B_RIGCTLD_ADDR}:${B_RIGCTLD_PORT} M ${B_SAVED_MODE} 2400 >/dev/null 2>&1 || true"
    [[ -n "$B_SAVED_RFPOWER" && "$B_SAVED_RFPOWER" != "na" ]] && ssh_b "rigctl -m 2 -r ${B_RIGCTLD_ADDR}:${B_RIGCTLD_PORT} L RFPOWER ${B_SAVED_RFPOWER} >/dev/null 2>&1 || true"
    [[ -n "$B_SAVED_MICGAIN" && "$B_SAVED_MICGAIN" != "na" ]] && ssh_b "rigctl -m 2 -r ${B_RIGCTLD_ADDR}:${B_RIGCTLD_PORT} L MICGAIN ${B_SAVED_MICGAIN} >/dev/null 2>&1 || true"
    [[ -n "$B_SAVED_COMP" && "$B_SAVED_COMP" != "na" ]] && ssh_b "rigctl -m 2 -r ${B_RIGCTLD_ADDR}:${B_RIGCTLD_PORT} L COMP ${B_SAVED_COMP} >/dev/null 2>&1 || true"
}

build_on_a() {
    echo "==> Building cpal-enabled binaries on Station A (${A_SSH})"
    local repo_dir
    local cargo_bin
    repo_dir="$(a_repo)"
    cargo_bin="$(a_cargo)"
    if [[ -z "${cargo_bin}" ]]; then
        echo "ERROR: cargo missing on Station A" >&2
        exit 1
    fi

    ssh_a "cd '${repo_dir}' && \
        git pull --ff-only 2>&1 | tail -3 && \
        '${cargo_bin}' build --release -p openpulse-cli --features cpal-backend >/tmp/openpulse-cli-build.log 2>&1 || { tail -30 /tmp/openpulse-cli-build.log; exit 1; } && \
        tail -3 /tmp/openpulse-cli-build.log && \
        '${cargo_bin}' build --release -p openpulse-ardop --features cpal >/tmp/openpulse-ardop-build.log 2>&1 || { tail -30 /tmp/openpulse-ardop-build.log; exit 1; } && \
        tail -3 /tmp/openpulse-ardop-build.log && \
        echo '[build] Station A binaries ready'"
}

transfer_binaries_a_to_b() {
    echo "==> Transferring binaries A -> B (preserving repo-like layout)"
    local ar
    local br
    ar="$(a_repo)"
    br="$(b_repo)"

    ssh_b "mkdir -p '${br}/target/release' '${br}/scripts'"

    # Stream tar over ssh to avoid writing extra temp files on orchestrator or B.
    ssh_a "tar -C '${ar}/target/release' -cf - openpulse openpulse-tnc" | \
        ssh_b "tar -C '${br}/target/release' -xf -"

    # Keep the path shape expected by scripts on B even without git metadata.
    ssh_b "test -x '${br}/target/release/openpulse' && test -x '${br}/target/release/openpulse-tnc'"
    echo "  [transfer] deployed binaries to ${br}/target/release"
}

status_a() {
    echo "-- Station A (${A_LABEL} / ISS) --"
    local ar
    ar="$(a_repo)"
    ssh_a "
        printf '  openpulse:       '; test -x '${ar}/target/release/openpulse' && echo present || echo MISSING
        printf '  openpulse-tnc:   '; test -x '${ar}/target/release/openpulse-tnc' && echo present || echo MISSING
        printf '  rigctld running: '; pgrep -x rigctld >/dev/null 2>&1 && echo yes || echo no
        printf '  CAT serial:      '; test -e '${A_CAT_PORT}' && echo present || echo MISSING
    " || echo "  (SSH to A failed)"
}

status_b() {
    echo "-- Station B (${B_LABEL} / IRS) --"
    local br
    local bl
    br="$(b_repo)"
    bl="$(b_logdir)"
    ssh_b "
        printf '  openpulse:       '; test -x '${br}/target/release/openpulse' && echo present || echo MISSING
        printf '  openpulse-tnc:   '; test -x '${br}/target/release/openpulse-tnc' && echo present || echo MISSING
        printf '  rigctld running: '; pgrep -x rigctld >/dev/null 2>&1 && echo yes || echo no
        printf '  CAT serial:      '; test -e '${B_CAT_PORT}' && echo present || echo MISSING
        printf '  log dir:         '; test -d '${bl}' && echo present || echo missing
    " || echo "  (SSH to B failed)"
}

cleanup_all() {
    echo "==> Cleanup"
    restore_rig_state_a
    restore_rig_state_b
    stop_rigctld_a
    stop_rigctld_b
    ssh_a "pkill -f 'openpulse receive|openpulse transmit|openpulse-tnc|openpulse-kisstnc' 2>/dev/null || true" || true
    ssh_b "pkill -f 'openpulse receive|openpulse transmit|openpulse-tnc|openpulse-kisstnc' 2>/dev/null || true" || true
    echo "  done"
}

preflight_check() {
    echo "==> Pre-flight rig check"
    local fail=0
    local rc_a="rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT}"
    local rc_b="rigctl -m 2 -r ${B_RIGCTLD_ADDR}:${B_RIGCTLD_PORT}"

    # Apply corrections on both stations before readback (best-effort).
    echo "  Applying corrections (COMP/NB/NR/SQL/VOX=0)..."
    ssh_a "${rc_a} L COMP 0 2>/dev/null || true; \
           ${rc_a} L NB   0 2>/dev/null || true; \
           ${rc_a} L NR   0 2>/dev/null || true; \
           ${rc_a} L SQL  0 2>/dev/null || true; \
           ${rc_a} L VOX  0 2>/dev/null || true" 2>/dev/null || true
    ssh_b "${rc_b} L COMP 0 2>/dev/null || true; \
           ${rc_b} L NB   0 2>/dev/null || true; \
           ${rc_b} L NR   0 2>/dev/null || true; \
           ${rc_b} L SQL  0 2>/dev/null || true; \
           ${rc_b} L VOX  0 2>/dev/null || true" 2>/dev/null || true

    # Batch-read all rig state + audio in a single SSH session per station.
    # Each line of output is KEY:VALUE so we can parse safely even if some
    # rigctl calls return empty or multi-line results.
    local a_raw
    a_raw="$(ssh_a "
        _mode_out=\$(${rc_a} m 2>/dev/null | grep -v Hamlib || echo na)
        _sink_vol=\$(pactl get-sink-volume @DEFAULT_SINK@ 2>/dev/null \
            | grep -o '[0-9]*%' | head -n1 | tr -d '%' || echo na)
        _sink_mute=\$(pactl get-sink-mute @DEFAULT_SINK@ 2>/dev/null \
            | awk '{print \$2}' || echo na)
        printf 'FREQ:%s\n'     \"\$(${rc_a} f        2>/dev/null | grep -v Hamlib | tail -n1 || echo na)\"
        printf 'MODE:%s\n'     \"\$(printf '%s' \"\$_mode_out\" | head -n1)\"
        printf 'PASSBAND:%s\n' \"\$(printf '%s' \"\$_mode_out\" | sed -n '2p')\"
        printf 'RFPOWER:%s\n'  \"\$(${rc_a} l RFPOWER  2>/dev/null | grep -v Hamlib | tail -n1 || echo na)\"
        printf 'COMP:%s\n'     \"\$(${rc_a} l COMP     2>/dev/null | grep -v Hamlib | tail -n1 || echo na)\"
        printf 'NB:%s\n'       \"\$(${rc_a} l NB       2>/dev/null | grep -v Hamlib | tail -n1 || echo na)\"
        printf 'NR:%s\n'       \"\$(${rc_a} l NR       2>/dev/null | grep -v Hamlib | tail -n1 || echo na)\"
        printf 'SQL:%s\n'      \"\$(${rc_a} l SQL      2>/dev/null | grep -v Hamlib | tail -n1 || echo na)\"
        printf 'VOX:%s\n'      \"\$(${rc_a} l VOX      2>/dev/null | grep -v Hamlib | tail -n1 || echo na)\"
        printf 'RFGAIN:%s\n'   \"\$(${rc_a} l RFGAIN   2>/dev/null | grep -v Hamlib | tail -n1 || echo na)\"
        printf 'PREAMP:%s\n'   \"\$(${rc_a} l PREAMP   2>/dev/null | grep -v Hamlib | tail -n1 || echo na)\"
        printf 'SWR:%s\n'      \"\$(${rc_a} l SWR      2>/dev/null | grep -v Hamlib | tail -n1 || echo na)\"
        printf 'STRENGTH:%s\n' \"\$(${rc_a} l STRENGTH 2>/dev/null | grep -v Hamlib | tail -n1 || echo na)\"
        printf 'SINKVOL:%s\n'  \"\${_sink_vol:-na}\"
        printf 'SINKMUTE:%s\n' \"\${_sink_mute:-na}\"
    " 2>/dev/null || echo "")"

    local b_raw
    b_raw="$(ssh_b "
        _mode_out=\$(${rc_b} m 2>/dev/null | grep -v Hamlib || echo na)
        _src_vol=\$(pactl get-source-volume @DEFAULT_SOURCE@ 2>/dev/null \
            | grep -o '[0-9]*%' | head -n1 | tr -d '%' || echo na)
        _src_mute=\$(pactl get-source-mute @DEFAULT_SOURCE@ 2>/dev/null \
            | awk '{print \$2}' || echo na)
        printf 'FREQ:%s\n'     \"\$(${rc_b} f        2>/dev/null | grep -v Hamlib | tail -n1 || echo na)\"
        printf 'MODE:%s\n'     \"\$(printf '%s' \"\$_mode_out\" | head -n1)\"
        printf 'PASSBAND:%s\n' \"\$(printf '%s' \"\$_mode_out\" | sed -n '2p')\"
        printf 'RFPOWER:%s\n'  \"\$(${rc_b} l RFPOWER  2>/dev/null | grep -v Hamlib | tail -n1 || echo na)\"
        printf 'COMP:%s\n'     \"\$(${rc_b} l COMP     2>/dev/null | grep -v Hamlib | tail -n1 || echo na)\"
        printf 'NB:%s\n'       \"\$(${rc_b} l NB       2>/dev/null | grep -v Hamlib | tail -n1 || echo na)\"
        printf 'NR:%s\n'       \"\$(${rc_b} l NR       2>/dev/null | grep -v Hamlib | tail -n1 || echo na)\"
        printf 'SQL:%s\n'      \"\$(${rc_b} l SQL      2>/dev/null | grep -v Hamlib | tail -n1 || echo na)\"
        printf 'VOX:%s\n'      \"\$(${rc_b} l VOX      2>/dev/null | grep -v Hamlib | tail -n1 || echo na)\"
        printf 'RFGAIN:%s\n'   \"\$(${rc_b} l RFGAIN   2>/dev/null | grep -v Hamlib | tail -n1 || echo na)\"
        printf 'PREAMP:%s\n'   \"\$(${rc_b} l PREAMP   2>/dev/null | grep -v Hamlib | tail -n1 || echo na)\"
        printf 'SWR:%s\n'      \"\$(${rc_b} l SWR      2>/dev/null | grep -v Hamlib | tail -n1 || echo na)\"
        printf 'STRENGTH:%s\n' \"\$(${rc_b} l STRENGTH 2>/dev/null | grep -v Hamlib | tail -n1 || echo na)\"
        printf 'SRCVOL:%s\n'   \"\${_src_vol:-na}\"
        printf 'SRCMUTE:%s\n'  \"\${_src_mute:-na}\"
    " 2>/dev/null || echo "")"

    # Extract KEY:VALUE from raw multi-line output.
    _pf_val() { printf '%s' "${1}" | sed -n "s/^${2}://p" | head -n1; }
    _pf_lt()  { awk -v v="${1}" -v t="${2}" 'BEGIN{print (v+0 < t+0) ? 1 : 0}'; }
    _pf_gt()  { awk -v v="${1}" -v t="${2}" 'BEGIN{print (v+0 > t+0) ? 1 : 0}'; }

    # Parse Station A.
    local a_freq a_mode a_passband a_rfpower a_comp a_nb a_nr a_sql a_vox
    local a_rfgain a_preamp a_swr a_strength a_sinkvol a_sinkmute
    a_freq="$(_pf_val "$a_raw" FREQ)";         a_freq="${a_freq:-na}"
    a_mode="$(_pf_val "$a_raw" MODE)";         a_mode="${a_mode:-na}"
    a_passband="$(_pf_val "$a_raw" PASSBAND)"; a_passband="${a_passband:-na}"
    a_rfpower="$(_pf_val "$a_raw" RFPOWER)";   a_rfpower="${a_rfpower:-na}"
    a_comp="$(_pf_val "$a_raw" COMP)";         a_comp="${a_comp:-na}"
    a_nb="$(_pf_val "$a_raw" NB)";             a_nb="${a_nb:-na}"
    a_nr="$(_pf_val "$a_raw" NR)";             a_nr="${a_nr:-na}"
    a_sql="$(_pf_val "$a_raw" SQL)";           a_sql="${a_sql:-na}"
    a_vox="$(_pf_val "$a_raw" VOX)";           a_vox="${a_vox:-na}"
    a_rfgain="$(_pf_val "$a_raw" RFGAIN)";     a_rfgain="${a_rfgain:-na}"
    a_preamp="$(_pf_val "$a_raw" PREAMP)";     a_preamp="${a_preamp:-na}"
    a_swr="$(_pf_val "$a_raw" SWR)";           a_swr="${a_swr:-na}"
    a_strength="$(_pf_val "$a_raw" STRENGTH)"; a_strength="${a_strength:-na}"
    a_sinkvol="$(_pf_val "$a_raw" SINKVOL)";   a_sinkvol="${a_sinkvol:-na}"
    a_sinkmute="$(_pf_val "$a_raw" SINKMUTE)"; a_sinkmute="${a_sinkmute:-na}"

    # Parse Station B.
    local b_freq b_mode b_passband b_rfpower b_comp b_nb b_nr b_sql b_vox
    local b_rfgain b_preamp b_swr b_strength b_srcvol b_srcmute
    b_freq="$(_pf_val "$b_raw" FREQ)";         b_freq="${b_freq:-na}"
    b_mode="$(_pf_val "$b_raw" MODE)";         b_mode="${b_mode:-na}"
    b_passband="$(_pf_val "$b_raw" PASSBAND)"; b_passband="${b_passband:-na}"
    b_rfpower="$(_pf_val "$b_raw" RFPOWER)";   b_rfpower="${b_rfpower:-na}"
    b_comp="$(_pf_val "$b_raw" COMP)";         b_comp="${b_comp:-na}"
    b_nb="$(_pf_val "$b_raw" NB)";             b_nb="${b_nb:-na}"
    b_nr="$(_pf_val "$b_raw" NR)";             b_nr="${b_nr:-na}"
    b_sql="$(_pf_val "$b_raw" SQL)";           b_sql="${b_sql:-na}"
    b_vox="$(_pf_val "$b_raw" VOX)";           b_vox="${b_vox:-na}"
    b_rfgain="$(_pf_val "$b_raw" RFGAIN)";     b_rfgain="${b_rfgain:-na}"
    b_preamp="$(_pf_val "$b_raw" PREAMP)";     b_preamp="${b_preamp:-na}"
    b_swr="$(_pf_val "$b_raw" SWR)";           b_swr="${b_swr:-na}"
    b_strength="$(_pf_val "$b_raw" STRENGTH)"; b_strength="${b_strength:-na}"
    b_srcvol="$(_pf_val "$b_raw" SRCVOL)";     b_srcvol="${b_srcvol:-na}"
    b_srcmute="$(_pf_val "$b_raw" SRCMUTE)";   b_srcmute="${b_srcmute:-na}"

    # Display per-station summary.
    echo ""
    echo "  ${A_LABEL} (ISS / TX):"
    echo "    freq     = ${a_freq} Hz  [expected ${TEST_FREQ_HZ}]"
    echo "    mode     = ${a_mode}  passband = ${a_passband} Hz"
    echo "    rfpower  = ${a_rfpower}  comp=${a_comp}  nb=${a_nb}  nr=${a_nr}  sql=${a_sql}  vox=${a_vox}"
    echo "    rfgain   = ${a_rfgain}  preamp=${a_preamp}  swr=${a_swr}  strength=${a_strength} dBm"
    echo "    audio TX = ${a_sinkvol}%  mute=${a_sinkmute}"
    echo ""
    echo "  ${B_LABEL} (IRS / RX):"
    echo "    freq     = ${b_freq} Hz  [expected ${TEST_FREQ_HZ}]"
    echo "    mode     = ${b_mode}  passband = ${b_passband} Hz"
    echo "    rfpower  = ${b_rfpower}  comp=${b_comp}  nb=${b_nb}  nr=${b_nr}  sql=${b_sql}  vox=${b_vox}"
    echo "    rfgain   = ${b_rfgain}  preamp=${b_preamp}  swr=${b_swr}  strength=${b_strength} dBm"
    echo "    audio RX = ${b_srcvol}%  mute=${b_srcmute}"
    echo ""

    # --- CRITICAL: frequency must match on both stations ---
    for _entry in "${A_LABEL}:${a_freq}" "${B_LABEL}:${b_freq}"; do
        local _lbl="${_entry%%:*}" _val="${_entry#*:}"
        if [[ "$_val" != "na" && "$_val" != "${TEST_FREQ_HZ}" ]]; then
            echo "  ERROR: ${_lbl} frequency ${_val} != expected ${TEST_FREQ_HZ}" >&2
            fail=1
        fi
    done

    # --- CRITICAL: RF power must be >= 1% ---
    for _entry in "${A_LABEL}:${a_rfpower}" "${B_LABEL}:${b_rfpower}"; do
        local _lbl="${_entry%%:*}" _val="${_entry#*:}"
        if [[ "$_val" != "na" ]] && [[ "$(_pf_lt "$_val" 0.01)" == "1" ]]; then
            echo "  ERROR: ${_lbl} rfpower=${_val} is effectively 0 — set A_RFPOWER/B_RFPOWER in the profile" >&2
            fail=1
        fi
    done

    # --- CRITICAL: TX audio output must not be muted ---
    if [[ "$a_sinkmute" == "yes" ]]; then
        echo "  ERROR: ${A_LABEL} PulseAudio output is muted — no audio will reach the radio" >&2
        fail=1
    fi

    # --- CRITICAL: RX capture must not be muted ---
    if [[ "$b_srcmute" == "yes" ]]; then
        echo "  ERROR: ${B_LABEL} PulseAudio capture is muted — received audio cannot be decoded" >&2
        fail=1
    fi

    # --- WARN: mode should match TEST_MODE_RIG ---
    for _entry in "${A_LABEL}:${a_mode}" "${B_LABEL}:${b_mode}"; do
        local _lbl="${_entry%%:*}" _val="${_entry#*:}"
        if [[ "$_val" != "na" && "$_val" != "${TEST_MODE_RIG}" && "$_val" != "USB" ]]; then
            echo "  WARN: ${_lbl} mode=${_val} — expected ${TEST_MODE_RIG} or USB" >&2
        fi
    done

    # --- WARN: passband should be >= 2400 Hz for digital modes ---
    for _entry in "${A_LABEL}:${a_passband}" "${B_LABEL}:${b_passband}"; do
        local _lbl="${_entry%%:*}" _val="${_entry#*:}"
        if [[ "$_val" != "na" && -n "$_val" ]] && [[ "$(_pf_lt "$_val" 2400)" == "1" ]]; then
            echo "  WARN: ${_lbl} passband=${_val} Hz — digital modes need >= 2400 Hz" >&2
        fi
    done

    # --- WARN: TX audio output level too low ---
    if [[ "$a_sinkvol" != "na" && -n "$a_sinkvol" ]] && [[ "$(_pf_lt "$a_sinkvol" 20)" == "1" ]]; then
        echo "  WARN: ${A_LABEL} PulseAudio output volume=${a_sinkvol}% — may be too low for TX audio" >&2
    fi

    # --- WARN: RX capture level too low ---
    if [[ "$b_srcvol" != "na" && -n "$b_srcvol" ]] && [[ "$(_pf_lt "$b_srcvol" 20)" == "1" ]]; then
        echo "  WARN: ${B_LABEL} PulseAudio capture volume=${b_srcvol}% — may be too low to decode" >&2
    fi

    # --- WARN: squelch active (gates received audio) ---
    for _entry in "${A_LABEL}:${a_sql}" "${B_LABEL}:${b_sql}"; do
        local _lbl="${_entry%%:*}" _val="${_entry#*:}"
        if [[ "$_val" != "na" ]] && [[ "$(_pf_gt "$_val" 0.1)" == "1" ]]; then
            echo "  WARN: ${_lbl} sql=${_val} — squelch active, may gate received audio" >&2
        fi
    done

    # --- WARN: noise blanker / noise reduction still active after correction ---
    for _entry in "${A_LABEL}:NB:${a_nb}" "${A_LABEL}:NR:${a_nr}" \
                  "${B_LABEL}:NB:${b_nb}" "${B_LABEL}:NR:${b_nr}"; do
        local _lbl="${_entry%%:*}"
        local _key; _key="$(printf '%s' "$_entry" | cut -d: -f2)"
        local _val; _val="$(printf '%s' "$_entry" | cut -d: -f3)"
        if [[ "$_val" != "na" ]] && [[ "$(_pf_gt "$_val" 0.0)" == "1" ]]; then
            echo "  WARN: ${_lbl} ${_key}=${_val} — DSP filter still active after correction attempt" >&2
        fi
    done

    # --- WARN: VOX still on after correction ---
    for _entry in "${A_LABEL}:${a_vox}" "${B_LABEL}:${b_vox}"; do
        local _lbl="${_entry%%:*}" _val="${_entry#*:}"
        if [[ "$_val" != "na" ]] && [[ "$(_pf_gt "$_val" 0.0)" == "1" ]]; then
            echo "  WARN: ${_lbl} vox=${_val} — VOX still active after correction attempt" >&2
        fi
    done

    if [[ "$fail" == "1" ]]; then
        echo "  Pre-flight check FAILED — aborting test run" >&2
        exit 1
    fi
    echo "  Pre-flight OK"
}

setup() {
    check_ssh_agent
    local bl
    bl="$(b_logdir)"

    mkdir -p "$OUTPUT_DIR"
    ssh_b "mkdir -p '${bl}'"

    build_on_a
    transfer_binaries_a_to_b
    run_loopback_regression full

    ssh_a "command -v rigctld >/dev/null || { echo 'ERROR: rigctld missing on Station A'; exit 1; }"
    ssh_b "command -v rigctld >/dev/null || { echo 'ERROR: rigctld missing on Station B'; exit 1; }"

    if [[ -n "${A_AUDIO_DEVICE}" ]]; then
        verify_audio_device_a
    fi
    verify_audio_device_b

    if is_truthy "${POWER_CYCLE_ENABLE}"; then
        power_cycle_a
        power_cycle_b
    fi

    start_rigctld_a
    start_rigctld_b
    sleep 1
    save_rig_state_a
    save_rig_state_b
    apply_known_good_settings_a
    apply_known_good_settings_b
    tune_a
    tune_b
    maybe_tune_high_swr_a "startup"
    maybe_tune_high_swr_b "startup"
    maybe_tune_high_swr_a "qsy"
    maybe_tune_high_swr_b "qsy"

    echo "==> Setup complete"
    status_a
    status_b
}

setup_side_a() {
    check_ssh_agent
    mkdir -p "$OUTPUT_DIR"

    build_on_a
    run_loopback_regression full

    ssh_a "command -v rigctld >/dev/null || { echo 'ERROR: rigctld missing on Station A'; exit 1; }"

    if is_truthy "${POWER_CYCLE_ENABLE}"; then
        power_cycle_a
    fi
    start_rigctld_a
    sleep 1
    save_rig_state_a
    apply_known_good_settings_a
    verify_audio_device_a
    verify_ptt_control_a
    verify_audio_codec_a
    tune_a
    maybe_tune_high_swr_a "startup"
    maybe_tune_high_swr_a "qsy"

    echo "==> Side-A setup complete"
    status_a
}

cleanup_side_a() {
    echo "==> Cleanup"
    restore_rig_state_a
    stop_rigctld_a
    ssh_a "pkill -f 'openpulse receive|openpulse transmit|openpulse-tnc|openpulse-kisstnc' 2>/dev/null || true" || true
    echo "  done"
}

run_side_a_transmit() {
    local case_spec normalized_case_spec MODE FEC PAYLOAD_SIZE
    case_spec="$SIDE_A_SINGLE_CASE"
    if [[ -n "$SINGLE_CASE" ]]; then
        case_spec="$SINGLE_CASE"
    fi

    normalized_case_spec="$case_spec"
    if [[ "$normalized_case_spec" == *,* && "$normalized_case_spec" != *"|"* ]]; then
        normalized_case_spec="${normalized_case_spec//,/|}"
    fi

    IFS='|' read -r MODE FEC PAYLOAD_SIZE <<< "$normalized_case_spec"
    if [[ -z "${MODE}" || -z "${PAYLOAD_SIZE}" ]]; then
        echo "FAIL (invalid side-A case format; expected MODE|FEC|PAYLOAD_BYTES or MODE|PAYLOAD_BYTES)"
        return 1
    fi
    if [[ -z "${FEC}" ]]; then
        FEC="none"
    fi
    if ! [[ "$PAYLOAD_SIZE" =~ ^[0-9]+$ ]] || (( PAYLOAD_SIZE < 1 || PAYLOAD_SIZE > 255 )); then
        echo "FAIL (invalid payload size '${PAYLOAD_SIZE}'; must be 1..255)"
        return 1
    fi

    local ts git_sha report
    local ar
    ts="$(date -u +%Y-%m-%dT%H%M%S)"
    git_sha="$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")"
    report="${OUTPUT_DIR}/side-a-${ts}.json"
    mkdir -p "$OUTPUT_DIR"
    ar="$(a_repo)"

    local a_iss_log="/tmp/openpulse-side-a-${MODE}-${FEC}.log"
    local a_tel_log="/tmp/openpulse-side-a-telemetry-${MODE}-${FEC}.log"
    local a_device_arg=""
    if [[ -n "${A_AUDIO_DEVICE}" ]]; then
        a_device_arg="--device '${A_AUDIO_DEVICE}'"
    fi

    local payload_text
    payload_text="$(python3 -c "import secrets, string, sys; a = string.ascii_letters + string.digits; sys.stdout.write(''.join(secrets.choice(a) for _ in range(${PAYLOAD_SIZE})))")"

    local iss_exit=0
    local telemetry_summary=""
    local tel_ptt_on="na"
    local tel_alc_nonzero="na"
    local tel_rfm_nonzero="na"
    local tel_swr_max="na"
    local tel_pcm_playback="na"
    local tel_pcm_mixer_line="na"

    echo "==> Side-A transmit smoke test"
    echo "    Station: ${A_LABEL} on ${A_SSH}   callsign=${CALLSIGN_A}"
    echo "    Mode: ${MODE}   Payload: ${PAYLOAD_SIZE}B   Audio: ${A_AUDIO_DEVICE_LABEL}"
    echo "    Freq: ${TEST_FREQ_HZ} Hz (${TEST_MODE_RIG_A}) (2m enforced)"
    echo "    Report: ${report}"
    echo ""
    preflight_check

    local tel_pid=""
    (
        ssh_a "set +e; rm -f '${a_tel_log}'; \
            pcm_line=\$(amixer -c CODEC get PCM 2>/dev/null | grep 'Front Left' | head -n1 || true); \
            echo 'PCM_MIXER_LINE='\"\${pcm_line}\" >>'${a_tel_log}'; \
            pcm=\$(printf '%s\n' \"\$pcm_line\" | sed -n 's/.*\[\([0-9][0-9]*%\)\].*/\1/p'); \
            [[ -n \"\$pcm\" ]] || pcm=na; \
            echo 'PCM_PLAYBACK='\"\${pcm}\" >>'${a_tel_log}'; \
            for _ in \$(seq 1 ${TELEMETRY_SAMPLES}); do \
                ts=\$(date +%H:%M:%S.%3N); \
                ptt=\$(rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} t 2>/dev/null | tail -n 1 || echo na); \
                alc=\$(rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} l ALC_METER 2>/dev/null | tail -n 1 || echo na); \
                rfm=\$(rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} l RFPOWER_METER 2>/dev/null | tail -n 1 || echo na); \
                swr=\$(rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} l SWR 2>/dev/null | tail -n 1 || echo na); \
                echo \"\${ts} PTT=\${ptt} ALC=\${alc} RFM=\${rfm} SWR=\${swr}\" >>'${a_tel_log}'; \
                sleep ${TELEMETRY_INTERVAL}; \
            done"
    ) &
    tel_pid="$!"

    timeout "${TX_TIMEOUT}" ssh ${SSH_OPTS} "${A_SSH}" \
        "'${ar}/target/release/openpulse' \
            --backend cpal \
            --log info \
            --ptt rigctld \
            --rig ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} \
            transmit \
            --mode '${MODE}' \
                    --fec "${FEC//_/-}" \
            ${a_device_arg} \
            '${payload_text}' \
            >'${a_iss_log}' 2>&1" \
        || iss_exit=$?

    sleep "${KILL_WAIT}"
    ssh_a "pkill -f '${ar}/target/release/openpulse .*transmit' 2>/dev/null || true" || true

    if [[ -n "$tel_pid" ]]; then
        wait "$tel_pid" || true
    fi

    tel_ptt_on="$(ssh_a "awk '/PTT=1/{c++} END{print c+0}' '${a_tel_log}' 2>/dev/null || echo na" || echo na)"
    tel_alc_nonzero="$(ssh_a "awk -F'ALC=' '{if (NF>1){split(\$2,a,\" \" ); v=a[1]+0; if (v>0)c++}} END{print c+0}' '${a_tel_log}' 2>/dev/null || echo na" || echo na)"
    tel_rfm_nonzero="$(ssh_a "awk -F'RFM=' '{if (NF>1){v=\$2+0; if (v>0)c++}} END{print c+0}' '${a_tel_log}' 2>/dev/null || echo na" || echo na)"
    tel_swr_max="$(ssh_a "awk -F'SWR=' '{if (NF>1){v=\$2+0; if (!seen || v>mx){mx=v; seen=1}}} END{if (seen) print mx; else print \"na\"}' '${a_tel_log}' 2>/dev/null || echo na" || echo na)"
    tel_pcm_playback="$(ssh_a "awk -F'=' '/^PCM_PLAYBACK=/{print \$2; exit}' '${a_tel_log}' 2>/dev/null || echo na" || echo na)"
    tel_pcm_mixer_line="$(ssh_a "awk -F'=' '/^PCM_MIXER_LINE=/{sub(/^PCM_MIXER_LINE=/, \"\", \$0); print; exit}' '${a_tel_log}' 2>/dev/null || echo na" || echo na)"

    telemetry_summary="A(ptt_on=${tel_ptt_on}, pcm=${tel_pcm_playback}, mixer=\"${tel_pcm_mixer_line}\", alc>0=${tel_alc_nonzero}, rfm>0=${tel_rfm_nonzero}, swr_max=${tel_swr_max})"

    local test_pass=false
    local fail_reason=""
    if [[ $iss_exit -ne 0 ]]; then
        fail_reason="ISS exit ${iss_exit}"
    elif [[ "${tel_alc_nonzero}" == "0" && "${tel_rfm_nonzero}" == "0" ]]; then
        fail_reason="side-A transmit produced no RF/ALC movement"
    else
        test_pass=true
    fi

    if $test_pass; then
        echo "PASS"
    else
        echo "FAIL (${fail_reason})"
    fi
    echo "    telemetry: ${telemetry_summary}"

    cat > "${report}" <<JSON
{
  "timestamp": "${ts}",
  "git_sha": "${git_sha}",
  "station": "$(json_escape "${A_LABEL}")",
  "callsign": "$(json_escape "${CALLSIGN_A}")",
  "freq_hz": ${TEST_FREQ_HZ},
  "mode": "$(json_escape "${MODE}")",
  "fec": "$(json_escape "${FEC}")",
  "payload_bytes": ${PAYLOAD_SIZE},
  "audio_device": "$(json_escape "${A_AUDIO_DEVICE}")",
  "audio_device_label": "$(json_escape "${A_AUDIO_DEVICE_LABEL}")",
  "result": "$(if $test_pass; then echo pass; else echo fail; fi)",
  "fail_reason": "$(json_escape "${fail_reason}")",
  "iss_exit": ${iss_exit},
  "telemetry": {
    "ptt_on": "$(json_escape "${tel_ptt_on}")",
    "pcm_playback": "$(json_escape "${tel_pcm_playback}")",
    "pcm_mixer_line": "$(json_escape "${tel_pcm_mixer_line}")",
    "alc_nonzero": "$(json_escape "${tel_alc_nonzero}")",
    "rfm_nonzero": "$(json_escape "${tel_rfm_nonzero}")",
    "swr_max": "$(json_escape "${tel_swr_max}")"
  }
}
JSON

    echo "    Report: ${report}"

    [[ $test_pass == true ]]
}

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
    "QPSK250|none|128"
    "QPSK500|none|128"
    "QPSK500|rs|128"
    "QPSK500|soft_concatenated|128"
)

run_matrix() {
    local ts
    ts="$(date -u +%Y-%m-%dT%H%M%S)"
    local git_sha
    git_sha="$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")"
    local report="${OUTPUT_DIR}/onair-${ts}.json"
    mkdir -p "$OUTPUT_DIR"

    local cases=()
    if [[ -n "$SINGLE_CASE" ]]; then
        cases=("$SINGLE_CASE")
    elif [[ "$TIER" == "quick" ]]; then
        cases=("${QUICK_CASES[@]}")
    else
        cases=("${FULL_CASES[@]}")
    fi

    local total="${#cases[@]}"
    local pass=0
    local fail=0
    local results=()
    local loopback_case_counter=0

    local ar
    local br
    local bl
    ar="$(a_repo)"
    br="$(b_repo)"
    bl="$(b_logdir)"

    local iss_label
    local irs_label
    local iss_station
    local irs_station
    local iss_callsign
    local irs_callsign
    if [[ "$REVERSE" == "1" ]]; then
        iss_label="${B_LABEL}"
        irs_label="${A_LABEL}"
        iss_station="${B_SSH}"
        irs_station="${A_SSH}"
        iss_callsign="${CALLSIGN_B}"
        irs_callsign="${CALLSIGN_A}"
    else
        iss_label="${A_LABEL}"
        irs_label="${B_LABEL}"
        iss_station="${A_SSH}"
        irs_station="${B_SSH}"
        iss_callsign="${CALLSIGN_A}"
        irs_callsign="${CALLSIGN_B}"
    fi

    preflight_check

    echo "==> On-air test matrix (tier=${TIER}, ${total} cases)"
    echo "    ISS: ${iss_label} on ${iss_station}   callsign=${iss_callsign}"
    echo "    IRS: ${irs_label} on ${irs_station}   callsign=${irs_callsign}"
    echo "    Freq: ${TEST_FREQ_HZ} Hz (A:${TEST_MODE_RIG_A}, B:${TEST_MODE_RIG_B}) (2m enforced)"
    echo "    Report: ${report}"
    echo ""

    for case_spec in "${cases[@]}"; do
        loopback_case_counter=$(( loopback_case_counter + 1 ))
        if (( LOOPBACK_REGRESSION_INTERVAL > 0 && loopback_case_counter % LOOPBACK_REGRESSION_INTERVAL == 0 )); then
            run_loopback_regression
        fi
        local normalized_case_spec
        normalized_case_spec="$case_spec"
        if [[ "$normalized_case_spec" == *,* && "$normalized_case_spec" != *"|"* ]]; then
            normalized_case_spec="${normalized_case_spec//,/|}"
        fi

        IFS='|' read -r MODE FEC PAYLOAD_SIZE <<< "$normalized_case_spec"

        if [[ -z "${MODE}" || -z "${FEC}" || -z "${PAYLOAD_SIZE}" ]]; then
            echo "FAIL (invalid case format; expected MODE|FEC|PAYLOAD_BYTES)"
            fail=$(( fail + 1 ))
            results+=("{\"mode\":\"$(json_escape "${MODE:-}")\",\"fec\":\"$(json_escape "${FEC:-}")\",\"payload_bytes\":0,\"result\":\"fail\",\"fail_reason\":\"invalid case format\",\"iss_exit\":255}")
            continue
        fi

        if ! [[ "$PAYLOAD_SIZE" =~ ^[0-9]+$ ]] || (( PAYLOAD_SIZE < 1 || PAYLOAD_SIZE > 255 )); then
            echo "FAIL (invalid payload size '${PAYLOAD_SIZE}'; must be 1..255)"
            fail=$(( fail + 1 ))
            results+=("{\"mode\":\"$(json_escape "${MODE}")\",\"fec\":\"$(json_escape "${FEC}")\",\"payload_bytes\":0,\"result\":\"fail\",\"fail_reason\":\"invalid payload size ${PAYLOAD_SIZE}\",\"iss_exit\":255}")
            continue
        fi

        printf "  [%-22s fec=%-18s payload=%4sB] ... " "${MODE}" "${FEC}" "${PAYLOAD_SIZE}"

        local b_irs_log="${bl}/irs-${MODE}-${FEC}.log"
        local a_iss_log="/tmp/openpulse-iss-${MODE}-${FEC}.log"
        local a_irs_log="/tmp/openpulse-irs-${MODE}-${FEC}.log"
        local b_iss_log="/tmp/openpulse-iss-${MODE}-${FEC}.log"
        local a_device_arg=""
        local b_device_arg=""
        if [[ -n "${A_AUDIO_DEVICE}" ]]; then
            a_device_arg="--device '${A_AUDIO_DEVICE}'"
        fi
        if [[ -n "${B_AUDIO_DEVICE}" ]]; then
            b_device_arg="--device '${B_AUDIO_DEVICE}'"
        fi

        local payload_text
        payload_text="$(python3 -c "import secrets, string, sys; a = string.ascii_letters + string.digits; sys.stdout.write(''.join(secrets.choice(a) for _ in range(${PAYLOAD_SIZE})))")"

        local iss_exit=0
        local irs_content=""
        local telemetry_summary=""
        local tel_iss_ptt_on="na"
        local tel_iss_alc_nonzero="na"
        local tel_iss_rfm_nonzero="na"
        local tel_iss_pcm_playback="na"
        local tel_irs_strength_max="na"

        local irs_listen_ms
        irs_listen_ms=$(( (IRS_STARTUP_WAIT + TX_TIMEOUT + 30) * 1000 ))

        if [[ "$REVERSE" == "1" ]]; then
            # 1) Start IRS receiver on Station A.
            ssh_a "pids=\$(pgrep -f '${ar}/target/release/openpulse .*receive' || true); \
                for pid in \$pids; do \
                    [[ \"\$pid\" != \"\$\$\" ]] && kill \"\$pid\" 2>/dev/null || true; \
                done; \
                nohup '${ar}/target/release/openpulse' \
                    --backend cpal \
                    --log debug \
                    --ptt none \
                    receive \
                    --mode '${MODE}' \
                    --fec "${FEC//_/-}" \
                    --listen-ms ${irs_listen_ms} \
                    ${a_device_arg} \
                    >'${a_irs_log}' 2>&1 </dev/null &"

            sleep "${IRS_STARTUP_WAIT}"

            local irs_alive
            irs_alive="$(ssh_a "pgrep -f '${ar}/target/release/openpulse .*receive' >/dev/null && echo yes || echo no" || echo no)"
            if [[ "$irs_alive" != "yes" ]]; then
                local irs_boot_log
                irs_boot_log="$(ssh_a "tail -n 80 '${a_irs_log}' 2>/dev/null || true" || true)"
                echo "FAIL (IRS not running before transmit)"
                fail=$(( fail + 1 ))
                results+=("{\"mode\":\"$(json_escape "${MODE}")\",\"fec\":\"$(json_escape "${FEC}")\",\"payload_bytes\":${PAYLOAD_SIZE},\"result\":\"fail\",\"fail_reason\":\"IRS not running before transmit\",\"iss_exit\":255}")
                if [[ -n "$irs_boot_log" ]]; then
                    echo "    IRS log excerpt: $(printf '%s' "$irs_boot_log" | tail -n 1)"
                fi
                continue
            fi

            # 2) Send from ISS on Station B.
            local tel_iss_pid=""
            local tel_irs_pid=""
            local b_tel_log="/tmp/openpulse-telemetry-iss-${MODE}-${FEC}.log"
            local a_tel_log="/tmp/openpulse-telemetry-irs-${MODE}-${FEC}.log"
            if [[ "$TELEMETRY_ENABLE" == "1" ]]; then
                (
                    ssh_b "set +e; rm -f '${b_tel_log}'; \
                        pcm_line=\$(amixer -c CODEC get PCM 2>/dev/null | grep 'Front Left' | head -n1 || true); \
                        echo 'PCM_MIXER_LINE='\"\${pcm_line}\" >>'${b_tel_log}'; \
                        pcm=\$(printf '%s\n' "\$pcm_line" | sed -n 's/.*\[\([0-9][0-9]*%\)\].*/\1/p'); \
                        [[ -n "\$pcm" ]] || pcm=na; \
                        echo 'PCM_PLAYBACK='\"\${pcm}\" >>'${b_tel_log}'; \
                        for _ in \$(seq 1 ${TELEMETRY_SAMPLES}); do \
                            ts=\$(date +%H:%M:%S.%3N); \
                            ptt=\$(rigctl -m 2 -r ${B_RIGCTLD_ADDR}:${B_RIGCTLD_PORT} t 2>/dev/null | tail -n 1 || echo na); \
                            alc=\$(rigctl -m 2 -r ${B_RIGCTLD_ADDR}:${B_RIGCTLD_PORT} l ALC_METER 2>/dev/null | tail -n 1 || echo na); \
                            rfm=\$(rigctl -m 2 -r ${B_RIGCTLD_ADDR}:${B_RIGCTLD_PORT} l RFPOWER_METER 2>/dev/null | tail -n 1 || echo na); \
                            echo \"\${ts} PTT=\${ptt} ALC=\${alc} RFM=\${rfm}\" >>'${b_tel_log}'; \
                            sleep ${TELEMETRY_INTERVAL}; \
                        done"
                ) &
                tel_iss_pid="$!"

                (
                    ssh_a "set +e; rm -f '${a_tel_log}'; \
                        for _ in \$(seq 1 ${TELEMETRY_SAMPLES}); do \
                            ts=\$(date +%H:%M:%S.%3N); \
                            sm=\$(rigctl -m 2 -r ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} l STRENGTH 2>/dev/null | tail -n 1 || echo na); \
                            echo \"\${ts} STRENGTH=\${sm}\" >>'${a_tel_log}'; \
                            sleep ${TELEMETRY_INTERVAL}; \
                        done"
                ) &
                tel_irs_pid="$!"
            fi

            timeout "${TX_TIMEOUT}" ssh ${SSH_OPTS} "${B_SSH}" \
                "'${br}/target/release/openpulse' \
                    --backend cpal \
                    --log info \
                    --ptt rigctld \
                    --rig ${B_RIGCTLD_ADDR}:${B_RIGCTLD_PORT} \
                    transmit \
                    --mode '${MODE}' \
                    --fec "${FEC//_/-}" \
                    ${b_device_arg} \
                    '${payload_text}' \
                    >'${b_iss_log}' 2>&1" \
                || iss_exit=$?

            sleep "${KILL_WAIT}"
            ssh_a "pids=\$(pgrep -f '${ar}/target/release/openpulse .*receive' || true); \
                for pid in \$pids; do \
                    [[ \"\$pid\" != \"\$\$\" ]] && kill \"\$pid\" 2>/dev/null || true; \
                done"

            irs_content="$(ssh_a "cat '${a_irs_log}' 2>/dev/null || true" || true)"

            if [[ "$TELEMETRY_ENABLE" == "1" ]]; then
                if [[ -n "$tel_iss_pid" ]]; then
                    wait "$tel_iss_pid" || true
                fi
                if [[ -n "$tel_irs_pid" ]]; then
                    wait "$tel_irs_pid" || true
                fi

                tel_iss_ptt_on="$(ssh_b "awk '/PTT=1/{c++} END{print c+0}' '${b_tel_log}' 2>/dev/null || echo na" || echo na)"
                tel_iss_alc_nonzero="$(ssh_b "awk -F'ALC=' '{if (NF>1){split(\$2,a,\" \" ); v=a[1]+0; if (v>0)c++}} END{print c+0}' '${b_tel_log}' 2>/dev/null || echo na" || echo na)"
                tel_iss_rfm_nonzero="$(ssh_b "awk -F'RFM=' '{if (NF>1){v=\$2+0; if (v>0)c++}} END{print c+0}' '${b_tel_log}' 2>/dev/null || echo na" || echo na)"
                tel_iss_pcm_playback="$(ssh_b "awk -F'=' '/^PCM_PLAYBACK=/{print \$2; exit}' '${b_tel_log}' 2>/dev/null || echo na" || echo na)"
                local tel_iss_pcm_mixer_line
                tel_iss_pcm_mixer_line="$(ssh_b "awk -F'=' '/^PCM_MIXER_LINE=/{sub(/^PCM_MIXER_LINE=/, \"\", \$0); print; exit}' '${b_tel_log}' 2>/dev/null || echo na" || echo na)"
                tel_irs_strength_max="$(ssh_a "awk -F'STRENGTH=' '{if (NF>1){v=\$2+0; if (!seen || v>mx){mx=v; seen=1}}} END{if (seen) print mx; else print \"na\"}' '${a_tel_log}' 2>/dev/null || echo na" || echo na)"
                telemetry_summary="ISS(ptt_on=${tel_iss_ptt_on}, pcm=${tel_iss_pcm_playback}, mixer=\"${tel_iss_pcm_mixer_line}\", alc>0=${tel_iss_alc_nonzero}, rfm>0=${tel_iss_rfm_nonzero}) IRS(str_max=${tel_irs_strength_max})"
            fi
        else
            # 1) Start IRS receiver on Station B.
            ssh_b "pids=\$(pgrep -f '${br}/target/release/openpulse .*receive' || true); \
                for pid in \$pids; do \
                    [[ \"\$pid\" != \"\$\$\" ]] && kill \"\$pid\" 2>/dev/null || true; \
                done; \
                nohup '${br}/target/release/openpulse' \
                    --backend cpal \
                    --log debug \
                    --ptt none \
                    receive \
                    --mode '${MODE}' \
                    --fec "${FEC//_/-}" \
                    --listen-ms ${irs_listen_ms} \
                    ${b_device_arg} \
                    >'${b_irs_log}' 2>&1 </dev/null &"

            sleep "${IRS_STARTUP_WAIT}"

            local irs_alive
            irs_alive="$(ssh_b "pgrep -f '${br}/target/release/openpulse .*receive' >/dev/null && echo yes || echo no" || echo no)"
            if [[ "$irs_alive" != "yes" ]]; then
                local irs_boot_log
                irs_boot_log="$(ssh_b "tail -n 80 '${b_irs_log}' 2>/dev/null || true" || true)"
                echo "FAIL (IRS not running before transmit)"
                fail=$(( fail + 1 ))
                results+=("{\"mode\":\"$(json_escape "${MODE}")\",\"fec\":\"$(json_escape "${FEC}")\",\"payload_bytes\":${PAYLOAD_SIZE},\"result\":\"fail\",\"fail_reason\":\"IRS not running before transmit\",\"iss_exit\":255}")
                if [[ -n "$irs_boot_log" ]]; then
                    echo "    IRS log excerpt: $(printf '%s' "$irs_boot_log" | tail -n 1)"
                fi
                continue
            fi

            # 2) Send from ISS on Station A.
            timeout "${TX_TIMEOUT}" ssh ${SSH_OPTS} "${A_SSH}" \
                "'${ar}/target/release/openpulse' \
                    --backend cpal \
                    --log info \
                    --ptt rigctld \
                    --rig ${A_RIGCTLD_ADDR}:${A_RIGCTLD_PORT} \
                    transmit \
                    --mode '${MODE}' \
                    --fec "${FEC//_/-}" \
                    ${a_device_arg} \
                    '${payload_text}' \
                    >'${a_iss_log}' 2>&1" \
                || iss_exit=$?

            sleep "${KILL_WAIT}"
            ssh_b "pids=\$(pgrep -f '${br}/target/release/openpulse .*receive' || true); \
                for pid in \$pids; do \
                    [[ \"\$pid\" != \"\$\$\" ]] && kill \"\$pid\" 2>/dev/null || true; \
                done"

            irs_content="$(ssh_b "cat '${b_irs_log}' 2>/dev/null || true" || true)"
        fi

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
            run_loopback_regression || echo "  [loopback] FAIL after test failure — software regression suspected" >&2
            # Show the last lines of the IRS log to expose audio/decode issues.
            local irs_head="" irs_tail=""
            if [[ "$REVERSE" == "1" ]]; then
                irs_head="$(ssh_a "head -n 15 '${a_irs_log}' 2>/dev/null || true" || true)"
                irs_tail="$(ssh_a "tail -n 25 '${a_irs_log}' 2>/dev/null || true" || true)"
            else
                irs_head="$(ssh_b "head -n 15 '${b_irs_log}' 2>/dev/null || true" || true)"
                irs_tail="$(ssh_b "tail -n 25 '${b_irs_log}' 2>/dev/null || true" || true)"
            fi
            if [[ -n "$irs_head" ]]; then
                echo "    IRS log head:"
                echo "$irs_head" | sed 's/^/      /'
            fi
            if [[ -n "$irs_tail" ]]; then
                echo "    IRS log tail:"
                echo "$irs_tail" | sed 's/^/      /'
            fi
        fi

        if [[ -n "$telemetry_summary" ]]; then
            echo "    telemetry: ${telemetry_summary}"
        fi

        results+=("{\"mode\":\"$(json_escape "${MODE}")\",\"fec\":\"$(json_escape "${FEC}")\",\"payload_bytes\":${PAYLOAD_SIZE},\"result\":\"$(${test_pass} && echo pass || echo fail)\",\"fail_reason\":\"$(json_escape "${fail_reason}")\",\"iss_exit\":${iss_exit},\"telemetry_iss_ptt_on\":\"$(json_escape "${tel_iss_ptt_on}")\",\"telemetry_iss_pcm_playback\":\"$(json_escape "${tel_iss_pcm_playback}")\",\"telemetry_iss_alc_nonzero\":\"$(json_escape "${tel_iss_alc_nonzero}")\",\"telemetry_iss_rfm_nonzero\":\"$(json_escape "${tel_iss_rfm_nonzero}")\",\"telemetry_irs_strength_max\":\"$(json_escape "${tel_irs_strength_max}")\"}")
    done

    local results_json
    results_json="$(IFS=,; echo "${results[*]}")"

    cat > "${report}" <<JSON
{
  "timestamp": "${ts}",
  "git_sha": "${git_sha}",
  "tier": "${TIER}",
    "iss_station": "$(json_escape "${iss_station}")",
    "irs_station": "$(json_escape "${irs_station}")",
    "callsign_iss": "$(json_escape "${iss_callsign}")",
    "callsign_irs": "$(json_escape "${irs_callsign}")",
  "freq_hz": ${TEST_FREQ_HZ},
    "rig_mode_a": "$(json_escape "${TEST_MODE_RIG_A}")",
    "rig_mode_b": "$(json_escape "${TEST_MODE_RIG_B}")",
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

case "$ACTION" in
    setup)
        setup
        ;;
    run)
        trap 'cleanup_all' EXIT
        check_ssh_agent
        start_rigctld_a
        start_rigctld_b
        save_rig_state_a
        save_rig_state_b
        apply_known_good_settings_a
        apply_known_good_settings_b
        tune_a
        tune_b
        maybe_tune_high_swr_a "startup"
        maybe_tune_high_swr_b "startup"
        maybe_tune_high_swr_a "qsy"
        maybe_tune_high_swr_b "qsy"
        run_loopback_regression quick
        run_matrix
        ;;
    sidea)
        trap 'cleanup_side_a' EXIT
        setup_side_a
        run_side_a_transmit
        ;;
    supervise)
        trap 'cleanup_all' EXIT
        check_ssh_agent
        setup
        run_matrix
        ;;
    status)
        status_a
        status_b
        ;;
    cleanup)
        cleanup_all
        ;;
    *)
        usage
        exit 1
        ;;
esac
