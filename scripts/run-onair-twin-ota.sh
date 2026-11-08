#!/usr/bin/env bash
# On-air TWIN-OTA scenario: two real openpulse-server daemons on two RF stations,
# driving receiver-led OTA adaptive rate-stepping over the air. Station A is the
# ISS (sends traffic, climbs its TX rate as B recommends); station B is the IRS.
# Watch both directions live with openpulse-twinview over SSH-forwarded ports.
#
# The real-radio counterpart of the in-process twin rig: instead of bridging two
# daemons through a channel model in one process, the daemons key real radios and
# the channel is the air. Each station runs one rigctld (CAT freq + PTT) and the
# cpal audio backend on its rig's soundcard.
#
# Prereqs: rigctld/rigctl + a cpal soundcard on BOTH stations; SSH access; a
#   filled-in profile (docs/config/onair-twin-ota.example.sh).
#
# Usage:
#   source docs/config/onair-twin-ota.example.sh
#   ./scripts/run-onair-twin-ota.sh <setup|deploy|run|supervise|status|cleanup>
#
# `deploy` is for a station B with no checkout or cargo (the remote FT-991A): it builds on A and
# copies openpulse-server and openpulse into B's ${B_REPO_DIR}/target/release. `run` refuses to key
# unless both stations' openpulse-server binaries are byte-identical (A1/A2 need one commit).
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/lib/rig-power.sh
source "$REPO_ROOT/scripts/lib/rig-power.sh"

# ── Profile (env, with safe defaults) ─────────────────────────────────────────
A_SSH="${A_SSH:-}"; B_SSH="${B_SSH:-}"
SSH_OPTS="${SSH_OPTS:--o BatchMode=yes -o ConnectTimeout=10}"
CALLSIGN_A="${CALLSIGN_A:-N0CALL}"; CALLSIGN_B="${CALLSIGN_B:-N0CALL}"
GRID_A="${GRID_A:-AA00}"; GRID_B="${GRID_B:-AA00}"
A_LABEL="${A_LABEL:-Station A}"; B_LABEL="${B_LABEL:-Station B}"
A_HAMLIB_MODEL="${A_HAMLIB_MODEL:-0}"; B_HAMLIB_MODEL="${B_HAMLIB_MODEL:-0}"
A_CAT_PORT="${A_CAT_PORT:-}"; A_CAT_BAUD="${A_CAT_BAUD:-19200}"
B_CAT_PORT="${B_CAT_PORT:-}"; B_CAT_BAUD="${B_CAT_BAUD:-19200}"
A_PTT_PORT="${A_PTT_PORT:-$A_CAT_PORT}"; A_PTT_TYPE="${A_PTT_TYPE:-RTS}"
B_PTT_PORT="${B_PTT_PORT:-$B_CAT_PORT}"; B_PTT_TYPE="${B_PTT_TYPE:-RTS}"
A_RIGCTLD_PORT="${A_RIGCTLD_PORT:-4532}"; B_RIGCTLD_PORT="${B_RIGCTLD_PORT:-4532}"
A_AUDIO_DEVICE="${A_AUDIO_DEVICE:-default}"; B_AUDIO_DEVICE="${B_AUDIO_DEVICE:-default}"
A_REPO_DIR="${A_REPO_DIR:-\$HOME/OpenPulseHF}"; B_REPO_DIR="${B_REPO_DIR:-\$HOME/OpenPulseHF}"
# 2 m by default (decision 13: HF only with the release candidate). Override for an HF run.
TEST_FREQ_HZ="${TEST_FREQ_HZ:-144640000}"; TEST_MODE_RIG="${TEST_MODE_RIG:-USB}"
# Per-station dial offset from TEST_FREQ_HZ: the FT-991A with OTHER SHIFT (065) = +1500 needs +1564
# (task 6, docs/dev/onair-tasks.md). Same meaning as in run-onair-ic9700-ft991a.sh.
A_FREQ_OFFSET_HZ="${A_FREQ_OFFSET_HZ:-0}"; B_FREQ_OFFSET_HZ="${B_FREQ_OFFSET_HZ:-0}"
A_FREQ_HZ=$(( TEST_FREQ_HZ + A_FREQ_OFFSET_HZ )); B_FREQ_HZ=$(( TEST_FREQ_HZ + B_FREQ_OFFSET_HZ ))
# Passband sent with the mode (`M <mode> <passband>`); the FT-991A's known-good filter is 2400.
RIG_PASSBAND_HZ="${RIG_PASSBAND_HZ:-3000}"
# Per station; `keep` sends no mode (set it on the rig: the FT-818's DIG/USER-U has no safe CAT name),
# and then *_TEST_MODE_EXPECT must name the reading (`rigctl m`) or the run stops before any keying.
# Falls back to run-onair-ic9700-ft991a.sh's names (TEST_MODE_RIG_B, ...), which the FT-818 profile sets:
# without that, sourcing it sent `M USB` to an FT-818 whose mode is set on the rig.
A_TEST_MODE_RIG="${A_TEST_MODE_RIG:-${TEST_MODE_RIG_A:-$TEST_MODE_RIG}}"; B_TEST_MODE_RIG="${B_TEST_MODE_RIG:-${TEST_MODE_RIG_B:-$TEST_MODE_RIG}}"
A_TEST_MODE_EXPECT="${A_TEST_MODE_EXPECT:-${TEST_MODE_EXPECT_A:-}}"; B_TEST_MODE_EXPECT="${B_TEST_MODE_EXPECT:-${TEST_MODE_EXPECT_B:-}}"
# The run refuses a frequency outside this range. A frequency on another band would also switch a
# rig's per-band settings, the FT-818's FRONT/REAR antenna among them. Widen it only for an HF run.
BAND_MIN_HZ="${BAND_MIN_HZ:-${BAND2M_MIN_HZ:-144500000}}"; BAND_MAX_HZ="${BAND_MAX_HZ:-${BAND2M_MAX_HZ:-144750000}}"
# Power is only ever LOWERED to these, never raised (scripts/lib/rig-power.sh). Set one to empty
# to leave that rig's power alone.
A_RFPOWER="${A_RFPOWER-0.10}"; B_RFPOWER="${B_RFPOWER-0.10}"
OTA_PROFILE="${OTA_PROFILE:-fast}"; START_MODE="${START_MODE:-BPSK250}"
DAEMON_TCP_PORT="${DAEMON_TCP_PORT:-9000}"
# Recorded explicitly in every station config, so the bundle's config snapshot attributes any effect
# to the DSP that ran (re-baseline item 7). Defaults match the daemon's.
NOTCH_ENABLED="${NOTCH_ENABLED:-true}"; AGC_ENABLED="${AGC_ENABLED:-false}"; CESSB_ENABLED="${CESSB_ENABLED:-true}"
SIGNOFF_IDLE_SECS="${SIGNOFF_IDLE_SECS:-10}"
PTT_LEADER_MS="${PTT_LEADER_MS:-0}"   # #1257; measure the rig before raising it
TRAFFIC_INTERVAL="${TRAFFIC_INTERVAL:-2}"; TRAFFIC_SIZE="${TRAFFIC_SIZE:-128}"
TRAFFIC_DURATION="${TRAFFIC_DURATION:-120}"
# Which station sends the traffic (the ISS); the other is the receiver that leads the rate.
# A2 needs both directions: one run with A, one with B.
TRAFFIC_FROM="${TRAFFIC_FROM:-A}"
# Seconds both daemons listen before traffic starts, so each receiver's energy gate has an idle
# floor before the first frame arrives (task 14: station B started ~3 s before A's first frame).
TRAFFIC_WARMUP="${TRAFFIC_WARMUP:-30}"
# The daemons' RUST_LOG. `debug` logs each decode attempt and its timing, which a failed on-air run
# needs (task 11: an `info` log could not say why station B never decoded).
DAEMON_LOG_LEVEL="${DAEMON_LOG_LEVEL:-info}"
DAEMON_LOG_LEVEL_B="${DAEMON_LOG_LEVEL_B:-$DAEMON_LOG_LEVEL}"
# After the traffic window, how long to wait at most for each station's end-of-exchange (sign-off) ID
# before the logs are collected. The ID waits 10 s after the station's last transmission and for a quiet
# channel, and the sending daemon keeps sending its queued frames after the traffic script stops: in
# task 18 Part D the last frames went out 31-40 s after the window, so a fixed 25 s wait ended
# mid-exchange. The runner waits until each station's log shows a sign-off ID after its last
# transmission, or this many seconds. The traffic script queues a message every few seconds whatever
# the daemon is doing, and the daemon holds up to 64 queued commands, so the sender can keep sending
# for many minutes after the window (task 19: still sending when a 180 s wait ran out). 900 s lets that
# backlog drain; the progress lines show when each station last transmitted.
SIGNOFF_WAIT_MAX="${SIGNOFF_WAIT_MAX:-900}"
OUTPUT_DIR="${OUTPUT_DIR:-docs/dev/test-reports}"

ACTION="${1:-supervise}"
CFG_DIR='${HOME}/.config/openpulse-twin-ota'   # remote XDG_CONFIG_HOME per station
# The kernel keeps 15 characters of a process name, so `pgrep -x openpulse-server` (16) never matches
# (task 11: the started daemon read as FAILED and `cleanup` left it running). Match the truncated
# name exactly; `-f` would also match the ssh shell whose command line names the binary.
DAEMON_COMM="openpulse-serve"

die() { echo "ERROR: $*" >&2; exit 1; }
ssh_a() { ssh ${SSH_OPTS} "${A_SSH}" "$@"; }
ssh_b() { ssh ${SSH_OPTS} "${B_SSH}" "$@"; }

preflight() {
    [[ -n "$A_SSH" && -n "$B_SSH" ]] || die "A_SSH/B_SSH unset — source the profile first"
    local f
    for f in "$A_FREQ_HZ" "$B_FREQ_HZ"; do
        (( f >= BAND_MIN_HZ && f <= BAND_MAX_HZ )) \
            || die "dial ${f} Hz (TEST_FREQ_HZ ${TEST_FREQ_HZ} + offset) is outside ${BAND_MIN_HZ}-${BAND_MAX_HZ}; set BAND_MIN_HZ/BAND_MAX_HZ for another band"
    done
    [[ "$TRAFFIC_FROM" == "A" || "$TRAFFIC_FROM" == "B" ]] || die "TRAFFIC_FROM must be A or B, not '${TRAFFIC_FROM}'"
    [[ "$CALLSIGN_A" != "N0CALL" && "$CALLSIGN_B" != "N0CALL" ]] || die "set real callsigns"
    command -v ssh >/dev/null || die "ssh not found"
}

# ── per-station helpers (run remotely) ────────────────────────────────────────
# $1 = ssh fn name, others via the *_VARS captured at call sites.
start_rigctld() {  # ssh_fn model cat_port cat_baud ptt_port ptt_type rigctld_port label
    local sshfn="$1" model="$2" cat="$3" baud="$4" ptt="$5" ptype="$6" port="$7" label="$8"
    echo "  [$label] starting rigctld (model=$model cat=$cat ptt=$ptype:$ptt port=$port)"
    "$sshfn" "pkill -x rigctld 2>/dev/null || true; sleep 0.5; \
        nohup rigctld -m $model -r $cat -s $baud -p $ptt -P $ptype -t $port \
        </dev/null >/tmp/twin-ota-rigctld.log 2>&1 & sleep 1; \
        pgrep -x rigctld >/dev/null && echo '  [$label] rigctld ok' || { echo '  [$label] rigctld FAILED'; exit 1; }"
}

tune() {  # ssh_fn rigctld_port rfpower mode expected_reading label dial_hz; non-zero stops the run
    local sshfn="$1" port="$2" power="$3" mode="$4" expect="$5" label="$6" dial="$7" now
    echo "  [$label] tune → ${dial} Hz (TEST_FREQ_HZ ${TEST_FREQ_HZ} + $(( dial - TEST_FREQ_HZ ))), mode ${mode}"
    "$sshfn" "rigctl -m 2 -r 127.0.0.1:$port F ${dial} 2>/dev/null || echo '  [$label] WARN set freq'"
    # Hamlib 4.6.2's rigctl can print a `rigctld: Hamlib ...` banner line before the reading (task 11),
    # so take the all-digit line, not the first one.
    now="$("$sshfn" "rigctl -m 2 -r 127.0.0.1:$port f 2>/dev/null | grep -E '^[0-9]+\$' | tail -n1" || true)"
    if [[ "$now" != "$dial" ]]; then
        echo "  [$label] ERROR: dial reads '${now:-unreadable}', expected ${dial}" >&2
        return 1
    fi
    if [[ "$mode" == "keep" ]]; then
        now="$("$sshfn" "rigctl -m 2 -r 127.0.0.1:$port m 2>/dev/null | grep -v '^rigctld:' | head -n1" || true)"
        echo "  [$label] mode: not touched; rig reads '${now:-unreadable}'"
        if [[ -z "$expect" ]]; then
            echo "  [$label] ERROR: mode is 'keep' but its *_TEST_MODE_EXPECT is unset; read the mode set on the rig and put it there" >&2
            return 1
        elif [[ "$now" != "$expect" ]]; then
            echo "  [$label] ERROR: mode reads '${now:-unreadable}', expected '${expect}' (set on the rig)" >&2
            return 1
        fi
    else
        "$sshfn" "rigctl -m 2 -r 127.0.0.1:$port M ${mode} ${RIG_PASSBAND_HZ} 2>/dev/null || echo '  [$label] WARN set mode'"
        # Read the mode back: a failed set used to warn and key anyway (review of task 11's fixes).
        now="$("$sshfn" "rigctl -m 2 -r 127.0.0.1:$port m 2>/dev/null | grep -v '^rigctld:' | head -n1" || true)"
        if [[ "$now" != "$mode" ]]; then
            echo "  [$label] ERROR: mode reads '${now:-unreadable}' after setting '${mode}'" >&2
            return 1
        fi
    fi
    rig_power_lower_only "$sshfn" "127.0.0.1:$port" "$power" "$label"
}

write_config() {  # ssh_fn callsign grid audio_device ptt_backend rigctld_port label
    local sshfn="$1" call="$2" grid="$3" dev="$4" rigport="$5" label="$6"
    echo "  [$label] writing daemon config (cpal device=$dev, OTA=$OTA_PROFILE)"
    "$sshfn" "mkdir -p $CFG_DIR/openpulse && cat > $CFG_DIR/openpulse/config.toml <<EOF
[station]
callsign = \"$call\"
grid_square = \"$grid\"
auto_id_signoff_idle_secs = $SIGNOFF_IDLE_SECS
[audio]
backend = \"cpal\"
device = \"$dev\"
[modem]
mode = \"$START_MODE\"
profile = \"$OTA_PROFILE\"
ptt_backend = \"rigctld\"
ptt_leader_ms = $PTT_LEADER_MS
ota_enabled = true
ota_profile = \"$OTA_PROFILE\"
notch_enabled = $NOTCH_ENABLED
agc_enabled = $AGC_ENABLED
cessb_enabled = $CESSB_ENABLED
[observability]
# A2 is scored from the OtaRateDecision events, which the daemon broadcasts but retains only in
# audit mode (re-baseline item 5).
audit_mode = true
archive_dir = \"$CFG_DIR/openpulse/audit\"
[radio]
cat_backend = \"rigctld\"
rigctld_addr = \"127.0.0.1:$rigport\"
[daemon]
tcp_port = $DAEMON_TCP_PORT
EOF"
}

build_remote() {  # ssh_fn repo_dir label
    local sshfn="$1" repo="$2" label="$3"
    echo "  [$label] building openpulse-server --features cpal + openpulse CLI (may take a while)…"
    # A non-interactive ssh shell may not have cargo on PATH (task 11): use ~/.cargo/bin as the Gate 6
    # runner's a_cargo() does. The CLI is built WITH audio (cpal-backend), the same build
    # run-onair-ic9700-ft991a.sh puts there, so `deploy` does not replace station B's audio CLI with a
    # lean one (task 11); as a control client (ota-status) either build works.
    "$sshfn" "cd $repo && PATH=\$HOME/.cargo/bin:\$PATH && \
        cargo build --release -p openpulse-daemon --features cpal --bin openpulse-server >/tmp/twin-ota-build.log 2>&1 && \
        cargo build --release -p openpulse-cli --features cpal-backend --bin openpulse >>/tmp/twin-ota-build.log 2>&1 \
        && echo '  [$label] build ok' || { tail -8 /tmp/twin-ota-build.log; exit 1; }"
}

# What a station needs to run: the daemon, the control CLI (ota-status) and the traffic injector.
RUN_FILES="target/release/openpulse-server target/release/openpulse scripts/twin-traffic.sh"

# Build on A, then copy RUN_FILES into B's repo-shaped directory (B needs no checkout or cargo).
deploy_a_to_b() {
    local ma mb
    ma="$(ssh_a "uname -m" || true)"; mb="$(ssh_b "uname -m" || true)"
    [[ -n "$ma" && "$ma" == "$mb" ]] || die "$A_LABEL is '${ma:-unreachable}', $B_LABEL is '${mb:-unreachable}': a binary built on A will not run on B"
    build_remote ssh_a "$A_REPO_DIR" "$A_LABEL" || die "build on $A_LABEL failed; nothing copied"
    echo "  [$B_LABEL] copying ${RUN_FILES} from $A_LABEL → $B_REPO_DIR"
    ssh_b "mkdir -p $B_REPO_DIR/target/release $B_REPO_DIR/scripts" || die "cannot create $B_REPO_DIR on $B_LABEL"
    ssh_a "tar -C $A_REPO_DIR -cf - ${RUN_FILES}" | ssh_b "tar -C $B_REPO_DIR -xf -"
    same_run_files || die "copy to $B_LABEL did not produce identical files"
}

# Both stations must run the same build: v0.16.0 and later commits do not interoperate, and A1/A2
# evidence counts only when both stations run one commit. A missing file is a mismatch.
same_run_files() {
    local ha hb n
    ha="$(ssh_a "cd $A_REPO_DIR && sha256sum ${RUN_FILES} 2>/dev/null" || true)"
    hb="$(ssh_b "cd $B_REPO_DIR && sha256sum ${RUN_FILES} 2>/dev/null" || true)"
    echo "  $A_LABEL:"; echo "${ha:-    (none)}" | sed 's/^/    /'
    echo "  $B_LABEL:"; echo "${hb:-    (none)}" | sed 's/^/    /'
    n="$(printf '%s\n' "$ha" | grep -c .)"
    [[ "$n" -eq 3 && "$ha" == "$hb" ]]
}

launch_daemon() {  # ssh_fn repo_dir label
    local sshfn="$1" repo="$2" label="$3" loglevel="$DAEMON_LOG_LEVEL"
    [[ "$label" == "$B_LABEL" ]] && loglevel="$DAEMON_LOG_LEVEL_B"
    echo "  [$label] launching openpulse-server (control 127.0.0.1:$DAEMON_TCP_PORT)"
    # An old daemon must be gone first, and "ok" is checked on THIS launch's pid: a name match after
    # 2 s could be a stale daemon that outlived the stop while the new one died (review, task 11).
    "$sshfn" "pkill -x $DAEMON_COMM 2>/dev/null; \
        for i in 1 2 3 4 5 6 7 8 9 10; do pgrep -x $DAEMON_COMM >/dev/null || break; sleep 0.5; done; \
        if pgrep -x $DAEMON_COMM >/dev/null; then echo '  [$label] daemon FAILED: an old daemon did not stop'; exit 1; fi; \
        XDG_CONFIG_HOME=$CFG_DIR RUST_LOG=$loglevel \
        nohup $repo/target/release/openpulse-server </dev/null >/tmp/twin-ota-daemon.log 2>&1 & pid=\$!; \
        sleep 2; kill -0 \$pid 2>/dev/null && echo '  [$label] daemon ok' || { echo '  [$label] daemon FAILED'; tail -5 /tmp/twin-ota-daemon.log; exit 1; }"
}

# CAT read-back of what the rig was actually set to, so a decode anomaly can be attributed
# (re-baseline item 5): frequency, mode + passband (the filter width), RIT and XIT (the trim).
rig_readback() {  # ssh_fn rigctld_port label
    local sshfn="$1" port="$2" label="$3"
    "$sshfn" "echo '# $label'; for c in f m j z; do printf '%s: ' \$c; \
        rigctl -m 2 -r 127.0.0.1:$port \$c 2>&1 | tr '\n' ' '; echo; done"
}

# Pull each station's retained audit events, daemon log and config next to the report.
collect_evidence() {  # report_path
    local report="$1" dir="${1%.log}-evidence"
    mkdir -p "$dir"
    for pair in "ssh_a:a:$A_LABEL:$A_RIGCTLD_PORT" "ssh_b:b:$B_LABEL:$B_RIGCTLD_PORT"; do
        IFS=: read -r fn tag label port <<<"$pair"
        "$fn" "cat $CFG_DIR/openpulse/audit/events.ndjson 2>/dev/null" >"$dir/events-$tag.ndjson" || true
        "$fn" "cat /tmp/twin-ota-daemon.log 2>/dev/null" >"$dir/daemon-$tag.log" || true
        "$fn" "cat $CFG_DIR/openpulse/config.toml 2>/dev/null" >"$dir/config-$tag.toml" || true
        rig_readback "$fn" "$port" "$label" >"$dir/rig-$tag.txt" 2>&1 || true
        local n; n=$(wc -l <"$dir/events-$tag.ndjson" 2>/dev/null || echo 0)
        echo "  [$label] evidence: $n audit events, rig read-back, config, daemon log" | tee -a "$report"
        [ "$n" -gt 0 ] || echo "  [$label] WARNING: no audit events retained — A2 cannot be scored from this run" | tee -a "$report"
    done
    echo "evidence: $dir" | tee -a "$report"
}

# ── actions ───────────────────────────────────────────────────────────────────
do_setup() {
    preflight
    echo "== setup =="
    build_remote ssh_a "$A_REPO_DIR" "$A_LABEL" || die "build on $A_LABEL failed"
    build_remote ssh_b "$B_REPO_DIR" "$B_LABEL" || die "build on $B_LABEL failed"
    echo "setup complete."
}

# A daemon did not start: stop both stations, and say whether that worked (review, task 11).
abort_after_launch() {
    if do_cleanup; then
        die "$1 daemon did not start; both stations stopped"
    else
        die "$1 daemon did not start; cleanup left something running (see ERROR above) — check by hand"
    fi
}

do_deploy() {
    preflight
    echo "== deploy =="
    deploy_a_to_b
    echo "deploy complete."
}

do_run() {
    preflight
    echo "== run =="
    same_run_files || die "the stations' daemon, CLI or traffic script differ or one is missing; nothing was keyed. Copy A's build to B with deploy (two separate builds need not be byte-identical)"
    start_rigctld ssh_a "$A_HAMLIB_MODEL" "$A_CAT_PORT" "$A_CAT_BAUD" "$A_PTT_PORT" "$A_PTT_TYPE" "$A_RIGCTLD_PORT" "$A_LABEL"
    start_rigctld ssh_b "$B_HAMLIB_MODEL" "$B_CAT_PORT" "$B_CAT_BAUD" "$B_PTT_PORT" "$B_PTT_TYPE" "$B_RIGCTLD_PORT" "$B_LABEL"
    tune ssh_a "$A_RIGCTLD_PORT" "$A_RFPOWER" "$A_TEST_MODE_RIG" "$A_TEST_MODE_EXPECT" "$A_LABEL" "$A_FREQ_HZ" \
        || die "station A failed its dial or mode check; nothing was keyed"
    tune ssh_b "$B_RIGCTLD_PORT" "$B_RFPOWER" "$B_TEST_MODE_RIG" "$B_TEST_MODE_EXPECT" "$B_LABEL" "$B_FREQ_HZ" \
        || die "station B failed its dial or mode check; nothing was keyed"
    write_config ssh_a "$CALLSIGN_A" "$GRID_A" "$A_AUDIO_DEVICE" "$A_RIGCTLD_PORT" "$A_LABEL"
    write_config ssh_b "$CALLSIGN_B" "$GRID_B" "$B_AUDIO_DEVICE" "$B_RIGCTLD_PORT" "$B_LABEL"
    # A dead daemon on one side would leave the other keying to nobody: stop both and end the run.
    launch_daemon ssh_a "$A_REPO_DIR" "$A_LABEL" || abort_after_launch "$A_LABEL"
    launch_daemon ssh_b "$B_REPO_DIR" "$B_LABEL" || abort_after_launch "$B_LABEL"

    cat <<EOF

Daemons up. To watch BOTH directions live, forward each control port locally and
run openpulse-twinview on your workstation:
  ssh ${SSH_OPTS} -N -L 9000:127.0.0.1:${DAEMON_TCP_PORT} ${A_SSH} &
  ssh ${SSH_OPTS} -N -L 9002:127.0.0.1:${DAEMON_TCP_PORT} ${B_SSH} &
  cargo run -p openpulse-twinview 127.0.0.1:9000 127.0.0.1:9002

EOF
    local iss_fn iss_repo iss_label to_call irs_fn irs_repo irs_label
    if [[ "$TRAFFIC_FROM" == "A" ]]; then
        iss_fn=ssh_a; iss_repo="$A_REPO_DIR"; iss_label="$A_LABEL"; to_call="$CALLSIGN_B"
        irs_fn=ssh_b; irs_repo="$B_REPO_DIR"; irs_label="$B_LABEL"
    else
        iss_fn=ssh_b; iss_repo="$B_REPO_DIR"; iss_label="$B_LABEL"; to_call="$CALLSIGN_A"
        irs_fn=ssh_a; irs_repo="$A_REPO_DIR"; irs_label="$A_LABEL"
    fi
    echo "Both daemons listening; traffic starts in ${TRAFFIC_WARMUP}s (warm-up) …"
    sleep "${TRAFFIC_WARMUP}"
    echo "Driving OTA traffic from ${iss_label} to ${irs_label} for ${TRAFFIC_DURATION}s …"

    # Drive traffic from the ISS's control port (background, time-boxed) and poll both daemons'
    # OTA status every 5 s, so the report shows the sender's level and the receiver's recommendation.
    local report="${OUTPUT_DIR}/onair-twin-ota-$(date +%Y%m%d-%H%M%S).log"
    mkdir -p "$OUTPUT_DIR"
    echo "# on-air twin-OTA  freq=${TEST_FREQ_HZ} dials A=${A_FREQ_HZ} B=${B_FREQ_HZ} profile=${OTA_PROFILE} traffic ${iss_label}→${irs_label} dur=${TRAFFIC_DURATION}s" | tee "$report"
    # `cd … || exit 1;`, not `cd … && nohup … &`: the latter backgrounds the whole list as a subshell
    # that keeps ssh's stdout open, so ssh did not return for the whole traffic window and the status
    # polling below never ran (task 11).
    "$iss_fn" "cd $iss_repo || exit 1; nohup timeout ${TRAFFIC_DURATION} \
        env ADDR=127.0.0.1:${DAEMON_TCP_PORT} TO=${to_call} INTERVAL=${TRAFFIC_INTERVAL} SIZE=${TRAFFIC_SIZE} \
        bash scripts/twin-traffic.sh </dev/null >/tmp/twin-ota-traffic.log 2>&1 & echo '  traffic started'"

    local end=$(( $(date +%s) + TRAFFIC_DURATION ))
    while [ "$(date +%s)" -lt "$end" ]; do
        local snap rsnap
        snap=$("$iss_fn" "$iss_repo/target/release/openpulse daemon --addr 127.0.0.1:${DAEMON_TCP_PORT} ota-status 2>/dev/null" || true)
        rsnap=$("$irs_fn" "$irs_repo/target/release/openpulse daemon --addr 127.0.0.1:${DAEMON_TCP_PORT} ota-status 2>/dev/null" || true)
        echo "$(date +%H:%M:%S)  ISS ${iss_label}: ${snap:-<no status>}  |  IRS ${irs_label}: ${rsnap:-<no status>}" | tee -a "$report"
        sleep 5
    done

    echo "traffic window complete; waiting up to ${SIGNOFF_WAIT_MAX}s for both stations' sign-off IDs …" | tee -a "$report"
    wait_for_signoff "$report"
    collect_evidence "$report"
    echo "report: $report"
    echo "(watch the live TX-level climb on both directions in openpulse-twinview)"
}

# Whether a station's last transmission is its sign-off ID: the ID's own TX-metadata line comes before
# its `transmitted station ID` line, so the last of the two kinds is the sign-off line exactly when
# nothing was transmitted after the ID.
signed_off() {
    "$1" "grep -E 'logged TX metadata|transmitted station ID' /tmp/twin-ota-daemon.log 2>/dev/null \
        | tail -n1 | grep -q 'kind=\"sign-off\"'"
}

# The UTC time of a station's last transmission (TX-metadata line), or `-`.
last_tx() {
    "$1" "grep 'logged TX metadata' /tmp/twin-ota-daemon.log 2>/dev/null | tail -n1 | cut -c12-19" || true
}

wait_for_signoff() {
    local report="$1" deadline=$(( $(date +%s) + SIGNOFF_WAIT_MAX )) a=no b=no
    local polls=0
    while [ "$(date +%s)" -lt "$deadline" ]; do
        signed_off ssh_a && a=yes || a=no
        signed_off ssh_b && b=yes || b=no
        [ "$a" = yes ] && [ "$b" = yes ] && break
        polls=$((polls + 1))
        if [ $((polls % 12)) -eq 0 ]; then
            echo "$(date +%H:%M:%S)  waiting for sign-off: ${A_LABEL} ${a} (last TX $(last_tx ssh_a)), ${B_LABEL} ${b} (last TX $(last_tx ssh_b))" | tee -a "$report"
        fi
        sleep 5
    done
    echo "sign-off IDs after the last transmission: ${A_LABEL} ${a}, ${B_LABEL} ${b}" | tee -a "$report"
}

do_status() {
    echo "== status =="
    for pair in "ssh_a:$A_LABEL" "ssh_b:$B_LABEL"; do
        local fn="${pair%%:*}" label="${pair##*:}"
        echo "[$label]"
        "$fn" "pgrep -x rigctld >/dev/null && echo '  rigctld: running' || echo '  rigctld: stopped'; \
               pgrep -x $DAEMON_COMM >/dev/null && echo '  daemon: running' || echo '  daemon: stopped'"
    done
}

# Stop both stations' daemon and rigctld, and report what is still running instead of assuming
# (task 11: it printed "stopped" while the daemon ran on). The daemon goes first, then PTT is
# commanded off through rigctld while it still runs (a daemon stopped mid-over could leave the rig
# keyed: the daemon has no SIGTERM handler, so its PTT guard does not run), then rigctld. Each gets up
# to 5 s to exit after TERM, then KILL, so a rig can stay keyed for up to ~5.5 s while a stuck daemon
# is waited out. Non-zero if anything survived.
do_cleanup() {
    echo "== cleanup =="
    local pair fn label port out left ptt rc=0
    for pair in "ssh_a:$A_RIGCTLD_PORT:$A_LABEL" "ssh_b:$B_RIGCTLD_PORT:$B_LABEL"; do
        IFS=: read -r fn port label <<<"$pair"
        out="$("$fn" "stop() { pkill -x \$1 2>/dev/null; for i in 1 2 3 4 5 6 7 8 9 10; do pgrep -x \$1 >/dev/null || return 0; sleep 0.5; done; pkill -KILL -x \$1 2>/dev/null; sleep 0.5; }; \
            stop $DAEMON_COMM; \
            if pgrep -x rigctld >/dev/null; then rigctl -m 2 -r 127.0.0.1:$port T 0 >/dev/null 2>&1 && echo ptt0-sent || echo ptt-off-failed; else echo no-rigctld; fi; \
            stop rigctld; \
            pgrep -x $DAEMON_COMM >/dev/null && echo daemon; pgrep -x rigctld >/dev/null && echo rigctld; true" || echo unreachable)"
        ptt="PTT 0 not sent: rigctld was not running"
        [[ "$out" == *ptt0-sent* ]] && ptt="PTT 0 sent while rigctld ran"
        left="$(printf '%s\n' "$out" | grep -vxE 'ptt0-sent|no-rigctld' | tr '\n' ' ' | sed 's/ *$//')"
        if [[ -z "$left" ]]; then
            echo "  [$label] daemon and rigctld stopped; ${ptt}"
        else
            echo "  [$label] ERROR: ${left} — check this station by hand" >&2
            rc=1
        fi
    done
    return $rc
}

case "$ACTION" in
    setup) do_setup ;;
    deploy) do_deploy ;;
    run) do_run ;;
    supervise) do_setup && do_run ;;
    status) do_status ;;
    cleanup) do_cleanup ;;
    *) die "unknown action '$ACTION' (setup|deploy|run|supervise|status|cleanup)" ;;
esac
