#!/usr/bin/env bash
# Station readiness check for an on-air pair, run from the control host (for the 2 m pair: rpi51).
#
# WHY THIS EXISTS
#   The first 2 m session (2026-10-26, docs/dev/onair-session-2026-10-26.md) lost most of its time to
#   station state, not to the modem: no SSH key between the Pis, a SoapySDR module that was never
#   built, scipy missing for the G0 analyser, PipeWire volumes lost on reboot, a CAT port and a mode
#   reading still marked TODO in the profile, and a preflight proof that could not pass. Each was
#   found by hitting it. This script checks all of them up front, on both stations, before a session
#   counts. It reuses the existing checks (onair-preflight.sh, onair-setup-audio-routing.sh --verify,
#   onair-rx-level-check.sh) rather than restating them.
#
# IT KEYS NOTHING AND CHANGES NOTHING. CAT is read (`f`, `m`) and never set; audio is verified, not
# routed; the idle-level capture runs only with --levels, and only while nothing transmits.
#
# Usage:
#   source docs/config/onair-ic9700-ft818.example.sh
#   scripts/onair-station-check.sh             # everything except the idle-level capture
#   scripts/onair-station-check.sh --levels    # plus the idle RX level check (no TX anywhere)
#
# Profile variables it reads: A_SSH/B_SSH/SSH_OPTS, A_/B_REPO_DIR, A_/B_CAT_PORT, A_/B_CAT_BAUD,
# A_/B_HAMLIB_MODEL, A_/B_RIGCTLD_PORT, TEST_MODE_RIG_A/B, TEST_MODE_EXPECT_A/B, BAND2M_MIN/MAX_HZ,
# A_/B_CODEC_MATCH, A_/B_EXPECT_AGC (optional AGC reading), and WITNESS_SDR_SIDE (A, B or empty).
#
# Exit: 0 = every automated check passed, 1 = at least one failed, 2 = usage error.
# The MANUAL list printed at the end is never checked by software; work through it on the rigs.
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LEVELS=0
for arg in "$@"; do
    case "$arg" in
        --levels) LEVELS=1 ;;
        -h|--help) sed -n '2,27p' "$0"; exit 0 ;;
        *) echo "unknown argument: $arg" >&2; exit 2 ;;
    esac
done

if [[ -z "${A_SSH:-}" || -z "${B_SSH:-}" ]]; then
    echo "error: A_SSH and B_SSH are unset — source the station profile first" >&2
    exit 2
fi
SSH_OPTS="${SSH_OPTS:--o BatchMode=yes -o ConnectTimeout=10}"

PASS=0
FAIL=0
ok() { echo "[PASS] $1"; PASS=$((PASS + 1)); }
bad() { echo "[FAIL] $1"; FAIL=$((FAIL + 1)); }
info() { echo "[INFO] $1"; }

# shellcheck disable=SC2086
on() { local target="$1"; shift; ssh ${SSH_OPTS} "$target" "$@"; }

# ── Control host ────────────────────────────────────────────────────────────────
echo "== control host: $(hostname)"
LOCAL_SHA="$(git -C "$REPO_ROOT" rev-parse HEAD 2>/dev/null || echo none)"
if [[ -n "$(git -C "$REPO_ROOT" status --porcelain --untracked-files=no 2>/dev/null)" ]]; then
    bad "control checkout has uncommitted changes; a session runs on a committed tree"
else
    ok "control checkout clean at ${LOCAL_SHA:0:8}"
fi

# ── Per station ─────────────────────────────────────────────────────────────────
# The remote half runs as one script per station, so each check is one round trip and the output
# arrives as PASS|/FAIL|/INFO| lines. Arguments are positional and the repo dir is expanded remotely
# (the profile holds it as a literal '${HOME}/...').
read -r -d '' REMOTE <<'REMOTE_EOF'
# One function, called with stdin from /dev/null: the script itself arrives on stdin (`bash -s`), so
# bash must parse all of it before any command runs, or a command that reads stdin eats the rest.
main() {
set -u
# The repo dir arrives as the profile's literal '${HOME}/...': substitute HOME, never eval.
repo="${1/#'${HOME}'/$HOME}"; cat_port="$2"; model="$3"; baud="$4"; rigctld_port="$5"
expect_mode="$6"; mode_is_keep="$7"; codec="$8"; want_sdr="$9"; band_lo="${10}"; band_hi="${11}"
ptt_port="${12}"; skip_rho="${13}"; expect_agc="${14}"; audio="${15}"
p() { echo "PASS|$1"; }; f() { echo "FAIL|$1"; }; i() { echo "INFO|$1"; }
# A non-interactive ssh shell does not read the rustup PATH set in an interactive profile
# (rpi53, 2026-10-26: cargo in ~/.cargo/bin, "missing command: cargo").
[[ -f "$HOME/.cargo/env" ]] && . "$HOME/.cargo/env"

i "host: $(. /etc/os-release 2>/dev/null; echo "${PRETTY_NAME:-unknown OS}"), $(rigctl --version 2>/dev/null | head -1)"
if sha="$(git -C "$repo" rev-parse HEAD 2>/dev/null)"; then
    p "repo at $repo"
    echo "SHA|$sha"
    if status="$(git -C "$repo" status --porcelain --untracked-files=no 2>/dev/null)"; then
        [[ -z "$status" ]] && p "checkout clean" || f "checkout has uncommitted changes"
    else
        f "git status failed in $repo"
    fi
else
    f "no git checkout at $repo"
fi

for c in cargo rigctl rigctld arecord wpctl pactl python3 timeout; do
    command -v "$c" >/dev/null 2>&1 && p "command: $c" || f "missing command: $c"
done
python3 -c 'import numpy, scipy' 2>/dev/null && p "python3 numpy + scipy (G0 analyser)" \
    || f "python3 numpy/scipy missing (apt install python3-numpy python3-scipy)"

# Release binaries, cpal, callsign: the existing preflight. SKIP_RHO_STANDDOWN comes from the
# profile (the #1060 proof's fixture failed on 2026-10-26; work plan parking).
if [[ -x "$repo/scripts/onair-preflight.sh" ]]; then
    out="$(cd "$repo" && SKIP_RHO_STANDDOWN="$skip_rho" scripts/onair-preflight.sh --strict 2>&1)"; rc=$?
    summary="$(printf '%s\n' "$out" | grep '^Summary:' | tail -1)"
    if [[ $rc -eq 0 ]]; then p "onair-preflight --strict: ${summary}"; else
        f "onair-preflight --strict: ${summary:-no summary}"
        printf '%s\n' "$out" | grep '^\[FAIL\]' | sed 's/^\[FAIL\] /INFO|  preflight: /'
    fi
fi
[[ -x "$repo/target/release/openpulse-server" ]] && p "openpulse-server built (the daemon path)" \
    || f "openpulse-server missing: cargo build --release -p openpulse-daemon --features cpal --bin openpulse-server"

if [[ -z "$cat_port" || "$cat_port" == TODO* ]]; then
    f "CAT port not set in the profile"
    for d in /dev/serial/by-id/*; do [[ -e "$d" ]] && i "  present: $d"; done
elif [[ -e "$cat_port" ]]; then
    p "CAT port present: $cat_port"
else
    f "CAT port missing: $cat_port"
    for d in /dev/serial/by-id/*; do [[ -e "$d" ]] && i "  present: $d"; done
fi

# Read-only CAT: frequency and mode, through a rigctld listening on the profile's port when there is
# one. Opening the tty directly raises DTR/RTS, and on a rig whose PTT port is a sibling interface
# (the IC-9700's _B port, the SCU-17's second CP2105 port) RTS keys the transmitter, so the direct
# open is refused for the PTT port and for any second interface (-if01-).
# Up to three tries: Hamlib's FT-818 backend takes ~2 s to open the port and on 2026-10-26 returned
# no frequency once and an empty mode once, with a full read on the next try.
reading=""
cat_cmd=()   # the rigctl invocation without its commands
if ss -ltn 2>/dev/null | grep -q ":${rigctld_port}[[:space:]]"; then
    cat_cmd=(rigctl -m 2 -r "127.0.0.1:${rigctld_port}")
elif [[ -e "$cat_port" ]]; then
    if [[ -n "$ptt_port" && "$cat_port" == "$ptt_port" ]] || [[ "$cat_port" == *-if01-* ]]; then
        f "refusing to open ${cat_port} directly: it is the PTT port or a second interface, and opening it can key the rig"
    else
        cat_cmd=(rigctl -m "$model" -r "$cat_port" -s "$baud")
    fi
fi
if [[ ${#cat_cmd[@]} -gt 0 ]]; then
    for try in 1 2 3; do
        reading="$(timeout 15 "${cat_cmd[@]}" f m 2>/dev/null)"
        [[ "$(printf '%s\n' "$reading" | sed -n 1p)" =~ ^[0-9]+$ && -n "$(printf '%s\n' "$reading" | sed -n 2p)" ]] && break
        sleep 1
    done
    [[ $try -gt 1 ]] && i "  CAT read needed ${try} tries"
fi
freq="$(printf '%s\n' "$reading" | sed -n 1p)"; mode="$(printf '%s\n' "$reading" | sed -n 2p)"
if [[ "$freq" =~ ^[0-9]+$ ]]; then
    p "CAT reads: ${freq} Hz, mode ${mode:-?}"
    if (( freq < band_lo || freq > band_hi )); then
        i "  dial is outside ${band_lo}-${band_hi}; the runner tunes inside it before keying"
    fi
    if [[ "$mode_is_keep" != 1 ]]; then
        # The runner sets this side's mode itself before keying, so a different reading now is fine.
        i "  mode is set by the runner (${expect_mode:-?}); reads '${mode}' now"
    elif [[ -z "$expect_mode" || "$expect_mode" == TODO* ]]; then
        f "mode is 'keep' but TEST_MODE_EXPECT_* is unset; the reading is '${mode}'"
    elif [[ "$mode" == "$expect_mode" ]]; then
        p "mode matches the profile: ${mode} (set on the rig, sent by nothing)"
    else
        f "mode reads '${mode}', profile expects '${expect_mode}' (set it on the rig)"
    fi
else
    f "CAT read failed (no frequency); is the rig on and the CAT rate right?"
fi

# AGC, where the profile names one (IC-9700: SLOW, Hamlib 3 — on 2026-10-27 FAST held the burst at
# 4.9x the idle floor in the USB audio and Gate 3 failed; SLOW gave 7.9x). Read, never set.
if [[ -n "$expect_agc" && ${#cat_cmd[@]} -gt 0 ]]; then
    agc="$(timeout 15 "${cat_cmd[@]}" l AGC 2>/dev/null | tail -n1)"
    if [[ "$agc" == "$expect_agc" ]]; then p "AGC reads ${agc} as the profile expects"
    else f "AGC reads '${agc}', profile expects ${expect_agc} (set it on the rig)"; fi
fi

arecord -l 2>/dev/null | grep -qi "$codec" && p "capture card matching '${codec}'" \
    || f "no capture card matching '${codec}' (arecord -l)"
# The modem opens "pulse" as an ALSA device; without the ALSA pulse plugin it does not exist and the
# transmit dies with "device not found: pulse" (rpi53, 2026-10-27).
if [[ "$audio" == "pulse" ]]; then
    aplay -L 2>/dev/null | grep -qx pulse && p "ALSA device 'pulse' present" \
        || f "no ALSA device 'pulse' (apt install libasound2-plugins)"
fi

if [[ "$want_sdr" == 1 ]]; then
    python3 -c 'import SoapySDR' 2>/dev/null && p "python3 SoapySDR bindings" \
        || f "python3 SoapySDR missing (apt install python3-soapysdr)"
    systemctl is-active --quiet sdrplay 2>/dev/null && p "sdrplay API service active" \
        || f "sdrplay API service not active (install the SDRplay API with bash; systemctl status sdrplay)"
    if timeout 20 SoapySDRUtil --find="driver=sdrplay" 2>&1 | grep -q '^Found device'; then
        p "SoapySDRPlay3 finds the SDR"
    else
        f "SoapySDRPlay3 does not find an SDRplay device (module built? SoapySDRUtil --info lists modules)"
    fi
fi
echo "END|"
}
main "$@" </dev/null
REMOTE_EOF

check_side() {
    local side="$1" target="$2" repo="$3" cat_port="$4" model="$5" baud="$6" rport="$7"
    local expect="$8" keep="$9" codec="${10}" sdr="${11}" ptt="${12}" audio="${13}" agc="${14}"
    echo
    echo "== station ${side}: ${target}"
    if ! on "$target" true 2>/dev/null; then
        bad "cannot reach ${target} with ${SSH_OPTS} (key-based SSH from this host is required)"
        return
    fi
    ok "SSH to ${target} without a password"
    local line remote_sha="" ended=0 args
    # ssh joins its arguments into one string for the remote shell: quote each one for it.
    args="$(printf ' %q' "$repo" "$cat_port" "$model" "$baud" "$rport" "$expect" "$keep" "$codec" \
        "$sdr" "${BAND2M_MIN_HZ:-144500000}" "${BAND2M_MAX_HZ:-144750000}" "$ptt" "${SKIP_RHO_STANDDOWN:-0}" \
        "$agc" "$audio")"
    while IFS= read -r line; do
        case "$line" in
            PASS\|*) ok "${line#PASS|}" ;;
            FAIL\|*) bad "${line#FAIL|}" ;;
            INFO\|*) info "${line#INFO|}" ;;
            SHA\|*) remote_sha="${line#SHA|}" ;;
            END\|*) ended=1 ;;
        esac
    done < <(on "$target" "bash -s --${args}" <<<"$REMOTE" 2>&1)
    # A remote script that aborts part-way prints fewer checks and no END: that is a failure, not a
    # shorter list of passes.
    [[ $ended -eq 1 ]] || bad "the checks on ${target} did not run to the end; the station's results above are incomplete"
    if [[ "$audio" != "pulse" ]]; then
        bad "audio device is '${audio}', not 'pulse': the modem bypasses PipeWire, so the routed volumes and --levels do not apply to it"
    fi
    if [[ -n "$remote_sha" && "$remote_sha" == "$LOCAL_SHA" ]]; then
        ok "same commit as the control host (${LOCAL_SHA:0:8})"
    elif [[ -n "$remote_sha" ]]; then
        bad "commit ${remote_sha:0:8} differs from the control host's ${LOCAL_SHA:0:8}; build both ends from one commit"
    fi
}

expected_mode() {   # $1 = what the runner sends, $2 = the expected reading under `keep`
    if [[ "$1" == "keep" ]]; then echo "${2:-}"; else echo "${1:-}"; fi
}
is_keep() { [[ "${1:-}" == "keep" ]] && echo 1 || echo 0; }

SDR_SIDE="${WITNESS_SDR_SIDE:-}"
[[ -n "$SDR_SIDE" ]] || info "WITNESS_SDR_SIDE unset: no station's SDR is checked"
DEFAULT_REPO='${HOME}/git/OpenPulseHF'   # literal: expanded on the station
check_side A "$A_SSH" "${A_REPO_DIR:-$DEFAULT_REPO}" "${A_CAT_PORT:-}" "${A_HAMLIB_MODEL:-}" \
    "${A_CAT_BAUD:-}" "${A_RIGCTLD_PORT:-4532}" \
    "$(expected_mode "${TEST_MODE_RIG_A:-}" "${TEST_MODE_EXPECT_A:-}")" "$(is_keep "${TEST_MODE_RIG_A:-}")" \
    "${A_CODEC_MATCH:-Codec}" "$([[ "$SDR_SIDE" == A ]] && echo 1 || echo 0)" "${A_PTT_PORT:-}" \
    "${A_AUDIO_DEVICE:-pulse}" "${A_EXPECT_AGC:-}"
check_side B "$B_SSH" "${B_REPO_DIR:-$DEFAULT_REPO}" "${B_CAT_PORT:-}" "${B_HAMLIB_MODEL:-}" \
    "${B_CAT_BAUD:-}" "${B_RIGCTLD_PORT:-4532}" \
    "$(expected_mode "${TEST_MODE_RIG_B:-}" "${TEST_MODE_EXPECT_B:-}")" "$(is_keep "${TEST_MODE_RIG_B:-}")" \
    "${B_CODEC_MATCH:-Codec}" "$([[ "$SDR_SIDE" == B ]] && echo 1 || echo 0)" "${B_PTT_PORT:-}" \
    "${B_AUDIO_DEVICE:-pulse}" "${B_EXPECT_AGC:-}"

# ── Audio routing and levels (existing scripts) ─────────────────────────────────
echo
echo "== audio routing (scripts/onair-setup-audio-routing.sh --verify)"
if "$REPO_ROOT/scripts/onair-setup-audio-routing.sh" --verify; then
    ok "both stations route to their CODEC at the profile's volumes"
else
    bad "audio routing or volumes differ from the profile; PipeWire volumes do not survive a reboot — re-run scripts/onair-setup-audio-routing.sh"
fi
if [[ $LEVELS -eq 1 ]]; then
    echo
    echo "== idle RX level (scripts/onair-rx-level-check.sh) — nothing may transmit now"
    if "$REPO_ROOT/scripts/onair-rx-level-check.sh"; then
        ok "idle RX level lets the energy gate discriminate on both stations"
    else
        bad "idle RX level check failed"
    fi
else
    info "idle RX level not checked (add --levels, with no TX anywhere and both rigs ON)"
fi

cat <<'MANUAL'

== MANUAL — not readable over CAT; confirm on the rigs before the session counts
  [ ] FT-818: FRONT antenna selected at 2 m, L1 (0.5 W), DIG/USER-U, CAT RATE matches the profile
  [ ] FT-818: VFO B on the same 2 m frequency as VFO A — Hamlib toggles VFO A/B on every CAT read, so
      a VFO B on HF puts the rig on HF for a moment at every read (7.077 MHz found on 2026-10-27)
  [ ] FT-818: away from the local noise source found on 2026-10-26 (S8–S9 floor)
  [ ] Both rigs: TX time-out timer (TOT) enabled — the A5 backstop
  [ ] IC-9700: the required settings in onair-signal-chain-verification.md (DATA MOD USB, AF output)
  [ ] RSPdx: a fixed attenuator or measured isolation against the IC-9700 beside it; its maximum
      input rating checked in SDRplay's datasheet (UNCHECKED in the repo)
  [ ] Antennas connected (Diamond RH3 on the FT-818); the agreed window and frequency announced
MANUAL

echo
echo "Summary: PASS=${PASS} FAIL=${FAIL}"
[[ $FAIL -eq 0 ]]
