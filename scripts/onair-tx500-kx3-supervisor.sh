#!/usr/bin/env bash
# SSH-supervised on-air validation for a Lab599 TX500 (Raspberry Pi) and an
# Elecraft KX3 (Linux laptop), both using Digirig and close-range attenuation on
# the first pass.
#
# Usage:
#   source docs/config/onair-tx500-kx3.example.sh
#   ./scripts/onair-tx500-kx3-supervisor.sh setup
#   ./scripts/onair-tx500-kx3-supervisor.sh manage status
#   ./scripts/onair-tx500-kx3-supervisor.sh supervise --all

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

usage() {
    cat <<'EOF'
Usage:
  source docs/config/onair-tx500-kx3.example.sh
  ./scripts/onair-tx500-kx3-supervisor.sh <setup|manage|run|supervise> [options]

Options:
  --profile FILE   Source this combined profile first (default: docs/config/onair-tx500-kx3.example.sh)
  --quick          Run only the quick matrix.
  --full           Run only the full matrix.
  --all            Run quick then full (default for supervise).
  --label NAME     Label used for evidence bundles and reports.
  --output DIR     Output root for matrix/evidence artifacts.
  --notes FILE     Optional operator notes file passed to the validation flow.
  --help           Show this help text.

Manage subcommands:
  status           Show remote process and config status on both stations.
  cleanup          Stop known OpenPulse helper processes on both stations.
EOF
}

ACTION="supervise"
PROFILE_FILE="docs/config/onair-tx500-kx3.example.sh"
TIERS=("quick" "full")
LABEL="tx500-kx3"
OUTPUT_DIR="docs/dev/test-reports"
NOTES_FILE=""
MANAGE_SUBCOMMAND="status"

while [[ $# -gt 0 ]]; do
    case "$1" in
        setup|manage|run|supervise)
            ACTION="$1"
            ;;
        status|cleanup)
            MANAGE_SUBCOMMAND="$1"
            ;;
        --profile)
            PROFILE_FILE="$2"
            shift
            ;;
        --quick)
            TIERS=("quick")
            ;;
        --full)
            TIERS=("full")
            ;;
        --all)
            TIERS=("quick" "full")
            ;;
        --label)
            LABEL="$2"
            shift
            ;;
        --output)
            OUTPUT_DIR="$2"
            shift
            ;;
        --notes)
            NOTES_FILE="$2"
            shift
            ;;
        --help|-h)
            usage
            exit 0
            ;;
        *)
            echo "Unknown argument: $1" >&2
            usage >&2
            exit 1
            ;;
    esac
    shift
done

if [[ ! -f "$PROFILE_FILE" ]]; then
    echo "Profile file not found: $PROFILE_FILE" >&2
    exit 1
fi

# shellcheck disable=SC1090
source "$PROFILE_FILE"

required_vars=(
    STATION_A
    STATION_B
    SSH_OPTS
    CALLSIGN_A
    CALLSIGN_B
    TX500_CONFIG_FILE
    KX3_CONFIG_FILE
    TX500_REMOTE_CONFIG
    KX3_REMOTE_CONFIG
    TX500_REMOTE_BIN_DIR
    KX3_REMOTE_BIN_DIR
    TX500_REMOTE_LOG_DIR
    KX3_REMOTE_LOG_DIR
)

for var_name in "${required_vars[@]}"; do
    if [[ -z "${!var_name:-}" ]]; then
        echo "Missing required profile variable: $var_name" >&2
        exit 1
    fi
done

ssh_agent_check() {
    if ! ssh-add -l >/dev/null 2>&1; then
        echo "ssh-agent is not ready or has no identities loaded; run ssh-add first" >&2
        exit 1
    fi
}

ssh_a() {
    # shellcheck disable=SC2086
    ssh ${SSH_OPTS} "${STATION_A}" "$@"
}

ssh_b() {
    # shellcheck disable=SC2086
    ssh ${SSH_OPTS} "${STATION_B}" "$@"
}

deploy_profile() {
    local host="$1"
    local local_config="$2"
    local remote_config="$3"
    local remote_bin_dir="$4"
    local remote_log_dir="$5"

    if [[ ! -f "$local_config" ]]; then
        echo "Missing local config file: $local_config" >&2
        exit 1
    fi

    ssh ${SSH_OPTS} "$host" "mkdir -p ${remote_bin_dir} ${remote_log_dir} $(dirname "$remote_config")"
    rsync -a -e "ssh ${SSH_OPTS}" "$local_config" "${host}:${remote_config}"
}

check_remote_state() {
    local host="$1"
    local remote_config="$2"
    local remote_bin_dir="$3"
    local remote_log_dir="$4"
    ssh ${SSH_OPTS} "$host" "printf 'station=%s\n' '$host'; \
        printf 'config=%s\n' '${remote_config}'; \
        test -f '${remote_config}' && echo 'config: present' || echo 'config: missing'; \
        test -x '${remote_bin_dir}/openpulse' && echo 'openpulse: present' || echo 'openpulse: missing'; \
        test -x '${remote_bin_dir}/openpulse-tnc' && echo 'openpulse-tnc: present' || echo 'openpulse-tnc: missing'; \
        test -x '${remote_bin_dir}/openpulse-kisstnc' && echo 'openpulse-kisstnc: present' || echo 'openpulse-kisstnc: missing'; \
        test -x '${remote_bin_dir}/openpulse-gateway' && echo 'openpulse-gateway: present' || echo 'openpulse-gateway: missing'; \
        ls -ld '${remote_log_dir}' 2>/dev/null || true; \
        pgrep -af 'openpulse|rigctld' 2>/dev/null || true"
}

cleanup_remote() {
    local host="$1"
    ssh ${SSH_OPTS} "$host" "pkill -f openpulse-tnc || true; pkill -f openpulse-kisstnc || true; pkill -f openpulse-gateway || true"
}

run_flow() {
    local tier="$1"
    local run_label="${LABEL}-${tier}"
    local flow_args=("./scripts/run-onair-validation-flow.sh")
    if [[ "$tier" == "quick" ]]; then
        flow_args+=("--quick")
    else
        flow_args+=("--full")
    fi
    flow_args+=("--label" "$run_label" "--output" "$OUTPUT_DIR")
    if [[ -n "$NOTES_FILE" ]]; then
        flow_args+=("--notes" "$NOTES_FILE")
    fi
    "${flow_args[@]}"
}

setup() {
    ssh_agent_check
    echo "==> Deploying TX500 config to ${STATION_A}"
    deploy_profile "$STATION_A" "$TX500_CONFIG_FILE" "$TX500_REMOTE_CONFIG" "$TX500_REMOTE_BIN_DIR" "$TX500_REMOTE_LOG_DIR"
    echo "==> Deploying KX3 config to ${STATION_B}"
    deploy_profile "$STATION_B" "$KX3_CONFIG_FILE" "$KX3_REMOTE_CONFIG" "$KX3_REMOTE_BIN_DIR" "$KX3_REMOTE_LOG_DIR"
    echo "==> Remote status"
    check_remote_state "$STATION_A" "$TX500_REMOTE_CONFIG" "$TX500_REMOTE_BIN_DIR" "$TX500_REMOTE_LOG_DIR"
    check_remote_state "$STATION_B" "$KX3_REMOTE_CONFIG" "$KX3_REMOTE_BIN_DIR" "$KX3_REMOTE_LOG_DIR"
}

manage() {
    case "$MANAGE_SUBCOMMAND" in
        status)
            check_remote_state "$STATION_A" "$TX500_REMOTE_CONFIG" "$TX500_REMOTE_BIN_DIR" "$TX500_REMOTE_LOG_DIR"
            check_remote_state "$STATION_B" "$KX3_REMOTE_CONFIG" "$KX3_REMOTE_BIN_DIR" "$KX3_REMOTE_LOG_DIR"
            ;;
        cleanup)
            cleanup_remote "$STATION_A"
            cleanup_remote "$STATION_B"
            ;;
        *)
            echo "Unknown manage subcommand: $MANAGE_SUBCOMMAND" >&2
            exit 1
            ;;
    esac
}

run_all() {
    local exit_code=0
    for tier in "${TIERS[@]}"; do
        echo "==> Running ${tier} validation tier"
        if ! run_flow "$tier"; then
            exit_code=1
        fi
        echo "==> Station status after ${tier} tier"
        check_remote_state "$STATION_A" "$TX500_REMOTE_CONFIG" "$TX500_REMOTE_BIN_DIR" "$TX500_REMOTE_LOG_DIR"
        check_remote_state "$STATION_B" "$KX3_REMOTE_CONFIG" "$KX3_REMOTE_BIN_DIR" "$KX3_REMOTE_LOG_DIR"
    done
    return "$exit_code"
}

case "$ACTION" in
    setup)
        setup
        ;;
    manage)
        manage
        ;;
    run)
        run_all
        ;;
    supervise)
        ssh_agent_check
        setup
        run_all
        ;;
    *)
        usage
        exit 1
        ;;
esac