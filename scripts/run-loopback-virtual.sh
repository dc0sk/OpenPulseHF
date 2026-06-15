#!/usr/bin/env bash
# Virtual audio loopback transport: TX -> snd-aloop -> RX on a single host.
#
# This is the DEFAULT loopback transport (see docs/dev/virtual-loopback.md):
#   virtual (this script, single clock, no analog) -> hardware (run-loopback-rpi51-rpi52.sh)
#   -> on-air (run-onair-*.sh), each gated on the previous passing.
#
# It drives the modem through the real cpal+ALSA+resampler path but with one
# clock and no cable/isolator, so a failure here is a DSP/code/config issue, not
# an analog or dual-clock-soundcard effect.
#
# Prereq: scripts/setup-virtual-loopback.sh (snd-aloop + aloop_tx/aloop_rx PCMs)
# and a cpal CLI build (cargo build --release -p openpulse-cli).
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

BIN="${OPENPULSE_BIN:-$REPO_ROOT/target/release/openpulse}"
TX_DEVICE="${TX_DEVICE:-aloop_tx}"
RX_DEVICE="${RX_DEVICE:-aloop_rx}"
PRE_WAIT="${PRE_WAIT:-7}"          # let the RX AFC-settling buffer (~6.4s) fill before TX
POST_WAIT="${POST_WAIT:-6}"
LISTEN_MS="${LISTEN_MS:-120000}"
PAYLOAD_BYTES="${PAYLOAD_BYTES:-32}"
RETRIES="${RETRIES:-3}"            # absorb intermittent cpal-TX underrun on slow/wideband modes
OUTPUT_DIR="${OUTPUT_DIR:-docs/dev/test-reports}"

if [[ ! -x "$BIN" ]]; then
    echo "ERROR: cpal CLI not found at $BIN — run: cargo build --release -p openpulse-cli" >&2
    exit 1
fi

# Full mode set straight from the registry (no curated exclusions).
# Override with MODES="MODE1 MODE2 ..." for a targeted run.
if [[ -n "${MODES:-}" ]]; then
    read -r -a ALL_MODES <<< "$MODES"
else
    mapfile -t ALL_MODES < <("$BIN" modes 2>/dev/null \
        | grep -oE '\(([^)]+)\)$' \
        | tr -d '()' | tr ',' '\n' | sed 's/^ *//;s/ *$//' | grep -E '^[A-Z0-9]' | sort -u)
fi

if [[ ${#ALL_MODES[@]} -eq 0 ]]; then
    echo "ERROR: could not enumerate modes from '$BIN modes'" >&2
    exit 1
fi

# Modes that physically cannot run at 8 kHz audio (>=4 samples/symbol needs Fs>=4*baud).
# These are SKIPPED WITH REASON (not silently excluded) until a higher-Fs transport exists.
needs_high_fs() { case "$1" in *9600*) return 0;; *) return 1;; esac; }

ts="$(date -u +%Y-%m-%dT%H%M%SZ)"
mkdir -p "$OUTPUT_DIR"
report="${OUTPUT_DIR}/loopback-virtual-${ts}.json"
results=(); pass=0; fail=0; skip=0

echo "==> virtual loopback (snd-aloop)  ${ts}"
echo "    bin=${BIN}  tx=${TX_DEVICE}  rx=${RX_DEVICE}  modes=${#ALL_MODES[@]}  payload=${PAYLOAD_BYTES}B  retries=${RETRIES}"
echo ""

run_once() {  # mode -> 0 pass / 1 fail ; sets $REASON
    local mode="$1"
    local payload rxlog txlog
    payload="$(python3 -c "import secrets,string;print(''.join(secrets.choice(string.ascii_letters+string.digits) for _ in range(${PAYLOAD_BYTES})))")"
    rxlog="/tmp/openpulse-vloop-rx-${mode}.log"; txlog="/tmp/openpulse-vloop-tx-${mode}.log"
    pkill -x openpulse 2>/dev/null; sleep 0.3
    "$BIN" --backend cpal --log debug --ptt none receive --mode "$mode" \
        --listen-ms "$LISTEN_MS" --device "$RX_DEVICE" --no-afc >"$rxlog" 2>&1 &
    local rxpid=$!
    sleep "$PRE_WAIT"
    if ! kill -0 $rxpid 2>/dev/null; then REASON="rx process died"; return 1; fi
    timeout 90 "$BIN" --backend cpal --log info --ptt none transmit --mode "$mode" \
        --device "$TX_DEVICE" "$payload" >"$txlog" 2>&1
    local txrc=$?
    sleep "$POST_WAIT"
    kill $rxpid 2>/dev/null; wait $rxpid 2>/dev/null
    if [[ $txrc -ne 0 ]]; then
        REASON="tx error: $(grep -iE 'error|too low|unsupported' "$txlog" | grep -v 'ALSA lib' | head -1 | sed 's/^ *//')"
        [[ -z "$REASON" ]] && REASON="tx rc=$txrc"
        return 1
    fi
    if grep -Fq "$payload" "$rxlog"; then REASON=""; return 0; fi
    # A single ALSA underrun is the benign stream-close artifact (logged by
    # snd_pcm_recover when the cpal stream is dropped) and is NOT the cause of a
    # decode failure — don't let it mask the real reason. Only >=2 underruns
    # indicate genuine mid-stream TX pacing trouble.
    local ucount; ucount=$(grep -c "underrun" "$txlog" 2>/dev/null)
    if [[ "${ucount:-0}" -ge 2 ]]; then REASON="tx underrun x${ucount} (rig pacing)"; else REASON="payload not decoded"; fi
    return 1
}

for MODE in "${ALL_MODES[@]}"; do
    if needs_high_fs "$MODE"; then
        printf "  %-18s SKIP (needs Fs>=38.4kHz; 9600 baud impossible at 8kHz)\n" "$MODE"
        results+=("{\"mode\":\"$MODE\",\"result\":\"skip\",\"reason\":\"needs Fs>=38.4kHz\"}"); skip=$((skip+1)); continue
    fi
    ok=1; REASON=""
    for ((r=1; r<=RETRIES; r++)); do
        if run_once "$MODE"; then ok=0; break; fi
    done
    if [[ $ok -eq 0 ]]; then
        printf "  %-18s PASS\n" "$MODE"
        results+=("{\"mode\":\"$MODE\",\"result\":\"pass\",\"reason\":\"\"}"); pass=$((pass+1))
    else
        printf "  %-18s FAIL (%s)\n" "$MODE" "$REASON"
        results+=("{\"mode\":\"$MODE\",\"result\":\"fail\",\"reason\":\"$REASON\"}"); fail=$((fail+1))
    fi
done

pkill -x openpulse 2>/dev/null
echo ""
echo "==> results: ${pass} pass, ${fail} fail, ${skip} skip (of ${#ALL_MODES[@]})"
results_json="$(IFS=,; echo "${results[*]}")"
printf '{"timestamp":"%s","transport":"virtual-snd-aloop","pass":%d,"fail":%d,"skip":%d,"total":%d,"cases":[%s]}\n' \
    "$ts" "$pass" "$fail" "$skip" "${#ALL_MODES[@]}" "$results_json" > "$report"
echo "    report: ${report}"
[[ $fail -eq 0 ]]
