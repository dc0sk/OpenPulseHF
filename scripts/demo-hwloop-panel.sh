#!/usr/bin/env bash
# One-shot demo: a real-audio modem link over the two-soundcard HARDWARE loopback,
# with the operator panel showing the spectrum/waterfall captured off the cable.
# No radios, no RF — but a real cpal+ALSA audio path with two independent clocks.
#
#   openpulse CLI  --(hwloop_tx, USB card A out)--> analog cable
#       --> (hwloop_rx, USB card B in) --> openpulse-server (RX) --> panel
#
# The receive daemon binds cpal on the RX card and serves its control port; the
# panel attaches and shows the REAL captured audio (DCD energy, AFC, spectrum,
# waterfall) and the decoded frames. A separate CLI transmits modem bursts out
# the TX card, so what you watch is genuine off-the-wire signal, not a simulation.
#
# This is the hardware/dual-clock rung: unlike the all-virtual twin demo
# (scripts/demo-twin-panel.sh), the two cards have independent sample clocks and a
# real analog cable, so it exercises sample-rate offset and analog effects.
#
# Prereq: two USB soundcards cabled TX-card line-out -> RX-card mic-in, and a cpal
# build. The script runs scripts/setup-dualcard-loopback.sh for you (publishes the
# hwloop_tx / hwloop_rx PCMs). If your cards are on different USB ports, set
# TX_BYPATH / RX_BYPATH (see that script) or run it once yourself and SKIP_SETUP=1.
#
# Env overrides:
#   MODE        modulation mode (default BPSK250 — robust on the dual-clock rig)
#   INTERVAL    seconds between transmitted bursts (default 5)
#   COUNT       number of bursts, 0 = forever (default 0)
#   SIZE        random payload size in bytes (default 32)
#   TX_DEVICE   cpal output PCM (default hwloop_tx)
#   RX_DEVICE   cpal input PCM  (default hwloop_rx)
#   RX_CALLSIGN receive-daemon callsign (default HWLOOP-RX)
#   SKIP_SETUP  set to 1 to skip the dual-card ALSA setup step
#   NO_PANEL    set to 1 to skip the GUI (headless: just run the link)
set -euo pipefail

MODE="${MODE:-BPSK250}"
INTERVAL="${INTERVAL:-5}"
COUNT="${COUNT:-0}"
SIZE="${SIZE:-32}"
TX_DEVICE="${TX_DEVICE:-hwloop_tx}"
RX_DEVICE="${RX_DEVICE:-hwloop_rx}"
RX_CALLSIGN="${RX_CALLSIGN:-HWLOOP-RX}"

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_DIR"

RX_ADDR="127.0.0.1:9000"   # the panel's default Connect target
SERVER="$REPO_DIR/target/release/openpulse-server"
CLI="$REPO_DIR/target/release/openpulse"
PANEL="$REPO_DIR/target/release/openpulse-panel"

# Real audio needs the cpal backend: the daemon gates it behind --features cpal,
# while the CLI ships cpal-backend in its DEFAULT features (so a plain build).
echo "==> building openpulse-server (cpal) + openpulse CLI + panel..."
cargo build --release -p openpulse-daemon --features cpal --bin openpulse-server
cargo build --release -p openpulse-cli
[[ "${NO_PANEL:-0}" == 1 ]] || cargo build --release -p openpulse-panel

# Publish the hwloop_tx / hwloop_rx PCMs (idempotent).
if [[ "${SKIP_SETUP:-0}" != 1 ]]; then
    echo "==> setting up the two-soundcard loopback (hwloop_tx / hwloop_rx)..."
    "$REPO_DIR/scripts/setup-dualcard-loopback.sh"
fi

# Verify cpal enumerates both PCMs by exact name (same gate the sweep uses).
echo "==> checking cpal sees both loopback devices..."
if ! "$CLI" --backend cpal devices 2>/dev/null | grep -q "^${TX_DEVICE}\b" \
   || ! "$CLI" --backend cpal devices 2>/dev/null | grep -q "^${RX_DEVICE}\b"; then
    echo "ERROR: cpal does not list both '${TX_DEVICE}' and '${RX_DEVICE}'." >&2
    echo "       Are both USB soundcards plugged in and cabled? Try re-running" >&2
    echo "       scripts/setup-dualcard-loopback.sh (set TX_BYPATH/RX_BYPATH if" >&2
    echo "       your cards live on different USB ports: ls /dev/snd/by-path)." >&2
    "$CLI" --backend cpal devices 2>/dev/null || true
    exit 1
fi

# Receive daemon config: cpal on the RX card, control port 9000, no PTT/CAT.
WORK="$(mktemp -d /tmp/openpulse-hwloop.XXXXXX)"
mkdir -p "$WORK/openpulse"
cat > "$WORK/openpulse/config.toml" <<EOF
[station]
callsign = "$RX_CALLSIGN"
grid_square = "AA00"
[audio]
backend = "cpal"
device = "$RX_DEVICE"
[modem]
mode = "$MODE"
profile = "hpx_hf"
ptt_backend = "none"
[daemon]
tcp_port = 9000
websocket_port = 9001
[radio]
cat_backend = "none"
EOF

PID_RX=""; PID_PANEL=""; PID_TX=""; CLEANED=0
cleanup() {
    [[ "$CLEANED" == 1 ]] && return; CLEANED=1
    echo; echo "==> stopping demo..."
    for pid in "$PID_TX" "$PID_PANEL" "$PID_RX"; do
        [[ -n "$pid" ]] && kill "$pid" 2>/dev/null || true
    done
    wait 2>/dev/null || true
    rm -rf "$WORK"
}
trap cleanup EXIT INT TERM

echo "==> starting receive daemon (cpal in=${RX_DEVICE}, control ${RX_ADDR}, ${RX_CALLSIGN})..."
XDG_CONFIG_HOME="$WORK" RUST_LOG="${RUST_LOG:-info}" "$SERVER" &
PID_RX=$!

# Wait for the control port.
wait_port() {
    local host="${1%:*}" port="${1##*:}" tries=0
    until (exec 3<>"/dev/tcp/${host}/${port}") 2>/dev/null; do
        exec 3<&- 3>&- 2>/dev/null || true
        tries=$((tries + 1))
        [[ $tries -gt 100 ]] && { echo "ERROR: ${1} never came up." >&2; exit 1; }
        sleep 0.2
    done
    exec 3<&- 3>&- 2>/dev/null || true
}
wait_port "$RX_ADDR"
echo "==> receive daemon up — capturing off the cable on ${RX_DEVICE}."

if [[ "${NO_PANEL:-0}" != 1 ]]; then
    echo "==> launching panel — click Connect (address is prefilled to ${RX_ADDR})."
    "$PANEL" &
    PID_PANEL=$!
fi

# Give the RX path a few seconds to settle before the first burst.
sleep 3

# Transmit modem bursts out the TX card on a loop; each burst crosses the real
# cable and the daemon decodes it (FEC None matches the daemon's RX path), so the
# panel shows live energy + decoded frames.
echo "==> transmitting ${MODE} bursts out ${TX_DEVICE} every ${INTERVAL}s (FEC none)."
i=0
rand_body() { LC_ALL=C tr -dc 'A-Za-z0-9' </dev/urandom | head -c "$SIZE"; }
while :; do
    i=$((i + 1))
    payload="HWLOOP $MODE SEQ$(printf '%03d' "$i") $(rand_body)"
    if "$CLI" --backend cpal --log error --ptt none transmit \
            --mode "$MODE" --fec none --device "$TX_DEVICE" "$payload" >/dev/null 2>&1; then
        echo "[$i] TX ${TX_DEVICE} -> cable -> ${RX_DEVICE}  (${#payload} B)  watch the panel decode it"
    else
        echo "[$i] TX failed (is ${TX_DEVICE} still present?)" >&2
    fi
    if [[ "$COUNT" -ne 0 && "$i" -ge "$COUNT" ]]; then
        echo "done (${COUNT} burst(s))."
        break
    fi
    sleep "$INTERVAL"
done

# Keep the daemon + panel alive after the last burst so you can inspect the panel.
[[ "${NO_PANEL:-0}" == 1 ]] || { echo "==> bursts done; panel still live. Ctrl+C to stop."; wait "$PID_RX"; }
