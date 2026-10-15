#!/usr/bin/env python3
"""Like keyplay.py, but also polls the rig's PO/ALC meters during the burst.

Runs ON the station. Prints a METER line with mean/peak PO (RM2) and ALC (RM4)
on the FT-991A's 0-255 CAT meter scale — used for drive-backoff sweeps (vary the
WAV's audio level, watch ALC vs ACPR). Same safety gates and bulletproof unkey
as keyplay.py.

Usage:   python3 keyplay_meter.py <wav_path> <max_watts>
Env:     OPHF_PORT, OPHF_BAUD, OPHF_ADEV (see keyplay.py)
"""
import os
import signal
import subprocess
import sys
import time

import serial

PORT = os.environ.get("OPHF_PORT", "/dev/ttyUSB0")
BAUD = int(os.environ.get("OPHF_BAUD", "38400"))
ADEV = os.environ.get("OPHF_ADEV", "plughw:CARD=CODEC,DEV=0")
WATCHDOG_S = 8.0


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

FT991A_ID = os.environ.get("OPHF_RIG_ID", "ID0670;")  # Yaesu CAT reference; not yet read off this rig
MIN_W = 5  # the FT-991A's lowest PC setting
MAX_W = 20  # protects the attenuator on the SDR path


def watts(pc):
    try:
        return int(pc[2:5]) if pc.startswith("PC") and pc.endswith(";") else 999
    except ValueError:
        return 999


def lower_power_only(s, want):
    """Lower the rig's power to `want` watts, never raise it; return the watts it will key at."""
    now = watts(ask(s, "PC;", 0.2))
    if now == 999 or want >= now:
        return now
    pc = ""
    for _ in range(4):
        s.write(("PC%03d;" % want).encode())
        time.sleep(0.6)  # PC needs >=0.5s to settle before read-back
        pc = ask(s, "PC;", 0.2)
        if watts(pc) == want:
            break
    return watts(pc)


def gate(s, want):
    """Refuse to key unless this is an FT-991A, in band, idle, at or below `want` and MAX_W."""
    rid = ask(s, "ID;", 0.15)
    if rid != FT991A_ID:
        # Send nothing more: ASCII CAT into another rig (an FT-818 speaks 5-byte binary) is noise.
        return "ABORT not an FT-991A (ID reads %r); this keyer speaks only its ASCII CAT" % rid, -1, 0, False
    fa = ask(s, "FA;", 0.15)
    hz = int(fa[2:11]) if fa.startswith("FA") and len(fa) >= 12 else -1
    if not (144_000_000 <= hz <= 146_000_000):
        return "ABORT band %s" % fa, hz, 0, True
    tx = ask(s, "TX;", 0.15)
    if tx != "TX0;":
        return "ABORT not idle %s" % tx, hz, 0, True
    w = lower_power_only(s, want)
    if w > MAX_W or w > want:
        return "ABORT power %dW (requested at most %dW; this keyer never raises power)" % (w, want), hz, w, True
    return None, hz, w, True


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



def on_sigterm(signum, frame):
    # `timeout` sends SIGTERM; exiting through SystemExit runs the `finally` that unkeys.
    sys.exit(128 + signum)


def main(wav, want):
    if not MIN_W <= want <= MAX_W:
        print("ABORT max_watts %d outside %d..%d" % (want, MIN_W, MAX_W))
        return 3
    signal.signal(signal.SIGTERM, on_sigterm)
    s = op()
    time.sleep(0.2)
    why, _, w, touched = gate(s, want)
    if why:
        print(why)
        s.close()
        if touched:
            force_rx()
        return 3
    po = []
    alc = []
    proc = None
    try:
        s.write(b"TX1;")
        t0 = time.monotonic()
        time.sleep(0.3)
        proc = subprocess.Popen(["aplay", "-q", "-D", ADEV, wav])
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
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1], int(sys.argv[2])))
