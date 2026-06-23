#!/usr/bin/env bash
# Generate test traffic for the twin-station rig: send random-data messages to a
# running daemon's NDJSON control port in a loop, so the panels show live RX/TX
# activity, rate adaptation, and waterfall energy without hand-typing anything.
#
# Each iteration opens the control port, sends one `send_message` with a random
# alphanumeric body (JSON-safe), and closes. The daemon transmits it at the
# active mode over the (snd-aloop / cabled) air; the peer decodes and its panel
# lights up. Uses bash's /dev/tcp, so no nc/socat needed.
#
# Usage:
#   scripts/twin-traffic.sh                 # → 127.0.0.1:9000, every 2 s, forever
#   scripts/twin-traffic.sh 127.0.0.1:9002  # drive station B instead
#   ADDR2=127.0.0.1:9002 scripts/twin-traffic.sh   # both directions (A→B and B→A)
#
# Env overrides:
#   ADDR     primary daemon control addr (default 127.0.0.1:9000); or pass as $1
#   ADDR2    second daemon addr for reverse-direction traffic (default: none)
#   TO/TO2   recipient callsign for ADDR / ADDR2 (default TWIN-B / TWIN-A)
#   INTERVAL seconds between rounds (default 2)
#   SIZE     random body size in bytes (default 64)
#   COUNT    number of rounds, 0 = infinite (default 0)
set -uo pipefail

ADDR="${1:-${ADDR:-127.0.0.1:9000}}"
ADDR2="${ADDR2:-}"
TO="${TO:-TWIN-B}"
TO2="${TO2:-TWIN-A}"
INTERVAL="${INTERVAL:-2}"
SIZE="${SIZE:-64}"
COUNT="${COUNT:-0}"

rand_body() {
    LC_ALL=C tr -dc 'A-Za-z0-9' </dev/urandom | head -c "$SIZE"
}

# Open the control port, send one send_message line, drain the response, close.
send_msg() {
    local addr="$1" to="$2" subj="$3" body="$4"
    local host="${addr%:*}" port="${addr##*:}"
    if ! exec 3<>"/dev/tcp/${host}/${port}"; then
        echo "  connect to ${addr} failed (is the daemon up?)" >&2
        return 1
    fi
    printf '{"cmd":"send_message","to":"%s","subject":"%s","body":"%s"}\n' \
        "$to" "$subj" "$body" >&3
    # Give the daemon a moment to read the line and respond before closing.
    read -r -t 2 _resp <&3 || true
    exec 3<&- 3>&-
}

trap 'echo; echo "stopped after ${i:-0} round(s)."; exit 0' INT TERM

echo "twin-traffic: ${SIZE}-byte random messages every ${INTERVAL}s"
echo "  ${ADDR} → ${TO}${ADDR2:+   and   ${ADDR2} → ${TO2}}"
echo "  (Ctrl+C to stop)"

i=0
while :; do
    i=$((i + 1))
    send_msg "$ADDR" "$TO" "traffic-$i" "$(rand_body)" \
        && echo "[$i] ${ADDR} → ${TO}  (${SIZE} B)"
    if [[ -n "$ADDR2" ]]; then
        send_msg "$ADDR2" "$TO2" "traffic-$i" "$(rand_body)" \
            && echo "[$i] ${ADDR2} → ${TO2}  (${SIZE} B)"
    fi
    if [[ "$COUNT" -ne 0 && "$i" -ge "$COUNT" ]]; then
        echo "done (${COUNT} round(s))."
        break
    fi
    sleep "$INTERVAL"
done
