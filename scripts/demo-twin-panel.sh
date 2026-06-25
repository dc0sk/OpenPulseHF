#!/usr/bin/env bash
# One-shot demo: fire up a full two-station OpenPulseHF link with live operator
# visualization, entirely in software — NO radios, NO sound hardware, NO sudo.
#
# What it launches:
#   1. The `twin_station` example — TWO real `openpulse-server` daemons in one
#      process, bridged through a deterministic channel model (A TX -> channel ->
#      B RX, and the reverse). Both run the FULL on-air stack (RateAdapter,
#      HpxReactor, OTA rate-stepping, QSY, repeater), so what you see is the true
#      modem behaviour over a simulated channel, not a reimplemented simulator.
#      Control ports: station A 127.0.0.1:9000, station B 127.0.0.1:9002.
#   2. `openpulse-twinview` — one GUI window showing BOTH stations / both
#      directions (spectrum, waterfall, HPX state, rate ladder), auto-connected
#      to 9000 + 9002.
#   3. `scripts/twin-traffic.sh` — drives random messages A->B and B->A so the
#      panels light up with live RX/TX, rate adaptation, and waterfall energy.
#
# Closing the twinview window (or Ctrl+C) tears everything down.
#
# Env overrides:
#   TWIN_SNR_DB  forward+reverse channel SNR in dB (default 20 — clean-ish HF)
#   INTERVAL     seconds between traffic messages (default 3)
#   SIZE         random message body size in bytes (default 64)
#   COUNT        number of traffic rounds, 0 = forever (default 0)
#   NO_TRAFFIC   set to 1 to skip the traffic generator (drive it by hand)
#   NO_VIEW      set to 1 to skip the GUI (headless: just run the bridged link)
set -euo pipefail

TWIN_SNR_DB="${TWIN_SNR_DB:-20}"
INTERVAL="${INTERVAL:-3}"
SIZE="${SIZE:-64}"
COUNT="${COUNT:-0}"

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_DIR"

A_ADDR="127.0.0.1:9000"   # station A control port (hardcoded in the example)
B_ADDR="127.0.0.1:9002"   # station B control port
TWIN_BIN="$REPO_DIR/target/release/examples/twin_station"
VIEW_BIN="$REPO_DIR/target/release/openpulse-twinview"

# CPU-only builds: the in-process bridge uses LoopbackBackend, so we need neither
# the cpal audio backend (ALSA headers) nor the gpu feature (wgpu) — keeps the
# demo build lean and fast.
echo "==> building bridged-pair example + twinview (CPU-only)..."
cargo build --release -p openpulse-daemon --no-default-features --example twin_station
[[ "${NO_VIEW:-0}" == 1 ]] || cargo build --release -p openpulse-twinview

PID_TWIN=""; PID_TRAFFIC=""; PID_VIEW=""; CLEANED=0
cleanup() {
    [[ "$CLEANED" == 1 ]] && return; CLEANED=1
    echo; echo "==> stopping demo..."
    for pid in "$PID_TRAFFIC" "$PID_VIEW" "$PID_TWIN"; do
        [[ -n "$pid" ]] && kill "$pid" 2>/dev/null || true
    done
    wait 2>/dev/null || true
}
trap cleanup EXIT INT TERM

# Launch the bridged daemon pair (serves 9000 + 9002).
echo "==> starting bridged two-station link (channel: AWGN ${TWIN_SNR_DB} dB)..."
TWIN_SNR_DB="$TWIN_SNR_DB" RUST_LOG="${RUST_LOG:-info}" "$TWIN_BIN" &
PID_TWIN=$!

# Wait for both control ports to accept connections.
wait_port() {
    local host="${1%:*}" port="${1##*:}" tries=0
    until (exec 3<>"/dev/tcp/${host}/${port}") 2>/dev/null; do
        exec 3<&- 3>&- 2>/dev/null || true
        tries=$((tries + 1))
        if [[ $tries -gt 100 ]]; then
            echo "ERROR: $1 never came up — is the build OK?" >&2
            exit 1
        fi
        sleep 0.2
    done
    exec 3<&- 3>&- 2>/dev/null || true
}
echo "==> waiting for control ports..."
wait_port "$A_ADDR"
wait_port "$B_ADDR"
echo "==> link up: station A ${A_ADDR} (TWIN-A)  |  station B ${B_ADDR} (TWIN-B)"

# Drive traffic both directions so both panels show activity.
if [[ "${NO_TRAFFIC:-0}" != 1 ]]; then
    echo "==> generating traffic A<->B every ${INTERVAL}s (${SIZE} B)..."
    ADDR2="$B_ADDR" TO="TWIN-B" TO2="TWIN-A" \
        INTERVAL="$INTERVAL" SIZE="$SIZE" COUNT="$COUNT" \
        "$REPO_DIR/scripts/twin-traffic.sh" "$A_ADDR" &
    PID_TRAFFIC=$!
fi

# Launch the live two-station view (foreground: closing it ends the demo).
if [[ "${NO_VIEW:-0}" == 1 ]]; then
    cat <<EOF

Headless mode — no GUI. The bridged link is running:
  station A  ${A_ADDR}  (TWIN-A)
  station B  ${B_ADDR}  (TWIN-B)
Attach a viewer yourself, e.g.:
  target/release/openpulse-twinview ${A_ADDR} ${B_ADDR}
Ctrl+C to stop.
EOF
    wait "$PID_TWIN"
else
    echo "==> launching twinview (close the window to stop the demo)..."
    "$VIEW_BIN" "$A_ADDR" "$B_ADDR" || true
fi
