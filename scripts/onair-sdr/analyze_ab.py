#!/usr/bin/env python3
"""ACPR / occupied-bandwidth analysis of CE-SSB OFF vs ON IQ captures.

Computes a Welch PSD of each capture, finds the signal centre near the expected
baseband offset, and reports adjacent/alternate-channel power ratios (ACPR),
99% occupied bandwidth, and the measurement noise floor — averaged over an OFF
group and an ON group. Each capture's PSD is normalised before averaging so the
off-air coupling level (which drifts between runs) cancels.

Usage:
    analyze_ab.py <off1.cf32[,off2,...]> <on1.cf32[,on2,...]>

Env (defaults suit OFDM52 at fc=144.600 captured with LO=144.650 MHz, fs=1 MSps):
    OPHF_FS            sample rate of the captures, Hz   (default 1e6)
    OPHF_SIG_OFFSET_HZ signal centre vs SDR LO, Hz       (default -50000)
    OPHF_CHAN_BW_HZ    channel (occupied) bandwidth, Hz  (default 2031)
"""
import os
import sys

import numpy as np
from scipy.signal import welch

FS = float(os.environ.get("OPHF_FS", 1e6))
EXP_OFF = float(os.environ.get("OPHF_SIG_OFFSET_HZ", -50e3))
CH = float(os.environ.get("OPHF_CHAN_BW_HZ", 2031.0))
NPER = 65536


def tx_portion(x):
    """Trim to the transmitting span (envelope above 30% of peak)."""
    env = np.abs(x)
    idx = np.where(env > 0.3 * env.max())[0]
    return x[idx[0] : idx[-1]] if len(idx) > NPER * 2 else x


def file_psd(fn):
    fr, p = welch(
        tx_portion(np.fromfile(fn, np.complex64)),
        fs=FS,
        nperseg=NPER,
        return_onesided=False,
        detrend=False,
    )
    return np.fft.fftshift(fr), np.fft.fftshift(p)


def avg_psd(files):
    acc = None
    fr = None
    for f in files:
        fr, p = file_psd(f)
        p = p / p.sum()  # normalise → coupling-level drift cancels
        acc = p if acc is None else acc + p
    return fr, acc / len(files)


def metrics(fr, p):
    w = (fr > EXP_OFF - 2000) & (fr < EXP_OFF + 2000)
    c = np.sum(fr[w] * p[w]) / np.sum(p[w])  # signal centroid

    def bp(lo, hi):
        return p[(fr >= lo) & (fr < hi)].sum()

    main = bp(c - CH / 2, c + CH / 2)
    a_lo = 10 * np.log10(bp(c - 1.5 * CH, c - CH / 2) / main)
    a_hi = 10 * np.log10(bp(c + CH / 2, c + 1.5 * CH) / main)
    alt = 10 * np.log10((bp(c - 2.5 * CH, c - 1.5 * CH) + bp(c + 1.5 * CH, c + 2.5 * CH)) / 2 / main)
    wb = (fr > c - 5 * CH) & (fr < c + 5 * CH)
    fb = fr[wb]
    cum = np.cumsum(p[wb]) / p[wb].sum()
    obw = fb[np.searchsorted(cum, 0.995)] - fb[np.searchsorted(cum, 0.005)]
    nf = (fr > c + 8 * CH) & (fr < c + 20 * CH)
    return dict(aL=a_lo, aU=a_hi, alt=alt, obw=obw, noise=10 * np.log10(np.mean(p[nf]) * CH / main))


offs = sys.argv[1].split(",")
ons = sys.argv[2].split(",")
print("per-file ACPR (lower/upper dBc):")
for grp, files in [("OFF", offs), ("ON", ons)]:
    for f in files:
        fr, p = file_psd(f)
        p = p / p.sum()
        mm = metrics(fr, p)
        print(f"  {grp} {f.split('/')[-1]:16} aL={mm['aL']:6.2f} aU={mm['aU']:6.2f} obw={mm['obw']:.0f}")

fr, p_off = avg_psd(offs)
_, p_on = avg_psd(ons)
mo = metrics(fr, p_off)
mn = metrics(fr, p_on)
print(f"\n=== averaged ({len(offs)} OFF, {len(ons)} ON), noise floor ~{mo['noise']:.1f} dBc ===")
print(f"  {'metric':18}  OFF      ON     d(ON-OFF)")
for k, lbl, unit in [
    ("aL", "ACPR lower", "dBc"),
    ("aU", "ACPR upper", "dBc"),
    ("alt", "alt chan", "dBc"),
    ("obw", "99% OBW", "Hz"),
]:
    print(f"  {lbl:18} {mo[k]:6.2f}  {mn[k]:6.2f}  {mn[k] - mo[k]:+6.2f} {unit}")
