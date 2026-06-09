#!/usr/bin/env bash
# Audio loopback test: rpi51 (ISS/TX) → USB soundcard cable → rpi52 (IRS/RX)
#
# No RF, no PTT, no rigctld.  Tests the software modem stack end-to-end with
# real audio hardware to isolate software bugs from RF/transceiver variables.
#
# Hardware:
#   rpi51 card 4 (Device_1) speaker out → cable → rpi52 card 3 (Device) mic in
#
# Usage:
#   ./scripts/run-loopback-rpi51-rpi52.sh
#   ./scripts/run-loopback-rpi51-rpi52.sh --mode BPSK250 --payload 64

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# ── SSH targets ────────────────────────────────────────────────────────────────
ISS_SSH="${ISS_SSH:-dc0sk@dc0sk-rpi51}"
IRS_SSH="${IRS_SSH:-dc0sk@dc0sk-rpi52}"
SSH_OPTS="${SSH_OPTS:--o BatchMode=yes -o ConnectTimeout=10}"

ssh_iss() { ssh ${SSH_OPTS} "${ISS_SSH}" "$@"; }
ssh_irs() { ssh ${SSH_OPTS} "${IRS_SSH}" "$@"; }

# ── Binaries ───────────────────────────────────────────────────────────────────
ISS_BIN="${ISS_BIN:-/home/dc0sk/git/OpenPulseHF/target/release/openpulse}"
IRS_BIN="${IRS_BIN:-/home/dc0sk/openpulse/bin/openpulse}"

# ── Audio devices ──────────────────────────────────────────────────────────────
# plughw lets ALSA resample 8 kHz ↔ 48 kHz; the cards only support 44100/48000.
ISS_DEVICE="${ISS_DEVICE:-plughw:CARD=Device_1,DEV=0}"
IRS_DEVICE="${IRS_DEVICE:-plughw:CARD=Device,DEV=0}"

# ── Timing ─────────────────────────────────────────────────────────────────────
# AFC settling needs ~51200 samples at 8 kHz = 6.4 s before frame detection
# begins. ISS must not transmit until after that window or the frame lands in
# the settling buffer and is never scanned. 10 s gives ~3.6 s margin.
IRS_STARTUP_WAIT="${IRS_STARTUP_WAIT:-10}"  # seconds before ISS starts TX
IRS_LISTEN_MS="${IRS_LISTEN_MS:-30000}"     # IRS receive window (ms)
TX_TIMEOUT="${TX_TIMEOUT:-60}"              # hard ISS transmit timeout (s)
KILL_WAIT="${KILL_WAIT:-12}"                # seconds after TX before killing IRS

# ── Test matrix ────────────────────────────────────────────────────────────────
DEFAULT_MODE="${DEFAULT_MODE:-BPSK250}"
DEFAULT_PAYLOAD="${DEFAULT_PAYLOAD:-64}"

# Parse optional overrides: --mode X --payload N
while [[ $# -gt 0 ]]; do
    case "$1" in
        --mode)    DEFAULT_MODE="$2";    shift 2 ;;
        --payload) DEFAULT_PAYLOAD="$2"; shift 2 ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
done

CASES=(
    "${DEFAULT_MODE}|${DEFAULT_PAYLOAD}"
)

pass=0
fail=0
total="${#CASES[@]}"

ts="$(date -u +%Y-%m-%dT%H%M%SZ)"
report="/tmp/loopback-report-${ts}.json"
results=()

echo "==> Audio loopback test (${ts})"
echo "    ISS: ${ISS_SSH}  device=${ISS_DEVICE}"
echo "    IRS: ${IRS_SSH}  device=${IRS_DEVICE}"
echo "    IRS listen: ${IRS_LISTEN_MS} ms"
echo ""

# ── Preflight ─────────────────────────────────────────────────────────────────
echo "==> Preflight"

iss_bin_ok="$(ssh_iss "test -x '${ISS_BIN}' && echo ok || echo missing" 2>/dev/null || echo missing)"
irs_bin_ok="$(ssh_irs "test -x '${IRS_BIN}' && echo ok || echo missing" 2>/dev/null || echo missing)"
if [[ "$iss_bin_ok" != "ok" ]]; then
    echo "FAIL: ISS binary missing at ${ISS_BIN} on ${ISS_SSH}" >&2; exit 1
fi
if [[ "$irs_bin_ok" != "ok" ]]; then
    echo "FAIL: IRS binary missing at ${IRS_BIN} on ${IRS_SSH}" >&2; exit 1
fi
echo "    binaries: ok"

# Verify IRS device is visible
irs_dev_ok="$(ssh_irs "~/openpulse/bin/openpulse --backend cpal devices 2>/dev/null | grep -Fq '${IRS_DEVICE}' && echo ok || echo missing" || echo missing)"
if [[ "$irs_dev_ok" != "ok" ]]; then
    echo "WARN: IRS device '${IRS_DEVICE}' not found in cpal device list — will attempt anyway"
else
    echo "    IRS device: ok"
fi

echo ""

# ── Test loop ─────────────────────────────────────────────────────────────────
for case_spec in "${CASES[@]}"; do
    IFS='|' read -r MODE PAYLOAD_SIZE <<< "$case_spec"

    payload_text="$(python3 -c "
import secrets, string, sys
a = string.ascii_letters + string.digits
sys.stdout.write(''.join(secrets.choice(a) for _ in range(${PAYLOAD_SIZE})))
")"

    irs_log="/tmp/openpulse-loopback-irs-${MODE}.log"
    iss_log="/tmp/openpulse-loopback-iss-${MODE}.log"

    printf "  [%-10s payload=%4sB] ... " "${MODE}" "${PAYLOAD_SIZE}"

    # 1) Start IRS receiver on rpi52.
    ssh_irs "pids=\$(pgrep -f '${IRS_BIN}.*receive' || true); \
        for pid in \$pids; do \
            [[ \"\$pid\" != \"\$\$\" ]] && kill \"\$pid\" 2>/dev/null || true; \
        done; \
        nohup '${IRS_BIN}' \
            --backend cpal \
            --log debug \
            --ptt none \
            receive \
            --mode '${MODE}' \
            --listen-ms ${IRS_LISTEN_MS} \
            --device '${IRS_DEVICE}' \
            >'${irs_log}' 2>&1 </dev/null &"

    sleep "${IRS_STARTUP_WAIT}"

    # Verify IRS is still running before transmitting.
    irs_alive="$(ssh_irs "pgrep -f '${IRS_BIN}.*receive' >/dev/null && echo yes || echo no" || echo no)"
    if [[ "$irs_alive" != "yes" ]]; then
        boot_log="$(ssh_irs "tail -n 10 '${irs_log}' 2>/dev/null || true" || true)"
        echo "FAIL (IRS not running)"
        fail=$(( fail + 1 ))
        results+=("{\"mode\":\"${MODE}\",\"payload_bytes\":${PAYLOAD_SIZE},\"result\":\"fail\",\"fail_reason\":\"IRS not running\"}")
        if [[ -n "$boot_log" ]]; then
            echo "    IRS boot log:"
            echo "$boot_log" | sed 's/^/      /'
        fi
        continue
    fi

    # 2) Transmit from rpi51.
    iss_exit=0
    timeout "${TX_TIMEOUT}" ssh ${SSH_OPTS} "${ISS_SSH}" \
        "'${ISS_BIN}' \
            --backend cpal \
            --log info \
            --ptt none \
            transmit \
            --mode '${MODE}' \
            --device '${ISS_DEVICE}' \
            '${payload_text}' \
            >'${iss_log}' 2>&1" \
        || iss_exit=$?

    # 3) Give the IRS receive window a moment to catch the tail, then kill.
    sleep "${KILL_WAIT}"
    ssh_irs "pids=\$(pgrep -f '${IRS_BIN}.*receive' || true); \
        for pid in \$pids; do \
            [[ \"\$pid\" != \"\$\$\" ]] && kill \"\$pid\" 2>/dev/null || true; \
        done"

    irs_content="$(ssh_irs "cat '${irs_log}' 2>/dev/null || true" || true)"

    # 4) Judge result.
    test_pass=false
    fail_reason=""
    if [[ $iss_exit -ne 0 ]]; then
        fail_reason="ISS exit ${iss_exit}"
    elif echo "${irs_content}" | grep -Fq "${payload_text}"; then
        test_pass=true
    else
        fail_reason="payload not in IRS output"
    fi

    if $test_pass; then
        echo "PASS"
        pass=$(( pass + 1 ))
        results+=("{\"mode\":\"${MODE}\",\"payload_bytes\":${PAYLOAD_SIZE},\"result\":\"pass\",\"fail_reason\":\"\"}")
    else
        echo "FAIL (${fail_reason})"
        fail=$(( fail + 1 ))
        results+=("{\"mode\":\"${MODE}\",\"payload_bytes\":${PAYLOAD_SIZE},\"result\":\"fail\",\"fail_reason\":\"${fail_reason}\"}")

        # Show ISS log tail for transmit issues.
        iss_tail="$(ssh_iss "tail -n 5 '${iss_log}' 2>/dev/null || true" || true)"
        if [[ -n "$iss_tail" ]]; then
            echo "    ISS log tail:"
            echo "$iss_tail" | sed 's/^/      /'
        fi

        # Show IRS AFC settling line explicitly (doesn't matter where it falls).
        irs_afc="$(echo "${irs_content}" | grep -E 'AFC settling|AFC decode|AFC:' || true)"
        if [[ -n "$irs_afc" ]]; then
            echo "    IRS AFC lines:"
            echo "$irs_afc" | sed 's/^/      /'
        fi

        # Show IRS tail for decode issues.
        irs_tail="$(echo "${irs_content}" | tail -n 20)"
        echo "    IRS log tail:"
        echo "$irs_tail" | sed 's/^/      /'
    fi
done

# ── Report ────────────────────────────────────────────────────────────────────
echo ""
echo "==> Results: ${pass}/${total} passed, ${fail} failed."

results_json="$(IFS=,; echo "${results[*]}")"
cat > "${report}" <<JSON
{
  "timestamp": "${ts}",
  "iss": "${ISS_SSH}",
  "irs": "${IRS_SSH}",
  "iss_device": "${ISS_DEVICE}",
  "irs_device": "${IRS_DEVICE}",
  "total": ${total},
  "pass": ${pass},
  "fail": ${fail},
  "cases": [${results_json}]
}
JSON
echo "    Report: ${report}"

[[ $fail -eq 0 ]]
