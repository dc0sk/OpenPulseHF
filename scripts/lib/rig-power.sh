# Rig RF power for the on-air runners: lower it or leave it, never raise it.
# Sourced, not executed: `source scripts/lib/rig-power.sh`.
#
# The station owner's rule (2026-10-14): tooling never increases a rig's power. The runners used
# to send their profile's RFPOWER unconditionally, and one restored the pre-run reading at cleanup,
# so either could raise power above what the operator had set on the rig.
#
# `rig_power_lower_only RUN RIG WANT LABEL`
#   RUN   a function that runs one shell command on the station (the runners' ssh_a / ssh_b)
#   RIG   rigctld address, host:port
#   WANT  requested power, Hamlib's 0–1 scale; empty means "do not touch power"
#   LABEL station label for the log line
# Sends `L RFPOWER WANT` only when the current reading is a number in 0–1 and WANT is strictly
# below it. Anything else (no request, an unreadable or out-of-scale reading, a request at or
# above the reading) leaves power alone and says why. The FT-818's Hamlib driver reads 10 and
# cannot set power, so it is never touched.
rig_power_lower_only() {
    local run="$1" rig="$2" want="$3" label="$4" now
    if [[ -z "$want" ]]; then
        echo "  [$label] power: not touched (no request; set it on the rig)"
        return 0
    fi
    # `|| now=""`: under a caller's `set -e`, a failed read must leave power alone, not end the run.
    now="$("$run" "rigctl -m 2 -r $rig l RFPOWER 2>/dev/null | tail -n1" 2>/dev/null | tr -d '[:space:]')" || now=""
    if rig_power_is_lower "$want" "$now"; then
        "$run" "rigctl -m 2 -r $rig L RFPOWER $want >/dev/null 2>&1" \
            && echo "  [$label] power: lowered ${now} -> ${want}" \
            || echo "  [$label] WARN power: lowering ${now} -> ${want} failed; left as is"
    else
        echo "  [$label] power: left as is (reading '${now:-unreadable}', requested ${want}; this script only lowers power)"
    fi
}

# True when both are plain numbers in Hamlib's 0–1 scale and WANT is strictly below NOW.
rig_power_is_lower() {
    local want="$1" now="$2"
    [[ "$want" =~ ^[0-9]*\.?[0-9]+$ && "$now" =~ ^[0-9]*\.?[0-9]+$ ]] || return 1
    awk -v w="$want" -v n="$now" 'BEGIN { exit !(n <= 1 && w < n) }'
}
