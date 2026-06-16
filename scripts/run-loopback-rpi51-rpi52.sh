#!/usr/bin/env bash
# Audio loopback test: rpi51 (ISS/TX) → USB soundcard cable → rpi52 (IRS/RX)
#
# No RF, no PTT, no rigctld.  Tests the software modem stack end-to-end with
# real audio hardware to isolate software bugs from RF/transceiver variables.
#
# Hardware (2026-06-12: rpi52's CM108 replaced with the same C-Media
# "USB Audio Device" model as rpi51 — both ends now use the same card):
#   rpi51 CARD=Device speaker out → cable → rpi52 CARD=Device mic in
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
ISS_DEVICE="${ISS_DEVICE:-plughw:CARD=Device,DEV=0}"
IRS_DEVICE="${IRS_DEVICE:-plughw:CARD=Device,DEV=0}"

# ── Timing ─────────────────────────────────────────────────────────────────────
# AFC settling needs ~51200 samples at 8 kHz = 6.4 s before frame detection
# begins. ISS must not transmit until after that window or the frame lands in
# the settling buffer and is never scanned. 10 s gives ~3.6 s margin.
IRS_STARTUP_WAIT="${IRS_STARTUP_WAIT:-10}"  # seconds before ISS starts TX
IRS_LISTEN_MS="${IRS_LISTEN_MS:-45000}"     # IRS receive window (ms)
TX_TIMEOUT="${TX_TIMEOUT:-60}"              # hard ISS transmit timeout (s)
FEC="${FEC:-none}"                          # FEC codec (none|rs|rs-interleaved|soft-concatenated|ldpc)
KILL_WAIT="${KILL_WAIT:-12}"                # seconds after TX before killing IRS

# ── Test matrix ────────────────────────────────────────────────────────────────
# Quick tier: one case per mode family; fast enough for repeated regression checks.
# BPSK31 excluded (full-tier only) — still KNOWN-FAILING on this rig: its ~12 s frame
#   is not reliably acquired; the decode scan never lands on the preamble (positions
#   tried start well past it).  This is a long-frame acquisition issue, separate from
#   the AFC carrier-offset path (whose one-symbol-short settling window was fixed in
#   engine.rs `afc_window`).  Tracked as a follow-up.
# QPSK500 excluded: AFC anchor fires at preamble start, retry misses by 200 samples (engine bug).
QUICK_CASES=(
    "BPSK100|64"
    "BPSK250|64"
    "QPSK125|64"
    "QPSK250|64"
)

# Full tier: broader coverage across baud rates and payload sizes.
FULL_CASES=(
    "BPSK31|32"
    "BPSK63|32"
    "BPSK100|64"
    "BPSK250|64"
    "QPSK125|64"
    "QPSK250|64"
    "QPSK500|128"
    "QPSK1000|128"
)

TIER="${TIER:-quick}"
OUTPUT_DIR="${OUTPUT_DIR:-docs/dev/test-reports}"
SINGLE_CASE=""
_LEGACY_MODE=""
_LEGACY_PAYLOAD="64"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --quick)       TIER="quick";         shift ;;
        --full)        TIER="full";          shift ;;
        --output)      OUTPUT_DIR="$2";      shift 2 ;;
        --single-case) SINGLE_CASE="$2";     shift 2 ;;
        --mode)        _LEGACY_MODE="$2";    shift 2 ;;
        --payload)     _LEGACY_PAYLOAD="$2"; shift 2 ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
done

# Resolve legacy --mode / --payload pair into a single-case spec.
[[ -n "${_LEGACY_MODE}" ]] && SINGLE_CASE="${_LEGACY_MODE}|${_LEGACY_PAYLOAD}"

if [[ -n "${SINGLE_CASE}" ]]; then
    CASES=("${SINGLE_CASE}")
elif [[ "${TIER}" == "full" ]]; then
    CASES=("${FULL_CASES[@]}")
else
    CASES=("${QUICK_CASES[@]}")
fi

pass=0
fail=0
total="${#CASES[@]}"

ts="$(date -u +%Y-%m-%dT%H%M%SZ)"
mkdir -p "${OUTPUT_DIR}"
report="${OUTPUT_DIR}/loopback-${TIER}-${ts}.json"
results=()

echo "==> Audio loopback test (${ts})  tier=${TIER}  cases=${total}"
echo "    ISS: ${ISS_SSH}  device=${ISS_DEVICE}"
echo "    IRS: ${IRS_SSH}  device=${IRS_DEVICE}"
echo "    IRS listen: ${IRS_LISTEN_MS} ms  report=${report}"
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

# Verify devices are present by USB by-path symlinks (set by identify-loopback-audio.sh).
# If not set, fall back to ALSA name check.
if [[ -n "${ISS_DEVICE_BYPATH:-}" ]]; then
    iss_path_name="${ISS_DEVICE_BYPATH##*/}"
    iss_path_ok="$(ssh_iss "ls /dev/snd/by-path/ 2>/dev/null | grep -Fq '${iss_path_name}' && echo ok || echo missing" || echo missing)"
    if [[ "$iss_path_ok" != "ok" ]]; then
        echo "FAIL: ISS audio device not found at by-path '${iss_path_name}' on ${ISS_SSH}" >&2
        echo "      Check USB cable connection and run scripts/identify-loopback-audio.sh to refresh." >&2
        exit 1
    fi
    echo "    ISS device: ok (by-path ${iss_path_name})"
else
    _iss_card_name="${ISS_DEVICE##*CARD=}"; _iss_card_name="${_iss_card_name%%,*}"
    iss_dev_ok="$(ssh_iss "aplay -l 2>/dev/null | grep -Fq ': ${_iss_card_name} [' && echo ok || echo missing" || echo missing)"
    if [[ "$iss_dev_ok" != "ok" ]]; then
        echo "WARN: ISS device '${ISS_DEVICE}' not found — will attempt anyway"
    else
        echo "    ISS device: ok (by ALSA name)"
    fi
fi

if [[ -n "${IRS_DEVICE_BYPATH:-}" ]]; then
    irs_path_name="${IRS_DEVICE_BYPATH##*/}"
    irs_path_ok="$(ssh_irs "ls /dev/snd/by-path/ 2>/dev/null | grep -Fq '${irs_path_name}' && echo ok || echo missing" || echo missing)"
    if [[ "$irs_path_ok" != "ok" ]]; then
        echo "FAIL: IRS audio device not found at by-path '${irs_path_name}' on ${IRS_SSH}" >&2
        echo "      Check USB cable connection and run scripts/identify-loopback-audio.sh to refresh." >&2
        exit 1
    fi
    echo "    IRS device: ok (by-path ${irs_path_name})"
else
    _irs_card_name="${IRS_DEVICE##*CARD=}"; _irs_card_name="${_irs_card_name%%,*}"
    irs_dev_ok="$(ssh_irs "arecord -l 2>/dev/null | grep -Fq ': ${_irs_card_name} [' && echo ok || echo missing" || echo missing)"
    if [[ "$irs_dev_ok" != "ok" ]]; then
        echo "WARN: IRS device '${IRS_DEVICE}' not found — will attempt anyway"
    else
        echo "    IRS device: ok (by ALSA name)"
    fi
fi

echo ""

# ── Audio level normalisation ─────────────────────────────────────────────────
# Extract ALSA card names from the device strings.
_irs_card="${IRS_DEVICE#*CARD=}"; _irs_card="${_irs_card%%,*}"

# Disable AGC and reset capture volume on IRS before every test case.
# The C-Media hardware AGC reduces Mic Capture Volume after successive strong
# signals.  After BPSK100 + BPSK250 both decode cleanly, the volume drops
# enough to push the QPSK carrier below ENERGY_GATE_THRESHOLD (0.0001),
# delaying fep by 3+ seconds and causing the frame to miss the receive window.
# Reset volume to max and disable AGC to prevent further drift during the test.
#
# 2026-06-12: rpi52's CM108 ("USB PnP Sound Device") was replaced with the same
# C-Media "USB Audio Device" model used on rpi51.  On this model the capture
# controls are NOT exposed under the simple-mixer names 'Mic Capture Volume' /
# 'Mic Capture Switch' (amixer set fails), only as raw controls — use cset.
# Capture range is 0–35 (35 = 23.0 dB max, ≈ the old CM108's 16 = 23.81 dB).
# The raw-control names are identical on both models, so cset works for either.
_normalise_irs_levels() {
    ssh_irs "
        amixer -c '${_irs_card}' cset name='Auto Gain Control' 0  >/dev/null 2>&1 || true
        amixer -c '${_irs_card}' cset name='Mic Capture Switch' 1 >/dev/null 2>&1 || true
        amixer -c '${_irs_card}' cset name='Mic Capture Volume' 35 >/dev/null 2>&1 || true
    " 2>/dev/null || true
}
_normalise_irs_levels  # Once at session start before the first case.

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
    _normalise_irs_levels  # Reset AGC/capture volume before each case.
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
            --fec '${FEC}' \
            --listen-ms ${IRS_LISTEN_MS} \
            --device '${IRS_DEVICE}' \
            --no-afc \
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
            --fec '${FEC}' \
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
