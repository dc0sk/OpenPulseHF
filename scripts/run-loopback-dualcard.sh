#!/usr/bin/env bash
# Hardware audio loopback on a SINGLE host through two USB soundcards.
#
#   modem TX --(hwloop_tx, USB card A out)--> analog cable --> (hwloop_rx, USB
#   card B in) --> modem RX
#
# Both ends run locally as two cpal CLI processes -- no SSH, no two Raspberry
# Pis. This is the hardware/dual-clock rung of the loopback ladder (see
# docs/dev/dualcard-loopback.md): unlike the single-clock virtual rig
# (run-loopback-virtual.sh), the two cards have independent sample clocks, so it
# reproduces the sample-rate-offset (SRO) and analog-cable effects that broke the
# wideband multicarrier / dense-QAM modes on the two-Pi rig -- the missing rung
# that previously required two machines.
#
# Prereq: scripts/setup-dualcard-loopback.sh (hwloop_tx / hwloop_rx PCMs +
# mixer normalisation) and a cpal CLI build (cargo build --release -p openpulse-cli).
#
# Usage:
#   scripts/run-loopback-dualcard.sh --quick
#   scripts/run-loopback-dualcard.sh --full
#   scripts/run-loopback-dualcard.sh --single-case "BPSK250|64"
#   scripts/run-loopback-dualcard.sh --level-check        # verify cable direction
#   FEC=soft-concatenated scripts/run-loopback-dualcard.sh --single-case "SCFDMA26-16QAM|64"
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

BIN="${OPENPULSE_BIN:-$REPO_ROOT/target/release/openpulse}"
TX_DEVICE="${TX_DEVICE:-hwloop_tx}"
RX_DEVICE="${RX_DEVICE:-hwloop_rx}"

# USB card indices for per-case mixer re-normalisation (the C-Media hardware AGC
# drifts capture gain down after strong frames). Auto-resolved from the hwloop
# PCM slaves in ~/.asoundrc; override if needed.
# pcm-name -> ALSA card INDEX (amixer needs an index, not a name).
#
# Accepts both slave forms: the fragile "hw:N,0" and the stable
# "hw:CARD=Name,DEV=0". Prefer the name form in ~/.asoundrc: ALSA assigns card
# indices in enumeration order, so they SHIFT when devices are re-probed — on
# 2026-07-19 `acp` moved 3 -> 4 mid-session and hwloop_tx silently started
# pointing at the laptop's internal audio instead of the USB adapter.
_slave_card() {
    local spec
    spec="$(awk -v p="pcm.$1" '$0 ~ p {f=1} f && /slave.pcm/ {print; exit}' "${HOME}/.asoundrc" 2>/dev/null)"
    # Name form: resolve through /proc/asound/<name> -> cardN
    local nm
    nm="$(printf '%s' "$spec" | sed -n 's/.*CARD=\([A-Za-z0-9_]*\).*/\1/p')"
    if [[ -n "$nm" ]]; then
        local link
        link="$(readlink -f "/proc/asound/$nm" 2>/dev/null)"
        [[ -n "$link" ]] && basename "$link" | sed 's/^card//' && return
    fi
    # Index form
    printf '%s' "$spec" | sed -n 's/.*hw:\([0-9]\+\).*/\1/p'
}
TX_CARD="${TX_CARD:-$(_slave_card hwloop_tx)}"
RX_CARD="${RX_CARD:-$(_slave_card hwloop_rx)}"

IRS_STARTUP_WAIT="${IRS_STARTUP_WAIT:-10}"  # RX AFC settle (~6.4 s) + margin before TX
# Defaults are FLOORS. Per case they are raised to fit the mode's own airtime -- see
# `airtime_for`. A fixed window silently truncates the slow rungs: a 255-byte RS block at
# BPSK31's 31.25 baud is 65 s of audio, so the 60 s transmit timeout and 45 s listen window
# reported BPSK31 as a mode failure when nothing was wrong with the waveform.
IRS_LISTEN_MS_FLOOR="${IRS_LISTEN_MS:-45000}"
TX_TIMEOUT_FLOOR="${TX_TIMEOUT:-60}"
IRS_LISTEN_EXPLICIT="${IRS_LISTEN_MS+set}"
TX_TIMEOUT_EXPLICIT="${TX_TIMEOUT+set}"
IRS_LISTEN_MS="$IRS_LISTEN_MS_FLOOR"
TX_TIMEOUT="$TX_TIMEOUT_FLOOR"
KILL_WAIT="${KILL_WAIT:-12}"
RETRIES="${RETRIES:-3}"                      # absorb transient wideband acquisition flakiness in long sweeps (matches run-loopback-virtual.sh)
FEC_EXPLICIT="${FEC+set}"
FEC="${FEC:-none}"                          # none|rs|rs-interleaved|soft-concatenated|ldpc
CAPTURE_GAIN="${CAPTURE_GAIN:-16}"          # moderate mic-capture gain; max clips a line->mic cable
OUTPUT_DIR="${OUTPUT_DIR:-docs/dev/test-reports}"

# Quick tier: one case per mode family validated on the two-Pi matched-card rig
# (see memory project-loopback-mode-matrix). Single-clock-tolerant modes.
QUICK_CASES=(
    "BPSK100|64"
    "BPSK250|64"
    "QPSK125|64"
    "QPSK250|64"
    "QPSK500|128"
    "8PSK500|128"
    "SCFDMA16|64"
)
# `--full` is derived from the plugin registry at run time, NOT from a hand-maintained list.
#
# The previous FULL_CASES array froze 14 cases and silently drifted as the registry grew: MFSK16
# (hpx_hf's SL1 rung) and the differential QPSK250-D/QPSK500-D modes were never in it, so a --full
# run reported 14/14 while covering none of them. A hardcoded list standing in for an enumeration
# reads as coverage; that is the same defect the audit found in CRATES_TESTED and the plugin
# registrars (audit 2026-07-19, loopback-revalidation-plan task A).
enumerate_registry_modes() {
    "$BIN" modes 2>/dev/null \
        | grep -oE '\(([^)]+)\)$' \
        | tr -d '()' | tr ',' '\n' | sed 's/^ *//;s/ *$//' | grep -E '^[A-Z0-9]' | sort -u
}

# Payload size for a mode, by symbol rate: slow rungs stay short so a case does not run for minutes.
payload_for() {
    case "$1" in
        MFSK16*)                       echo 32 ;;   # 31.25 baud, ~17 s per frame
        BPSK31|BPSK63)                 echo 32 ;;
        *9600*|*2000*|*1000*)          echo 128 ;;
        *500*)                         echo 128 ;;
        *)                             echo 64 ;;
    esac
}

# Some modes cannot decode without FEC, so a no-FEC sweep would report a FALSE failure.
#
# Differential (`-D`) encodes each dibit as a phase increment: a fade rotation cancels symbol to
# symbol, but every slip costs a dibit that only FEC can repair. Uncoded differential measures 0.00
# BY DESIGN (see CLAUDE.md, #923). Running it with FEC=none and recording "fail" would manufacture a
# regression that is really a configuration error.
fec_for() {
    # An explicitly-set FEC= wins: the per-mode value is a DEFAULT that stops a naive sweep recording
    # a false failure, not a mandate. Hardcoding it made deliberate experiments (e.g. trying LDPC on a
    # differential rung) silently run the wrong FEC.
    if [[ -n "${FEC_EXPLICIT:-}" ]]; then echo "$FEC"; return; fi
    case "$1" in
        *-D)      echo "rs" ;;
        MFSK16*)  echo "rs" ;;   # sub-floor rung is always FEC-protected in the ladder
        *)        echo "$FEC" ;;
    esac
}

# Airtime (seconds) of the largest frame each mode can send, read from the plugin registry via
# `openpulse modes --airtime`. Queried once; modes the CLI does not report fall back to the floors.
declare -A AIRTIME
_load_airtimes() {
    local m a
    while IFS=$'\t' read -r m a; do
        [[ -n "$m" ]] && AIRTIME["$m"]="$a"
    done < <("$BIN" --log error modes --airtime 2>/dev/null)
}

airtime_for() { echo "${AIRTIME[$1]:-0}"; }

# Raise TX_TIMEOUT / IRS_LISTEN_MS to fit this mode, unless the operator set them explicitly.
size_windows_for() {
    local mode="$1" a
    a="$(airtime_for "$mode")"
    if [[ -z "${TX_TIMEOUT_EXPLICIT:-}" ]]; then
        TX_TIMEOUT="$(awk -v a="$a" -v f="$TX_TIMEOUT_FLOOR" 'BEGIN{t=a*1.5+20; print (t>f)?int(t+0.5):f}')"
    fi
    if [[ -z "${IRS_LISTEN_EXPLICIT:-}" ]]; then
        IRS_LISTEN_MS="$(awk -v a="$a" -v w="$IRS_STARTUP_WAIT" -v f="$IRS_LISTEN_MS_FLOOR" \
            'BEGIN{t=(a+w+25)*1000; print (t>f)?int(t+0.5):f}')"
    fi
}

# Modes that cannot run on this rig, with the reason. Reported as SKIP, never silently dropped.
skip_reason_for() {
    case "$1" in
        *9600*) echo "needs Fs >= 38.4 kHz (>=4 samples/symbol); this path is 8 kHz" ;;
        FSK4-ACK|MFSK16-ACK) echo "ACK-channel waveform, exercised by the ARQ tests not a data sweep" ;;
        *) echo "" ;;
    esac
}

TIER="${TIER:-quick}"
SINGLE_CASE=""
LEVEL_CHECK=0
SRO_CHECK=0
while [[ $# -gt 0 ]]; do
    case "$1" in
        --quick)       TIER="quick";     shift ;;
        --full)        TIER="full";      shift ;;
        --single-case) SINGLE_CASE="$2"; shift 2 ;;
        --sro-check)   SRO_CHECK=1;      shift ;;
        --output)      OUTPUT_DIR="$2";  shift 2 ;;
        --level-check) LEVEL_CHECK=1;    shift ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
done

if [[ ! -x "$BIN" ]]; then
    echo "ERROR: cpal CLI not found at $BIN -- run: cargo build --release -p openpulse-cli" >&2
    exit 1
fi
if ! "$BIN" --backend cpal devices 2>/dev/null | grep -q "^${TX_DEVICE}\b" \
|| ! "$BIN" --backend cpal devices 2>/dev/null | grep -q "^${RX_DEVICE}\b"; then
    echo "ERROR: ${TX_DEVICE}/${RX_DEVICE} not enumerated -- run scripts/setup-dualcard-loopback.sh" >&2
    exit 1
fi

_normalise() {  # disable AGC, restore moderate capture gain on both cards
    for c in "$TX_CARD" "$RX_CARD"; do
        [[ -z "$c" ]] && continue
        amixer -c "$c" cset name='Auto Gain Control' 0 >/dev/null 2>&1 || true
        amixer -c "$c" cset name='Mic Capture Switch' 1 >/dev/null 2>&1 || true
        amixer -c "$c" cset name='Mic Capture Volume' "${CAPTURE_GAIN}" >/dev/null 2>&1 || true
    done
}

# Read the AGC state of a card: "off", "on", or "" when the card has no such control.
#
# Parses ONLY the value line, which `amixer cget` prefixes with ':' —
#     ; type=BOOLEAN,access=rw------,values=1
#     : values=off
# The `values=1` on the *type* line is the number of values the control carries, not its state.
# Matching `values=` anywhere reads that as "on" and makes the guard fire on a correctly-configured
# rig, which is how the first version of this function behaved.
_agc_state() {
    local line
    line="$(amixer -c "$1" cget name='Auto Gain Control' 2>/dev/null | sed -n 's/^[[:space:]]*:[[:space:]]*values=//p')" || return 0
    [[ -z "$line" ]] && return 0
    if [[ "${line%%,*}" == "on" || "${line%%,*}" == "1" ]]; then echo on; else echo off; fi
}

# Refuse to run a sweep on a rig whose capture AGC is live.
#
# `_normalise` SETS the mixers but cannot tell whether it worked: it `continue`s past an unresolved
# card, and every `amixer` call ends in `|| true`, so a wrong card index or a renamed control is
# silently a no-op. That is not hypothetical. ALSA assigns card indices in enumeration order, and
# unplugging the adapters resets their mixer state — so a session that ran a sweep without re-running
# `setup-dualcard-loopback.sh` measured a rig whose capture AGC was ON, and recorded the result as a
# property of the waveform.
#
# It cost eight modes a wrong classification. A capture AGC moves the level *during* a frame, so it
# destroys exactly the amplitude-carrying modes (64QAM, the dense SC-FDMA QAMs) and leaves the
# phase-only ones alone — which reads convincingly as "these modes can't survive the analog path".
# Re-run on a normalised rig, six of those eight passed with no code change at all (2026-07-22).
# Ablated directly: SCFDMA52-16QAM and -32QAM each FAIL 2/2 with the AGC on and PASS 2/2 with it off.
#
# So: verify, and abort loudly. A sweep that silently measures the wrong rig is worse than one that
# refuses to start.
_preflight_mixers() {
    local bad=0 c label idx state hot=()

    # Scan EVERY card that has the control, not just the two this script resolved. The failure this
    # guards against is `_normalise` touching the wrong card, and a guard that trusts the same
    # resolution would read the wrong card too and pass. Card indices shift on re-probe (2026-07-19:
    # `acp` moved 3 -> 4 mid-session and hwloop_tx silently followed), which is exactly when the
    # resolution is wrong and the check most needs to be independent of it.
    for idx in $(sed -n 's/^ *\([0-9]\+\) \[.*/\1/p' /proc/asound/cards 2>/dev/null); do
        [[ "$(_agc_state "$idx")" == "on" ]] && hot+=("$idx")
    done
    if [[ ${#hot[@]} -gt 0 ]]; then
        echo "ERROR: capture AGC is ON for card(s): ${hot[*]}" >&2
        for idx in "${hot[@]}"; do
            echo "  amixer -c ${idx} cset name='Auto Gain Control' off" >&2
        done
        echo "  ...or re-run scripts/setup-dualcard-loopback.sh (mixer state resets on replug)." >&2
        bad=1
    fi

    # An unresolved card means `_normalise` silently skipped it — the state above may be luck.
    for c in "$TX_CARD:TX" "$RX_CARD:RX"; do
        label="${c##*:}"; c="${c%%:*}"
        if [[ -z "$c" ]]; then
            echo "ERROR: could not resolve the ${label} card from ~/.asoundrc, so mixer" >&2
            echo "  normalisation is silently skipped for it." >&2
            echo "  Run: scripts/setup-dualcard-loopback.sh" >&2
            bad=1
        fi
    done

    if [[ $bad -ne 0 ]]; then
        echo "Refusing to sweep: results from a rig with a live capture AGC are not attributable" >&2
        echo "to the modem. Set AGC_PREFLIGHT=0 to override (and say so in whatever you report)." >&2
        exit 1
    fi
}

if [[ "${AGC_PREFLIGHT:-1}" != "0" ]]; then
    _normalise
    _preflight_mixers
fi

# ── Level check: confirm the analog cable carries TX -> RX at a sane level ────
# Measures the actual captured RMS/peak with arecord (reliable; the modem build
# logs no energy line) while the modem transmits, so it catches both a missing
# cable (no signal) and an over-hot mic input (clipping).
# Measure the sample-rate offset between the two cards. The rig's whole claimed value over the
# single-clock virtual rung is "two independent clocks" -- this is what proves whether it delivers it.
# Measured 2026-07-20: +0.10 ppm. These USB adapters slave to the host's USB frame clock, so they do
# NOT run independently, and an SRO explanation for a failure on this rig needs this number first.
if [[ "$SRO_CHECK" -eq 1 ]]; then
    echo "==> SRO check: 1 kHz tone, TX card ${TX_CARD:-?} -> RX card ${RX_CARD:-?}"
    python3 "$(dirname "$0")/lib/sro_estimator.py" >/dev/null || {
        echo "    ERROR: estimator self-test failed; a reading from it would be meaningless" >&2; exit 1; }
    _normalise
    pkill -x openpulse 2>/dev/null; sleep 0.3
    tone="/tmp/openpulse-sro-tone.wav"; rec="/tmp/openpulse-sro-rec.wav"
    python3 -c "
import math,struct,wave
sr=48000; w=wave.open('$tone','wb'); w.setnchannels(1); w.setsampwidth(2); w.setframerate(sr)
w.writeframes(b''.join(struct.pack('<h',int(12000*math.sin(2*math.pi*1000.0*n/sr))) for n in range(sr*60))); w.close()"
    arecord -D "plughw:${RX_CARD},0" -f S16_LE -r 48000 -c 1 -d 63 "$rec" >/dev/null 2>&1 &
    _rp=$!
    sleep 1; aplay -D "$TX_DEVICE" "$tone" >/dev/null 2>&1
    wait $_rp
    python3 -c "
import wave,struct,sys; sys.path.insert(0,'$(dirname "$0")/lib')
from sro_estimator import est_ppm
w=wave.open('$rec'); sr=w.getframerate(); n=w.getnframes()
d=struct.unpack('<%dh'%n, w.readframes(n))
amp=[abs(x) for x in d]; thr=max(amp)*0.3
idx=[i for i,a in enumerate(amp) if a>thr]
if len(idx)<sr*5: print('    ERROR: no tone captured -- check the cable'); sys.exit(1)
seg=d[idx[0]+sr:idx[-1]-sr]
print('    analysed %.1f s'%(len(seg)/sr))
print('    measured SRO: %+.2f ppm'%est_ppm(seg,sr,1000.0))"
    exit $?
fi

if [[ "$LEVEL_CHECK" -eq 1 ]]; then
    echo "==> Level check: modem TX on ${TX_DEVICE} (card ${TX_CARD:-?}), capture on RX card ${RX_CARD:-?}"
    _normalise
    pkill -x openpulse 2>/dev/null; sleep 0.3
    cap="/tmp/openpulse-dualcard-levelcheck.wav"
    arecord -D "plughw:${RX_CARD},0" -f S16_LE -r 48000 -c 1 -d 5 "$cap" >/dev/null 2>&1 &
    recpid=$!
    sleep 0.6
    "$BIN" --backend cpal --log error --ptt none transmit --mode BPSK250 \
        --device "$TX_DEVICE" "LEVELCHECK0123456789" >/dev/null 2>&1
    wait $recpid
    python3 - "$cap" <<'PY'
import wave, struct, math, sys
w = wave.open(sys.argv[1]); n = w.getnframes()
s = struct.unpack('<%dh' % n, w.readframes(n))
act = [x for x in s if abs(x) > 200]
rms = math.sqrt(sum(x*x for x in act)/len(act))/32768 if act else 0.0
peak = max((abs(x) for x in s), default=0)/32768
clip = sum(1 for x in s if abs(x) >= 32767)
print(f"    captured rms={rms:.4f}  peak={peak:.4f}  clipped_samples={clip}")
if rms < 0.01:
    print("    NO SIGNAL -- cable missing or running the other way; swap TX/RX or check jacks.")
elif clip > 0 or peak > 0.95:
    print("    CLIPPING -- lower CAPTURE_GAIN (re-run setup with e.g. CAPTURE_GAIN=10).")
else:
    print("    OK -- signal present and unclipped; safe to run the matrix.")
PY
    exit 0
fi

# Declared unconditionally: it is read by the summary below regardless of tier, and a
# tier-local declaration made that read a syntax error that silently swallowed every SKIP.
SKIPPED=()

if [[ -n "$SINGLE_CASE" ]]; then
    CASES=("$SINGLE_CASE")
elif [[ "$TIER" == "full" ]]; then
    CASES=()
    if [[ -n "${MODES:-}" ]]; then
        read -r -a _reg <<< "$MODES"
    else
        mapfile -t _reg < <(enumerate_registry_modes)
    fi
    if [[ ${#_reg[@]} -eq 0 ]]; then
        echo "ERROR: could not enumerate modes from '$BIN modes'" >&2
        exit 1
    fi
    for m in "${_reg[@]}"; do
        reason="$(skip_reason_for "$m")"
        if [[ -n "$reason" ]]; then
            SKIPPED+=("${m}|${reason}")
        else
            CASES+=("${m}|$(payload_for "$m")")
        fi
    done
else
    CASES=("${QUICK_CASES[@]}")
fi

ts="$(date -u +%Y-%m-%dT%H%M%SZ)"
mkdir -p "$OUTPUT_DIR"
report="${OUTPUT_DIR}/loopback-dualcard-${TIER}-${ts}.json"
_load_airtimes
pass=0; fail=0; total="${#CASES[@]}"; results=()

echo "==> Dual-card hardware loopback (${ts})  tier=${TIER}  cases=${total}"
echo "    TX: ${TX_DEVICE} (card ${TX_CARD:-?})   RX: ${RX_DEVICE} (card ${RX_CARD:-?})   FEC=${FEC}"
if [[ -n "${IRS_LISTEN_EXPLICIT:-}" ]]; then
    echo "    listen=${IRS_LISTEN_MS}ms (explicit)  report=${report}"
else
    echo "    listen>=${IRS_LISTEN_MS_FLOOR}ms (raised per mode to fit its airtime)  report=${report}"
fi
if [[ ${#SKIPPED[@]} -gt 0 ]]; then
    echo "    skipped (${#SKIPPED[@]}):"
    for sk in "${SKIPPED[@]}"; do
        IFS='|' read -r sm sr <<< "$sk"
        printf "      %-16s %s\n" "$sm" "$sr"
    done
fi
echo ""

# One TX->RX attempt. Sets REASON (empty on success); returns 0 pass / 1 fail.
run_once() {  # mode payload-size payload-text
    local MODE="$1" PAYLOAD_SIZE="$2" payload="$3"
    local rxlog="/tmp/openpulse-dualcard-rx-${MODE}.log" txlog="/tmp/openpulse-dualcard-tx-${MODE}.log"
    local MODE_FEC; MODE_FEC="$(fec_for "$MODE")"
    _normalise
    pkill -x openpulse 2>/dev/null; sleep 0.3
    "$BIN" --backend cpal --log debug --ptt none receive --mode "$MODE" \
        --fec "$MODE_FEC" --listen-ms "$IRS_LISTEN_MS" --device "$RX_DEVICE" --no-afc >"$rxlog" 2>&1 &
    local rxpid=$!
    sleep "$IRS_STARTUP_WAIT"
    if ! kill -0 $rxpid 2>/dev/null; then REASON="RX not running"; return 1; fi
    local iss_exit=0
    timeout "$TX_TIMEOUT" "$BIN" --backend cpal --log info --ptt none transmit --mode "$MODE" \
        --fec "$MODE_FEC" --device "$TX_DEVICE" "$payload" >"$txlog" 2>&1 || iss_exit=$?
    sleep "$KILL_WAIT"
    kill $rxpid 2>/dev/null; wait $rxpid 2>/dev/null
    if [[ $iss_exit -ne 0 ]]; then
        REASON="TX exit ${iss_exit}: $(grep -iE 'error|too low|unsupported' "$txlog" | grep -v 'ALSA lib' | head -1 | sed 's/^ *//')"
        return 1
    fi
    if grep -Fq "$payload" "$rxlog"; then REASON=""; return 0; fi
    REASON="payload not decoded"
    return 1
}

for case_spec in "${CASES[@]}"; do
    IFS='|' read -r MODE PAYLOAD_SIZE <<< "$case_spec"
    payload="$(python3 -c "import secrets,string;print(''.join(secrets.choice(string.ascii_letters+string.digits) for _ in range(${PAYLOAD_SIZE})))")"
    CASE_FEC="$(fec_for "$MODE")"
    size_windows_for "$MODE"
    printf "  [%-14s payload=%4sB fec=%-16s] ... " "$MODE" "$PAYLOAD_SIZE" "$CASE_FEC"

    ok=1; REASON=""; attempts=0
    for ((r=1; r<=RETRIES; r++)); do
        attempts=$r
        if run_once "$MODE" "$PAYLOAD_SIZE" "$payload"; then ok=0; break; fi
    done

    if [[ $ok -eq 0 ]]; then
        [[ $attempts -gt 1 ]] && echo "PASS (attempt ${attempts})" || echo "PASS"
        pass=$((pass+1))
        results+=("{\"mode\":\"$MODE\",\"payload_bytes\":$PAYLOAD_SIZE,\"fec\":\"$CASE_FEC\",\"result\":\"pass\",\"attempts\":$attempts,\"fail_reason\":\"\"}")
    else
        echo "FAIL (${REASON}) after ${attempts} attempt(s)"
        fail=$((fail+1))
        results+=("{\"mode\":\"$MODE\",\"payload_bytes\":$PAYLOAD_SIZE,\"fec\":\"$CASE_FEC\",\"result\":\"fail\",\"attempts\":$attempts,\"fail_reason\":\"$REASON\"}")
        rxlog="/tmp/openpulse-dualcard-rx-${MODE}.log"
        afc="$(grep -E 'AFC settling|AFC decode|AFC:' "$rxlog" 2>/dev/null | head -3 || true)"
        [[ -n "$afc" ]] && { echo "    RX AFC:"; echo "$afc" | sed 's/^/      /'; }
        echo "    RX tail:"; tail -n 12 "$rxlog" 2>/dev/null | sed 's/^/      /'
    fi
done

pkill -x openpulse 2>/dev/null
echo ""
echo "==> Results: ${pass}/${total} passed, ${fail} failed."
results_json="$(IFS=,; echo "${results[*]}")"
printf '{"timestamp":"%s","transport":"hardware-dualcard","tx_device":"%s","rx_device":"%s","fec":"%s","total":%d,"pass":%d,"fail":%d,"cases":[%s]}\n' \
    "$ts" "$TX_DEVICE" "$RX_DEVICE" "$FEC" "$total" "$pass" "$fail" "$results_json" > "$report"
echo "    Report: ${report}"
[[ $fail -eq 0 ]]
