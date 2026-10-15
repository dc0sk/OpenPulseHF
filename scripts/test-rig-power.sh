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
# Python: a Yaesu `PC` power write ("PC%03d" or a literal PC<digits>) only in keyers that define
# lower_power_only, once each.
for py in $(grep -rlE '"PC(%|[0-9])' --include='*.py' scripts); do
    n="$(grep -cE '"PC(%|[0-9])' "$py")"
    grep -q '^def lower_power_only' "$py" && [[ "$n" == 1 ]] \
        || hits+=$'\n'"$py: $n power write(s) outside lower_power_only"
done
if [[ -n "$hits" ]]; then
    printf '%s\n' "$hits"
    echo "FAIL: a script sends RF power directly; use rig_power_lower_only"
    fails=$((fails + 1))
fi

# The station-side keyers (scripts/onair-sdr/keyplay*.py) speak FT-991A ASCII CAT directly. Driven
# against a fake rig: they key only an FT-991A, in band and idle, and only ever lower its power.
python3 -I - "$PWD/scripts/onair-sdr" <<'EOF' || fails=$((fails + 1))
import contextlib, importlib.util, io, sys, types

class Rig:
    def __init__(self, rid="ID0670;", hz=144640000, pc=10, obey_pc=True, pc_reply=None):
        self.rid, self.hz, self.pc, self.obey_pc, self.pc_reply = rid, hz, pc, obey_pc, pc_reply
        self.tx, self.sent, self.out = 0, [], b""

class Port:
    def __init__(self, rig): self.rig = rig
    in_waiting = property(lambda self: len(self.rig.out))
    def reset_input_buffer(self): self.rig.out = b""
    def read(self, n): r, self.rig.out = self.rig.out, b""; return r
    def close(self): pass
    def write(self, b):
        r = self.rig
        for cmd in b.decode().split(";")[:-1]:
            r.sent.append(cmd)
            if cmd == "ID": r.out += r.rid.encode()
            elif cmd == "FA": r.out += b"FA%09d;" % r.hz
            elif cmd == "TX": r.out += b"TX%d;" % r.tx
            elif cmd == "TX1": r.tx = 1
            elif cmd == "TX0": r.tx = 0
            elif cmd == "PC": r.out += (r.pc_reply or "PC%03d;" % r.pc).encode()
            elif cmd.startswith("PC") and r.obey_pc: r.pc = int(cmd[2:])

class Proc:
    def wait(self, timeout=None): return 0
    def poll(self): return 0
    def kill(self): pass

fails = 0
for name in ("keyplay", "keyplay_meter"):
    for case, rig, want, keys, end_pc in [
        ("lowers 10 -> 5 and keys",            Rig(pc=10), 5, True, 5),
        ("never raises 5 -> 20; keys at 5",    Rig(pc=5), 20, True, 5),
        ("equal request: no write, keys",      Rig(pc=8), 8, True, 8),
        ("not an FT-991A (FT-818: no reply)",  Rig(rid=""), 5, False, None),
        ("out of band",                        Rig(hz=145650000 + 1000000), 5, False, None),
        ("rig ignores the lowering",           Rig(pc=30, obey_pc=False), 10, False, None),
        ("unreadable power",                   Rig(pc_reply="?;"), 5, False, None),
        ("above the 20 W cap",                 Rig(pc=25), 25, False, None),
    ]:
        serial = types.ModuleType("serial"); serial.Serial = lambda *a, **k: Port(rig)
        sys.modules["serial"] = serial
        spec = importlib.util.spec_from_file_location(name, f"{sys.argv[1]}/{name}.py")
        m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
        m.time.sleep = lambda s: None
        m.subprocess.Popen = lambda *a, **k: Proc()
        start_pc = rig.pc
        with contextlib.redirect_stdout(io.StringIO()):
            rc = m.main("/tmp/x.wav", want)
        keyed = "TX1" in rig.sent
        raised = any(c.startswith("PC") and len(c) > 2 and int(c[2:]) > start_pc for c in rig.sent)
        ok = keyed == keys and (rc == 0) == keys and not raised and rig.tx == 0
        if end_pc is not None: ok = ok and rig.pc == end_pc
        ok = ok and rig.sent[:1] == ["ID"]  # the rig check goes first, before any other byte
        if not ok:
            print(f"FAIL: {name}: {case}: rc={rc} keyed={keyed} raised={raised} pc={rig.pc} sent={rig.sent}")
            fails += 1
sys.exit(1 if fails else 0)
EOF

if [[ "$fails" == 0 ]]; then echo "RIG-POWER: PASS"; else echo "RIG-POWER: FAIL ($fails)"; exit 1; fi
