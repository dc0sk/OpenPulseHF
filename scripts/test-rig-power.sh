#!/usr/bin/env bash
# Tests for scripts/lib/rig-power.sh: the on-air runners may lower a rig's power, never raise it.
# A fake station records every command; the rig's RFPOWER reading is set per case.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
# shellcheck source=scripts/lib/rig-power.sh
source scripts/lib/rig-power.sh

READING=""; SENT=()
fake_station() {
    SENT+=("$1")
    case "$1" in
        *" l RFPOWER"*) printf '%s\n' "$READING" ;;
    esac
}
fails=0
# case NAME READING WANT EXPECT_SET(0|1)
case_() {
    local name="$1" expect="$4"; READING="$2"; SENT=()
    rig_power_lower_only fake_station 127.0.0.1:4532 "$3" test >/dev/null
    local set=0 c
    for c in "${SENT[@]}"; do [[ "$c" == *" L RFPOWER "* ]] && set=1; done
    if [[ "$set" != "$expect" ]]; then
        echo "FAIL: $name (reading '$2', requested '$3'): set=$set, expected $expect"
        fails=$((fails + 1))
    fi
}

case_ "lowers when the request is below the reading"   0.50  0.10 1
case_ "never raises"                                   0.05  0.10 0
case_ "leaves an equal request alone"                  0.10  0.10 0
case_ "no request touches nothing"                     0.50  ""   0
case_ "an unreadable rig is left alone"                ""    0.10 0
case_ "an out-of-scale reading (FT-818: 10) is left"   10    0.05 0
case_ "a non-numeric reading is left alone"            "RPRT -11" 0.05 0
case_ "a non-numeric request is refused"               0.50  "max" 0

# Under the callers' `set -euo pipefail`, a station that cannot be reached leaves power alone and
# does not end the run.
dead_station() { SENT+=("$1"); return 255; }
out="$(set -euo pipefail; SENT=(); rig_power_lower_only dead_station 127.0.0.1:4532 0.10 test; echo survived)"
[[ "$out" == *"left as is"*"survived" ]] || { echo "FAIL: an unreachable station under set -e: $out"; fails=$((fails + 1)); }

# The no-request case must not even read the rig.
READING=0.5; SENT=()
rig_power_lower_only fake_station 127.0.0.1:4532 "" test >/dev/null
[[ "${#SENT[@]}" == 0 ]] || { echo "FAIL: no request still sent: ${SENT[*]}"; fails=$((fails + 1)); }

# No script may set power except through the helper: a direct send is the old defect. Every script
# under scripts/ is scanned (comment lines skipped), for Hamlib's `L`/`\set_level` and the raw CAT
# power commands (Yaesu `PC`, Icom hex). A level held in a variable (`L $lvl`) is not caught.
SENDS_POWER='(\\set_level|(^|[^A-Za-z_])L)[[:space:]"'"'"']+RFPOWER|w[[:space:]]+['"'"'"](PC[0-9]|\\0x)'
scan() { grep -nE "$SENDS_POWER" | grep -vE '^[^:]*:[0-9]+:[[:space:]]*#|^[0-9]+:[[:space:]]*#'; }
# Positive control (verification mechanic 4): the filter must find every planted form.
planted=$'rigctl L RFPOWER 0.5\nx \\set_level RFPOWER 1\nrigctl L  RFPOWER 1\nrigctl "L" RFPOWER 1\nrigctl w \'PC005;\''
found="$(printf '%s\n' "$planted" | scan | wc -l)"
[[ "$found" == 5 ]] || { echo "FAIL: the power-send filter found $found of 5 planted sends"; fails=$((fails + 1)); }
hits="$(grep -rnE "$SENDS_POWER" --include='*.sh' scripts \
    | grep -v '^scripts/lib/rig-power.sh:\|^scripts/test-rig-power.sh:' \
    | grep -vE '^[^:]+:[0-9]+:[[:space:]]*#')"
if [[ -n "$hits" ]]; then
    printf '%s\n' "$hits"
    echo "FAIL: a script sends RF power directly; use rig_power_lower_only"
    fails=$((fails + 1))
fi

if [[ "$fails" == 0 ]]; then echo "RIG-POWER: PASS"; else echo "RIG-POWER: FAIL ($fails)"; exit 1; fi
