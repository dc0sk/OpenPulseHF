#!/usr/bin/env bash
# Launch two REAL openpulse-server daemons over the bidirectional snd-aloop
# loopback, for the real-audio twin-station rig. Each daemon uses the cpal backend
# on its own full-duplex PCM (aloop_a / aloop_b), so audio crosses A<->B through
# the real cpal+ALSA path. Attach one openpulse-panel per station to watch both
# directions live.
#
# Prereq: scripts/setup-twin-loopback.sh (publishes aloop_a / aloop_b).
#
# Env overrides:
#   MODE        modulation mode for both stations (default BPSK250)
#   PROFILE     adaptive profile (default hpx_hf); set OTA=1 to enable OTA stepping
#   OTA         1 to start a receiver-led OTA session on both daemons (default 0)
#   A_DEVICE    cpal device for station A (default aloop_a)
#   B_DEVICE    cpal device for station B (default aloop_b)
#   A_TCP/A_WS  station A control ports (default 9000 / 9001)
#   B_TCP/B_WS  station B control ports (default 9002 / 9003)
set -euo pipefail

MODE="${MODE:-BPSK250}"
PROFILE="${PROFILE:-hpx_hf}"
OTA="${OTA:-0}"
A_DEVICE="${A_DEVICE:-aloop_a}"
B_DEVICE="${B_DEVICE:-aloop_b}"
A_TCP="${A_TCP:-9000}"; A_WS="${A_WS:-9001}"
B_TCP="${B_TCP:-9002}"; B_WS="${B_WS:-9003}"

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SERVER="${REPO_DIR}/target/release/openpulse-server"

if [[ ! -x "$SERVER" ]]; then
    echo "==> building openpulse-server --features cpal"
    cargo build --release -p openpulse-daemon --features cpal --bin openpulse-server
fi

WORK="$(mktemp -d /tmp/openpulse-twin.XXXXXX)"
trap 'echo; echo "==> stopping daemons"; kill "${PID_A:-0}" "${PID_B:-0}" 2>/dev/null || true; rm -rf "$WORK"' EXIT INT TERM

write_cfg() {
    # $1 dir  $2 callsign  $3 device  $4 tcp  $5 ws
    local dir="$1/openpulse"
    mkdir -p "$dir"
    {
        echo '[station]';            echo "callsign = \"$2\""
        echo '[audio]';              echo 'backend = "cpal"'; echo "device = \"$3\""
        echo '[modem]';              echo "mode = \"$MODE\""; echo "profile = \"$PROFILE\""
        [[ "$OTA" == 1 ]] && { echo 'ota_enabled = true'; echo "ota_profile = \"$PROFILE\""; }
        echo '[daemon]';             echo "tcp_port = $4"; echo "websocket_port = $5"
        echo '[radio]';              echo 'cat_backend = "none"'
    } > "$dir/config.toml"
}

write_cfg "$WORK/a" "TWIN-A" "$A_DEVICE" "$A_TCP" "$A_WS"
write_cfg "$WORK/b" "TWIN-B" "$B_DEVICE" "$B_TCP" "$B_WS"

echo "==> starting station A (device=$A_DEVICE, control=127.0.0.1:$A_TCP)"
XDG_CONFIG_HOME="$WORK/a" RUST_LOG="${RUST_LOG:-info}" "$SERVER" &
PID_A=$!
echo "==> starting station B (device=$B_DEVICE, control=127.0.0.1:$B_TCP)"
XDG_CONFIG_HOME="$WORK/b" RUST_LOG="${RUST_LOG:-info}" "$SERVER" &
PID_B=$!

sleep 1
cat <<EOF

twin-station real-audio rig running (mode=$MODE, profile=$PROFILE, OTA=$OTA):
  station A  control 127.0.0.1:$A_TCP   audio $A_DEVICE
  station B  control 127.0.0.1:$B_TCP   audio $B_DEVICE

Attach the panels:
  openpulse-panel   # Connect 127.0.0.1:$A_TCP  (station A)
  openpulse-panel   # Connect 127.0.0.1:$B_TCP  (station B)

Drive traffic from one station, e.g.:
  openpulse daemon --addr 127.0.0.1:$A_TCP ota-status     # if OTA=1
  # or send a message from a panel; watch B decode it.

Ctrl+C to stop both daemons.
EOF

wait
