#!/usr/bin/env bash
# On-air TWIN-OTA scenario: two real openpulse-server daemons on two RF stations,
# driving receiver-led OTA adaptive rate-stepping over the air. Station A is the
# ISS (sends traffic, climbs its TX rate as B recommends); station B is the IRS.
# Watch both directions live with openpulse-twinview over SSH-forwarded ports.
#
# The real-radio counterpart of the in-process twin rig: instead of bridging two
# daemons through a channel model in one process, the daemons key real radios and
# the channel is the air. Each station runs one rigctld (CAT freq + PTT) and the
# cpal audio backend on its rig's soundcard.
#
# Prereqs: rigctld/rigctl + a cpal soundcard on BOTH stations; SSH access; a
#   filled-in profile (docs/config/onair-twin-ota.example.sh).
#
# Usage:
#   source docs/config/onair-twin-ota.example.sh
#   ./scripts/run-onair-twin-ota.sh <setup|run|supervise|status|cleanup>
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# ── Profile (env, with safe defaults) ─────────────────────────────────────────
A_SSH="${A_SSH:-}"; B_SSH="${B_SSH:-}"
SSH_OPTS="${SSH_OPTS:--o BatchMode=yes -o ConnectTimeout=10}"
CALLSIGN_A="${CALLSIGN_A:-N0CALL}"; CALLSIGN_B="${CALLSIGN_B:-N0CALL}"
GRID_A="${GRID_A:-AA00}"; GRID_B="${GRID_B:-AA00}"
A_LABEL="${A_LABEL:-Station A}"; B_LABEL="${B_LABEL:-Station B}"
A_HAMLIB_MODEL="${A_HAMLIB_MODEL:-0}"; B_HAMLIB_MODEL="${B_HAMLIB_MODEL:-0}"
A_CAT_PORT="${A_CAT_PORT:-}"; A_CAT_BAUD="${A_CAT_BAUD:-19200}"
B_CAT_PORT="${B_CAT_PORT:-}"; B_CAT_BAUD="${B_CAT_BAUD:-19200}"
A_PTT_PORT="${A_PTT_PORT:-$A_CAT_PORT}"; A_PTT_TYPE="${A_PTT_TYPE:-RTS}"
B_PTT_PORT="${B_PTT_PORT:-$B_CAT_PORT}"; B_PTT_TYPE="${B_PTT_TYPE:-RTS}"
A_RIGCTLD_PORT="${A_RIGCTLD_PORT:-4532}"; B_RIGCTLD_PORT="${B_RIGCTLD_PORT:-4532}"
A_AUDIO_DEVICE="${A_AUDIO_DEVICE:-default}"; B_AUDIO_DEVICE="${B_AUDIO_DEVICE:-default}"
A_REPO_DIR="${A_REPO_DIR:-\$HOME/OpenPulseHF}"; B_REPO_DIR="${B_REPO_DIR:-\$HOME/OpenPulseHF}"
TEST_FREQ_HZ="${TEST_FREQ_HZ:-14070000}"; TEST_MODE_RIG="${TEST_MODE_RIG:-USB}"
A_RFPOWER="${A_RFPOWER:-0.10}"; B_RFPOWER="${B_RFPOWER:-0.10}"
OTA_PROFILE="${OTA_PROFILE:-hpx_hf}"; START_MODE="${START_MODE:-BPSK250}"
DAEMON_TCP_PORT="${DAEMON_TCP_PORT:-9000}"
TRAFFIC_INTERVAL="${TRAFFIC_INTERVAL:-2}"; TRAFFIC_SIZE="${TRAFFIC_SIZE:-128}"
TRAFFIC_DURATION="${TRAFFIC_DURATION:-120}"
OUTPUT_DIR="${OUTPUT_DIR:-docs/dev/test-reports}"

ACTION="${1:-supervise}"
CFG_DIR='${HOME}/.config/openpulse-twin-ota'   # remote XDG_CONFIG_HOME per station

die() { echo "ERROR: $*" >&2; exit 1; }
ssh_a() { ssh ${SSH_OPTS} "${A_SSH}" "$@"; }
ssh_b() { ssh ${SSH_OPTS} "${B_SSH}" "$@"; }

preflight() {
    [[ -n "$A_SSH" && -n "$B_SSH" ]] || die "A_SSH/B_SSH unset — source the profile first"
    [[ "$CALLSIGN_A" != "N0CALL" && "$CALLSIGN_B" != "N0CALL" ]] || die "set real callsigns"
    command -v ssh >/dev/null || die "ssh not found"
}

# ── per-station helpers (run remotely) ────────────────────────────────────────
# $1 = ssh fn name, others via the *_VARS captured at call sites.
start_rigctld() {  # ssh_fn model cat_port cat_baud ptt_port ptt_type rigctld_port label
    local sshfn="$1" model="$2" cat="$3" baud="$4" ptt="$5" ptype="$6" port="$7" label="$8"
    echo "  [$label] starting rigctld (model=$model cat=$cat ptt=$ptype:$ptt port=$port)"
    "$sshfn" "pkill -x rigctld 2>/dev/null || true; sleep 0.5; \
        nohup rigctld -m $model -r $cat -s $baud -p $ptt -P $ptype -t $port \
        </dev/null >/tmp/twin-ota-rigctld.log 2>&1 & sleep 1; \
        pgrep -x rigctld >/dev/null && echo '  [$label] rigctld ok' || { echo '  [$label] rigctld FAILED'; exit 1; }"
}

tune() {  # ssh_fn rigctld_port rfpower label
    local sshfn="$1" port="$2" power="$3" label="$4"
    echo "  [$label] tune → ${TEST_FREQ_HZ} Hz ${TEST_MODE_RIG}, power=${power}"
    "$sshfn" "rigctl -m 2 -r 127.0.0.1:$port F ${TEST_FREQ_HZ} 2>/dev/null || echo '  [$label] WARN set freq'; \
        rigctl -m 2 -r 127.0.0.1:$port M ${TEST_MODE_RIG} 3000 2>/dev/null || echo '  [$label] WARN set mode'; \
        rigctl -m 2 -r 127.0.0.1:$port L RFPOWER ${power} 2>/dev/null || echo '  [$label] WARN set power'"
}

write_config() {  # ssh_fn callsign grid audio_device ptt_backend rigctld_port label
    local sshfn="$1" call="$2" grid="$3" dev="$4" rigport="$5" label="$6"
    echo "  [$label] writing daemon config (cpal device=$dev, OTA=$OTA_PROFILE)"
    "$sshfn" "mkdir -p $CFG_DIR/openpulse && cat > $CFG_DIR/openpulse/config.toml <<EOF
[station]
callsign = \"$call\"
grid_square = \"$grid\"
[audio]
backend = \"cpal\"
device = \"$dev\"
[modem]
mode = \"$START_MODE\"
profile = \"$OTA_PROFILE\"
ptt_backend = \"rigctld\"
ota_enabled = true
ota_profile = \"$OTA_PROFILE\"
[radio]
cat_backend = \"rigctld\"
rigctld_addr = \"127.0.0.1:$rigport\"
[daemon]
tcp_port = $DAEMON_TCP_PORT
EOF"
}

build_remote() {  # ssh_fn repo_dir label
    local sshfn="$1" repo="$2" label="$3"
    echo "  [$label] building openpulse-server --features cpal + openpulse CLI (may take a while)…"
    # Server needs cpal (real audio); the CLI is only used as a control client
    # (ota-status), so build it lean with --no-default-features.
    "$sshfn" "cd $repo && \
        cargo build --release -p openpulse-daemon --features cpal --bin openpulse-server >/tmp/twin-ota-build.log 2>&1 && \
        cargo build --release -p openpulse-cli --no-default-features --bin openpulse >>/tmp/twin-ota-build.log 2>&1 \
        && echo '  [$label] build ok' || { tail -8 /tmp/twin-ota-build.log; exit 1; }"
}

launch_daemon() {  # ssh_fn repo_dir label
    local sshfn="$1" repo="$2" label="$3"
    echo "  [$label] launching openpulse-server (control 127.0.0.1:$DAEMON_TCP_PORT)"
    "$sshfn" "pkill -x openpulse-server 2>/dev/null || true; sleep 0.5; \
        XDG_CONFIG_HOME=$CFG_DIR RUST_LOG=info \
        nohup $repo/target/release/openpulse-server </dev/null >/tmp/twin-ota-daemon.log 2>&1 & \
        sleep 2; pgrep -x openpulse-server >/dev/null && echo '  [$label] daemon ok' || { echo '  [$label] daemon FAILED'; tail -5 /tmp/twin-ota-daemon.log; exit 1; }"
}

# ── actions ───────────────────────────────────────────────────────────────────
do_setup() {
    preflight
    echo "== setup =="
    build_remote ssh_a "$A_REPO_DIR" "$A_LABEL"
    build_remote ssh_b "$B_REPO_DIR" "$B_LABEL"
    echo "setup complete."
}

do_run() {
    preflight
    echo "== run =="
    start_rigctld ssh_a "$A_HAMLIB_MODEL" "$A_CAT_PORT" "$A_CAT_BAUD" "$A_PTT_PORT" "$A_PTT_TYPE" "$A_RIGCTLD_PORT" "$A_LABEL"
    start_rigctld ssh_b "$B_HAMLIB_MODEL" "$B_CAT_PORT" "$B_CAT_BAUD" "$B_PTT_PORT" "$B_PTT_TYPE" "$B_RIGCTLD_PORT" "$B_LABEL"
    tune ssh_a "$A_RIGCTLD_PORT" "$A_RFPOWER" "$A_LABEL"
    tune ssh_b "$B_RIGCTLD_PORT" "$B_RFPOWER" "$B_LABEL"
    write_config ssh_a "$CALLSIGN_A" "$GRID_A" "$A_AUDIO_DEVICE" "$A_RIGCTLD_PORT" "$A_LABEL"
    write_config ssh_b "$CALLSIGN_B" "$GRID_B" "$B_AUDIO_DEVICE" "$B_RIGCTLD_PORT" "$B_LABEL"
    launch_daemon ssh_a "$A_REPO_DIR" "$A_LABEL"
    launch_daemon ssh_b "$B_REPO_DIR" "$B_LABEL"

    cat <<EOF

Daemons up. To watch BOTH directions live, forward each control port locally and
run openpulse-twinview on your workstation:
  ssh ${SSH_OPTS} -N -L 9000:127.0.0.1:${DAEMON_TCP_PORT} ${A_SSH} &
  ssh ${SSH_OPTS} -N -L 9002:127.0.0.1:${DAEMON_TCP_PORT} ${B_SSH} &
  cargo run -p openpulse-twinview 127.0.0.1:9000 127.0.0.1:9002

Driving OTA traffic from ${A_LABEL} for ${TRAFFIC_DURATION}s …
EOF

    # Drive traffic from A's control port (background, time-boxed) and poll A's OTA
    # TX level every 5 s so the report shows the rate ladder climbing on the air.
    local report="${OUTPUT_DIR}/onair-twin-ota-$(date +%Y%m%d-%H%M%S).log"
    mkdir -p "$OUTPUT_DIR"
    echo "# on-air twin-OTA  freq=${TEST_FREQ_HZ} profile=${OTA_PROFILE} A=${CALLSIGN_A}→B=${CALLSIGN_B} dur=${TRAFFIC_DURATION}s" | tee "$report"
    ssh_a "cd $A_REPO_DIR && nohup timeout ${TRAFFIC_DURATION} \
        env ADDR=127.0.0.1:${DAEMON_TCP_PORT} TO=${CALLSIGN_B} INTERVAL=${TRAFFIC_INTERVAL} SIZE=${TRAFFIC_SIZE} \
        bash scripts/twin-traffic.sh </dev/null >/tmp/twin-ota-traffic.log 2>&1 & echo '  traffic started'"

    local end=$(( $(date +%s) + TRAFFIC_DURATION ))
    while [ "$(date +%s)" -lt "$end" ]; do
        local snap
        snap=$(ssh_a "$A_REPO_DIR/target/release/openpulse daemon --addr 127.0.0.1:${DAEMON_TCP_PORT} ota-status 2>/dev/null" || true)
        echo "$(date +%H:%M:%S)  A: ${snap:-<no status>}" | tee -a "$report"
        sleep 5
    done

    echo "traffic window complete." | tee -a "$report"
    echo "report: $report"
    echo "(watch the live TX-level climb on both directions in openpulse-twinview)"
}

do_status() {
    echo "== status =="
    for pair in "ssh_a:$A_LABEL" "ssh_b:$B_LABEL"; do
        local fn="${pair%%:*}" label="${pair##*:}"
        echo "[$label]"
        "$fn" "pgrep -x rigctld >/dev/null && echo '  rigctld: running' || echo '  rigctld: stopped'; \
               pgrep -x openpulse-server >/dev/null && echo '  daemon: running' || echo '  daemon: stopped'"
    done
}

do_cleanup() {
    echo "== cleanup =="
    ssh_a "pkill -x openpulse-server 2>/dev/null; pkill -x rigctld 2>/dev/null; true"
    ssh_b "pkill -x openpulse-server 2>/dev/null; pkill -x rigctld 2>/dev/null; true"
    echo "stopped daemons and rigctld on both stations."
}

case "$ACTION" in
    setup) do_setup ;;
    run) do_run ;;
    supervise) do_setup && do_run ;;
    status) do_status ;;
    cleanup) do_cleanup ;;
    *) die "unknown action '$ACTION' (setup|run|supervise|status|cleanup)" ;;
esac
