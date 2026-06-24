#!/usr/bin/env python3
"""Capture IQ from an SDRplay RSP2 (or compatible SoapySDR device) to a CF32 file.

Part of the on-air SDR spectral-measurement toolset (see README.md). Runs on the
host that has the SDR; used by cap_during_tx.sh to capture while the station keys.

Usage:
    sdr_capture.py <center_hz> <fs_hz> <dur_s> <rfgr> <ifgr> <out.cf32>

Gain is set per element, NOT via the overall knob: RFGR = RF/LNA gain *reduction*
(0..8, higher = more attenuation = protects the front end), IFGR = IF gain
reduction (20..59, 20 = most IF gain). This avoids the inverted/confusing overall
gain mapping in SoapySDRPlay3. AGC is forced off. The device is opened with no
args because the SWIG dict/string filter is broken on Python 3.14 (only one device
is assumed present). Antenna defaults to "Antenna A" (override with OPHF_SDR_ANT).
Tune the LO ~50 kHz off the signal so it sits clear of the DC spike / IQ image.
"""
import os
import sys

import numpy as np
import SoapySDR
from SoapySDR import SOAPY_SDR_CF32, SOAPY_SDR_RX

center = float(sys.argv[1])
fs = float(sys.argv[2])
dur = float(sys.argv[3])
rfgr = float(sys.argv[4])
ifgr = float(sys.argv[5])
out = sys.argv[6]
antenna = os.environ.get("OPHF_SDR_ANT", "Antenna A")

sdr = SoapySDR.Device()  # no-args: py3.14 SWIG dict/string filter is broken
sdr.setSampleRate(SOAPY_SDR_RX, 0, fs)
sdr.setAntenna(SOAPY_SDR_RX, 0, antenna)
sdr.setGainMode(SOAPY_SDR_RX, 0, False)  # AGC off
sdr.setGain(SOAPY_SDR_RX, 0, "RFGR", rfgr)
sdr.setGain(SOAPY_SDR_RX, 0, "IFGR", ifgr)
sdr.setFrequency(SOAPY_SDR_RX, 0, center)
print(
    f"cfg: f={sdr.getFrequency(SOAPY_SDR_RX, 0) / 1e6:.4f}MHz "
    f"RFGR={sdr.getGain(SOAPY_SDR_RX, 0, 'RFGR'):.0f} "
    f"IFGR={sdr.getGain(SOAPY_SDR_RX, 0, 'IFGR'):.0f} ant={antenna}"
)

st = sdr.setupStream(SOAPY_SDR_RX, SOAPY_SDR_CF32)
sdr.activateStream(st)
n = int(fs * dur)
buf = np.empty(n, np.complex64)
chunk = np.empty(65536, np.complex64)
got = 0
for _ in range(4):  # discard initial settling
    sdr.readStream(st, [chunk], len(chunk))
while got < n:
    r = sdr.readStream(st, [chunk], min(len(chunk), n - got))
    if r.ret > 0:
        buf[got : got + r.ret] = chunk[: r.ret]
        got += r.ret
sdr.deactivateStream(st)
sdr.closeStream(st)

buf = buf[:got]
mag = np.abs(buf)
peak = mag.max()
rms = np.sqrt(np.mean(mag**2))
clip = np.mean((np.abs(buf.real) > 0.98) | (np.abs(buf.imag) > 0.98)) * 100
verdict = "OVERLOAD" if clip > 0.1 or peak > 0.99 else "OK"
print(f"captured {got / fs:.2f}s peak={peak:.4f} rms={rms:.4f} clip={clip:.2f}% {verdict}")
buf.tofile(out)
