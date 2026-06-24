#!/usr/bin/env python3
"""Station-side gated keyed burst over Yaesu CAT (FT-991A) — runs ON the station.

Safety-gated: refuses to key unless the rig reads in-band (144-146 MHz) and idle,
sets and verifies the requested power (capped at 20 W to protect an attenuator),
keys via CAT (TX1;), plays the WAV out the rig's USB audio codec, then ALWAYS
unkeys (TX0;) in a finally + verifies RX. Direct CAT is used (no rigctld needed).

Usage:   python3 keyplay.py <wav_path> <watts>
Env:     OPHF_PORT (default /dev/ttyUSB0), OPHF_BAUD (38400),
         OPHF_ADEV (default plughw:CARD=CODEC,DEV=0)
"""
import os
import subprocess
import sys
import time

import serial

PORT = os.environ.get("OPHF_PORT", "/dev/ttyUSB0")
BAUD = int(os.environ.get("OPHF_BAUD", "38400"))
ADEV = os.environ.get("OPHF_ADEV", "plughw:CARD=CODEC,DEV=0")
WATCHDOG_S = 8.0
WAV = sys.argv[1]
PW = int(sys.argv[2])


def op():
    return serial.Serial(PORT, BAUD, 8, "N", 1, timeout=0.5)


def ask(s, cmd, wait=0.12):
    s.reset_input_buffer()
    s.write(cmd.encode())
    time.sleep(wait)
    return s.read(s.in_waiting or 1).decode(errors="replace").strip()


def watts(pc):
    try:
        return int(pc[2:5])
    except ValueError:
        return 999


def set_power(s, val):
    pc = ""
    for _ in range(4):
        s.write(("PC%03d;" % val).encode())
        time.sleep(0.6)  # PC needs >=0.5s to settle before read-back
        pc = ask(s, "PC;", 0.2)
        if watts(pc) == val:
            return pc
    return pc


def force_rx():
    """Bulletproof unkey: fresh serial each attempt, hammer TX0;, confirm RX."""
    st = "?"
    for _ in range(4):
        try:
            s = op()
            time.sleep(0.2)
            for _ in range(4):
                s.write(b"TX0;")
                time.sleep(0.12)
            st = ask(s, "TX;", 0.2)
            s.close()
            if st == "TX0;":
                return st
        except Exception as e:  # noqa: BLE001
            print("retry", repr(e))
            time.sleep(0.3)
    return st


s = op()
time.sleep(0.2)
fa = ask(s, "FA;", 0.15)
tx = ask(s, "TX;", 0.15)
hz = int(fa[2:11]) if fa.startswith("FA") and len(fa) >= 12 else -1
if not (144_000_000 <= hz <= 146_000_000):
    print("ABORT band", fa)
    sys.exit(3)
if tx != "TX0;":
    print("ABORT not idle", tx)
    sys.exit(3)
pc = set_power(s, PW)
if watts(pc) > 20 or watts(pc) != PW:
    print("ABORT power", pc)
    force_rx()
    sys.exit(3)
print("TXSTART %.4fMHz %dW %s" % (hz / 1e6, PW, WAV.split("/")[-1]))
sys.stdout.flush()

proc = None
try:
    s.write(b"TX1;")
    time.sleep(0.25)
    proc = subprocess.Popen(["aplay", "-q", "-D", ADEV, WAV])
    proc.wait(timeout=WATCHDOG_S)
finally:
    if proc and proc.poll() is None:
        proc.kill()
    print("UNKEY", force_rx())
s.close()
