#!/usr/bin/env python3
"""Digital-domain out-of-band check on CE-SSB OFF vs ON audio WAVs.

Attributes spectral regrowth to the *conditioner* (DSP) rather than the PA: it
measures the conditioner's own out-of-band emission directly from the generated
8 kHz audio WAVs, with no radio in the loop. If the digital OOB is ~unchanged
between OFF and ON but on-air ACPR worsens (see analyze_ab.py), the regrowth is
PA compression, not the conditioner.

The in-band window defaults to OFDM52's occupied audio band (subcarriers 16..80
= ~484..2516 Hz); override with OPHF_INBAND_LO_HZ / OPHF_INBAND_HI_HZ.

Usage:
    digital_oob.py <off.wav> <on.wav> [label]
"""
import os
import sys
import wave

import numpy as np
from scipy.signal import welch

LO = float(os.environ.get("OPHF_INBAND_LO_HZ", 484.0))
HI = float(os.environ.get("OPHF_INBAND_HI_HZ", 2516.0))


def oob_db(fn):
    w = wave.open(fn, "rb")
    fs = w.getframerate()
    x = np.frombuffer(w.readframes(w.getnframes()), np.int16).astype(np.float32) / 32768.0
    fr, p = welch(x, fs=fs, nperseg=8192, scaling="density")
    p_in = p[(fr >= LO) & (fr < HI)].sum()
    p_tot = p.sum()
    return 10 * np.log10((p_tot - p_in) / p_in)


off = oob_db(sys.argv[1])
on = oob_db(sys.argv[2])
label = sys.argv[3] if len(sys.argv) > 3 else ""
print(f"  {label:12} OFF {off:6.2f} dB  ON {on:6.2f} dB  -> CE-SSB regrowth {on - off:+.2f} dB")
