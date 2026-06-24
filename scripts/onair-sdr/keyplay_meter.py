#!/usr/bin/env python3
"""Like keyplay.py, but also polls the rig's PO/ALC meters during the burst.

Runs ON the station. Prints a METER line with mean/peak PO (RM2) and ALC (RM4)
on the FT-991A's 0-255 CAT meter scale — used for drive-backoff sweeps (vary the
WAV's audio level, watch ALC vs ACPR). Same safety gates and bulletproof unkey
as keyplay.py.

Usage:   python3 keyplay_meter.py <wav_path> <watts>
Env:     OPHF_PORT, OPHF_BAUD, OPHF_ADEV (see keyplay.py)
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


def ask(s, cmd, wait=0.08):
    s.reset_input_buffer()
    s.write(cmd.encode())
    time.sleep(wait)
    return s.read(s.in_waiting or 1).decode(errors="replace").strip()


def meter(s, idx):
    r = ask(s, idx + ";")
    try:
        return int(r[len(idx) : -1]) if r.startswith(idx) and r.endswith(";") else -1
    except ValueError:
        return -1


def watts(pc):
    try:
        return int(pc[2:5])
    except ValueError:
        return 999


def set_power(s, val):
    pc = ""
    for _ in range(4):
        s.write(("PC%03d;" % val).encode())
        time.sleep(0.6)
        pc = ask(s, "PC;", 0.2)
        if watts(pc) == val:
            return pc
    return pc


def force_rx():
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
        except Exception:  # noqa: BLE001
            time.sleep(0.3)
    return "?"


s = op()
time.sleep(0.2)
if ask(s, "TX;", 0.15) != "TX0;":
    print("ABORT not idle")
    sys.exit(3)
if watts(set_power(s, PW)) != PW:
    print("ABORT power")
    force_rx()
    sys.exit(3)

po = []
alc = []
proc = None
try:
    s.write(b"TX1;")
    t0 = time.monotonic()
    time.sleep(0.3)
    proc = subprocess.Popen(["aplay", "-q", "-D", ADEV, WAV])
    while proc.poll() is None and (time.monotonic() - t0) < WATCHDOG_S:
        po.append(meter(s, "RM2"))  # RM2 = power out
        alc.append(meter(s, "RM4"))  # RM4 = ALC
        time.sleep(0.25)
    proc.wait(timeout=WATCHDOG_S)
finally:
    if proc and proc.poll() is None:
        proc.kill()
    force_rx()

po = [x for x in po if x >= 0]
alc = [x for x in alc if x >= 0]
print(
    "METER PO_mean=%.1f PO_pk=%d ALC_mean=%.1f ALC_pk=%d"
    % (
        sum(po) / max(1, len(po)),
        max(po or [0]),
        sum(alc) / max(1, len(alc)),
        max(alc or [0]),
    )
)
