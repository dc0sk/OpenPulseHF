#!/usr/bin/env python3
"""Station-side gated keyed burst over Yaesu CAT (FT-991A) — runs ON the station.

Safety-gated: refuses to key unless the rig answers as an FT-991A (ID0670;), reads
in-band (144-146 MHz) and idle, and its power is at or below <watts> and 20 W (the
attenuator's limit). Power is only ever lowered, never raised (station owner's rule,
2026-10-14): a request above the rig's setting keys at the setting. It then
keys via CAT (TX1;), plays the WAV out the rig's USB audio codec, then ALWAYS
unkeys (TX0;) in a finally + verifies RX. Direct CAT is used (no rigctld needed).

Usage:   python3 keyplay.py <wav_path> <max_watts>
Env:     OPHF_PORT (default /dev/ttyUSB0), OPHF_BAUD (38400),
         OPHF_ADEV (default plughw:CARD=CODEC,DEV=0), OPHF_RIG_ID (default ID0670;)
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


def op():
    return serial.Serial(PORT, BAUD, 8, "N", 1, timeout=0.5)


def ask(s, cmd, wait=0.12):
    s.reset_input_buffer()
    s.write(cmd.encode())
    time.sleep(wait)
    return s.read(s.in_waiting or 1).decode(errors="replace").strip()


FT991A_ID = os.environ.get("OPHF_RIG_ID", "ID0670;")  # Yaesu CAT reference; not yet read off this rig
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
        return "ABORT not an FT-991A (ID reads %r); this keyer speaks only its ASCII CAT" % rid, -1
    fa = ask(s, "FA;", 0.15)
    hz = int(fa[2:11]) if fa.startswith("FA") and len(fa) >= 12 else -1
    if not (144_000_000 <= hz <= 146_000_000):
        return "ABORT band %s" % fa, hz
    tx = ask(s, "TX;", 0.15)
    if tx != "TX0;":
        return "ABORT not idle %s" % tx, hz
    w = lower_power_only(s, want)
    if w > MAX_W or w > want:
        return "ABORT power %dW (requested at most %dW; this keyer never raises power)" % (w, want), hz
    return None, hz


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



def main(wav, want):
    s = op()
    time.sleep(0.2)
    why, hz = gate(s, want)
    if why:
        print(why)
        s.close()
        force_rx()
        return 3
    w = watts(ask(s, "PC;", 0.2))
    print("TXSTART %.4fMHz %dW %s" % (hz / 1e6, w, wav.split("/")[-1]))
    sys.stdout.flush()
    proc = None
    try:
        s.write(b"TX1;")
        time.sleep(0.25)
        proc = subprocess.Popen(["aplay", "-q", "-D", ADEV, wav])
        proc.wait(timeout=WATCHDOG_S)
    finally:
        if proc and proc.poll() is None:
            proc.kill()
        print("UNKEY", force_rx())
    s.close()
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1], int(sys.argv[2])))
