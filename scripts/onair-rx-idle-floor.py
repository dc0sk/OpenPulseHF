#!/usr/bin/env python3
"""Phase G0 gate: is a rig's receive USB-audio idle floor clean, or full of birdies?

The on-air campaign's rig-to-rig blocker is computer-borne RFI conducted into each
rig's USB-audio capture — narrow spectral lines (birdies) sitting 30-40 dB above the
noise floor, right in the modem passband (600-1400 Hz on the FT-991A; 1286/1394/1745 Hz
on the IC-705). Ferrites did not touch them; the recorded fix is galvanic USB isolation.
This checks whether that fix worked, on a plain idle capture (no TX anywhere, SDR stopped).

A birdie is a NARROW peak that stands well above the local broadband floor. The primary
criterion is therefore *prominence above the floor*, which is independent of capture gain
(a real birdie is ~40 dB up; ADC/thermal noise is broadband and flat). An absolute-dBFS
check is reported alongside but is secondary, because the operator's capture gain sets the
absolute scale and a quiet-but-birdied capture must still fail.

Usage:
    onair-rx-idle-floor.py <capture.wav>

Env:
    OPHF_BAND_LO_HZ     low edge of the modem passband to police, Hz   (default 300)
    OPHF_BAND_HI_HZ     high edge, Hz                                  (default 2600)
    OPHF_PROMINENCE_DB  a peak this far above the local floor is a birdie (default 15)
    OPHF_ABS_DBFS       also fail any in-band peak above this abs level (default -40)
    OPHF_MAX_BIRDIES    fail if more than this many birdies are found   (default 0)

Exit code: 0 = clean (PASS), 1 = birdies found (FAIL), 2 = usage/read error.
"""
import os
import sys
import wave

import numpy as np
from scipy.signal import welch


def _env_f(name, default):
    try:
        return float(os.environ.get(name, default))
    except ValueError:
        return default


BAND_LO = _env_f("OPHF_BAND_LO_HZ", 300.0)
BAND_HI = _env_f("OPHF_BAND_HI_HZ", 2600.0)
PROMINENCE_DB = _env_f("OPHF_PROMINENCE_DB", 15.0)
ABS_DBFS = _env_f("OPHF_ABS_DBFS", -40.0)
MAX_BIRDIES = int(_env_f("OPHF_MAX_BIRDIES", 0))


def read_wav_mono(path):
    """Return (samples float64 in -1..1, sample_rate). First channel if stereo."""
    with wave.open(path, "rb") as w:
        n, ch, sw, fs = (
            w.getnframes(),
            w.getnchannels(),
            w.getsampwidth(),
            w.getframerate(),
        )
        raw = w.readframes(n)
    if sw != 2:
        raise ValueError(f"expected 16-bit PCM, got sample width {sw} bytes")
    x = np.frombuffer(raw, dtype="<i2").astype(np.float64) / 32768.0
    if ch > 1:
        x = x[::ch]  # first channel
    return x, fs


def analyze(x, fs):
    """Welch PSD in dBFS; return (freqs, psd_db, floor_db) over the policed band.

    floor_db is a smoothed running median of the PSD, so a peak's height above it is
    its prominence. Welch with a modest segment keeps the frequency resolution fine
    enough to resolve a discrete birdie (a few Hz wide) yet averages the broadband
    floor.
    """
    nperseg = min(8192, len(x))
    # Blackman-Harris (sidelobes ~-92 dB) not the default Hann (~-31 dB): a strong birdie's
    # window sidelobes otherwise exceed the prominence threshold and get reported as a fan of
    # phantom birdies around the real line, sending the operator chasing lines that are not there.
    f, pxx = welch(
        x,
        fs=fs,
        window="blackmanharris",
        nperseg=nperseg,
        noverlap=nperseg // 2,
        scaling="spectrum",
    )
    # spectrum scaling => pxx is power per bin; sqrt to amplitude, dBFS ref 1.0 full-scale
    amp = np.sqrt(np.maximum(pxx, 1e-20))
    psd_db = 20.0 * np.log10(amp)

    # Broadband floor as a single robust statistic: the 25th percentile of the in-band PSD.
    # The noise floor is broadband and roughly flat, and birdies are a minority of bins, so a
    # low percentile lands on the noise regardless of how many lines are present. A local running
    # median does NOT work here — when the band is dense with strong lines its window keeps
    # catching tone skirts, inflating the "floor" and turning ordinary noise bins into phantom
    # prominent peaks (measured: floor lifted 30 dB in a birdie-saturated band).
    band = (f >= BAND_LO) & (f <= BAND_HI)
    floor_val = float(np.percentile(psd_db[band], 25.0))
    floor_db = np.full_like(psd_db, floor_val)
    return f, psd_db, floor_db, band


def occupied_band_db(f, psd_db, drop_db=20.0):
    """Contiguous band around the spectral peak that stays within `drop_db` of it, and its width.

    **This is the authoritative filter-width record, and CAT is not.** Measured on a real IC-9700
    with hamlib 4.6.2 (2026-08-17): hamlib reports a passband width of 0 regardless of the physical
    filter, and `M PKTUSB 0` does not restore a width — so the only evidence that a filter engaged
    is the audio that came through it. Idle noise is the ideal probe, because through a receive
    filter its spectrum IS the filter shape.

    Two design points, both forced by a control that failed:

    * **The reference is the peak of a median-smoothed PSD, not a global percentile.** A percentile
      over the whole spectrum lands in the STOPBAND once the filter is narrow — at 250 Hz only ~6 %
      of bins are in-band — which puts the threshold below every bin and reports the full spectrum
      as "occupied". Measured: a 250 Hz synthetic band came back as 3999 Hz.
    * **The run must be contiguous around that peak**, not the outermost bins above threshold: a
      distant birdie above the threshold would otherwise stretch the reported band across the gap.
      Smoothing first is what keeps a single-bin birdie from becoming the peak itself.

    Returns (lo_hz, hi_hz, width_hz, plateau_db); width is `nan` on an empty spectrum.
    """
    if len(f) == 0:
        return float("nan"), float("nan"), float("nan"), float("nan")
    # ~50 Hz of smoothing (bin spacing is fs/nperseg ~ 1 Hz): wide enough that a discrete birdie —
    # a few bins — cannot become the peak the band is anchored to, narrow enough to preserve the
    # shape of a 250 Hz filter (~250 bins). A 5-bin kernel was tried first and a single strong
    # out-of-band tone captured the measurement: it reported 3.9 Hz centred on the birdie.
    k = 51
    if len(psd_db) >= k:
        pad = k // 2
        padded = np.pad(psd_db, pad, mode="edge")
        smooth = np.array([np.median(padded[i:i + k]) for i in range(len(psd_db))])
    else:
        smooth = psd_db
    peak = int(np.argmax(smooth))
    plateau = float(smooth[peak])
    thresh = plateau - drop_db
    lo_i = peak
    while lo_i > 0 and smooth[lo_i - 1] >= thresh:
        lo_i -= 1
    hi_i = peak
    while hi_i < len(smooth) - 1 and smooth[hi_i + 1] >= thresh:
        hi_i += 1
    lo, hi = float(f[lo_i]), float(f[hi_i])
    return lo, hi, hi - lo, plateau


def find_birdies(f, psd_db, floor_db, band):
    """Local maxima in-band whose prominence exceeds the threshold OR whose absolute
    level exceeds ABS_DBFS. Returns a list of (freq, level_db, prominence_db)."""
    prom = psd_db - floor_db
    birdies = []
    for i in range(1, len(f) - 1):
        if not band[i]:
            continue
        # local maximum
        if psd_db[i] <= psd_db[i - 1] or psd_db[i] < psd_db[i + 1]:
            continue
        is_birdie = prom[i] >= PROMINENCE_DB or psd_db[i] >= ABS_DBFS
        if is_birdie:
            birdies.append((f[i], psd_db[i], prom[i]))
    # Merge peaks within ~10 Hz (same line, adjacent bins): keep the strongest.
    birdies.sort(key=lambda b: b[0])
    merged = []
    for b in birdies:
        if merged and abs(b[0] - merged[-1][0]) < 10.0:
            if b[2] > merged[-1][2]:
                merged[-1] = b
        else:
            merged.append(b)
    return merged


def self_test():
    """Validate the instrument against known answers. A measurement tool nobody has watched fail
    is indistinguishable from one that always returns a plausible number.

    Both of these controls caught a real defect while the function was being written: a global
    percentile reference reported a 250 Hz band as 3999 Hz, and a 5-bin smoothing kernel let a
    single out-of-band birdie capture the measurement (3.9 Hz centred on the tone).
    """
    fs = 8000
    rng = np.random.default_rng(7)

    def band_noise(width, birdie=None):
        n = fs * 8
        spec = np.fft.rfft(rng.standard_normal(n))
        freqs = np.fft.rfftfreq(n, 1.0 / fs)
        spec[~((freqs >= 1500 - width / 2) & (freqs <= 1500 + width / 2))] = 0.0
        sig = np.fft.irfft(spec, n)
        sig = sig / np.max(np.abs(sig)) * 0.2
        if birdie:
            sig = sig + 0.25 * np.sin(2 * np.pi * birdie * np.arange(n) / fs)
        sig = sig.astype(np.float32)
        return sig / ((np.max(np.abs(sig)) + 1e-12) / 0.2)

    cases = [
        ("clean 2400 Hz filter", band_noise(2400.0), 2400.0),
        ("clean 500 Hz filter", band_noise(500.0), 500.0),
        ("clean 250 Hz filter", band_noise(250.0), 250.0),
        ("500 Hz + birdie at 3200 Hz", band_noise(500.0, birdie=3200.0), 500.0),
        ("250 Hz + birdie at 1000 Hz", band_noise(250.0, birdie=1000.0), 250.0),
    ]
    ok = True
    print("self-test: occupied_band_db against known widths")
    for label, sig, want in cases:
        freqs, psd, _, _ = analyze(sig, fs)
        lo, hi, width, _ = occupied_band_db(freqs, psd)
        # Both halves matter: a width alone can be right while the band sits on a birdie.
        good = abs(width - want) / want <= 0.15 and lo <= 1500.0 <= hi
        ok &= good
        print(f"  {label:34s} want {want:6.0f} -> {width:7.1f} Hz ({lo:.0f}-{hi:.0f})"
              f"  {'OK' if good else 'FAIL'}")

    wide = (rng.standard_normal(fs * 8) * 0.2).astype(np.float32)
    freqs, psd, _, _ = analyze(wide, fs)
    _, _, width, _ = occupied_band_db(freqs, psd)
    good = width > 3000.0
    ok &= good
    print(f"  {'NEGATIVE: unfiltered noise reads wide':34s} want >3000 -> {width:7.1f} Hz"
          f"  {'OK' if good else 'FAIL'}")
    return 0 if ok else 1


def main(argv):
    if len(argv) > 1 and argv[1] == "--self-test":
        return self_test()
    if len(argv) != 2:
        print(__doc__)
        return 2
    try:
        x, fs = read_wav_mono(argv[1])
    except Exception as e:  # noqa: BLE001 - operator-facing tool
        print(f"ERROR reading {argv[1]}: {e}", file=sys.stderr)
        return 2
    if len(x) < fs // 2:
        print(f"ERROR: capture is {len(x)/fs:.2f} s; need >= 0.5 s", file=sys.stderr)
        return 2

    f, psd_db, floor_db, band = analyze(x, fs)
    birdies = find_birdies(f, psd_db, floor_db, band)

    in_band = psd_db[band]
    floor_med = float(np.median(floor_db[band]))
    rms_dbfs = 20.0 * np.log10(np.sqrt(np.mean(x**2)) + 1e-20)

    print(f"capture:        {argv[1]}")
    print(f"duration:       {len(x)/fs:.2f} s @ {fs} Hz")
    print(f"overall RMS:    {rms_dbfs:6.1f} dBFS")
    print(f"band policed:   {BAND_LO:.0f}-{BAND_HI:.0f} Hz")
    print(f"broadband floor:{floor_med:6.1f} dBFS (median in-band)")
    ob_lo, ob_hi, ob_w, _ = occupied_band_db(f, psd_db)
    print(
        f"occupied band:  {ob_w:6.0f} Hz ({ob_lo:.0f}-{ob_hi:.0f}) at -20 dB"
        "  <- the FILTER WIDTH; hamlib reports 0 on an IC-9700, so record this one"
    )
    print(
        f"criteria:       prominence >= {PROMINENCE_DB:.0f} dB  OR  level >= {ABS_DBFS:.0f} dBFS"
        f"  (max allowed: {MAX_BIRDIES})"
    )
    print("")

    if birdies:
        print(f"BIRDIES FOUND ({len(birdies)}):")
        print("    freq (Hz)   level (dBFS)   prominence (dB)")
        for fr, lv, pr in birdies:
            print(f"    {fr:9.0f}   {lv:11.1f}   {pr:14.1f}")
        print("")

    if len(birdies) > MAX_BIRDIES:
        print(
            "FAIL: the receive USB-audio floor is not clean. These lines are conducted RFI "
            "in the modem passband; a signal 30-40 dB below them cannot be decoded."
        )
        print(
            "      Fix: galvanic USB isolation (ADuM-class) on the rig's USB-audio link, or "
            "capture via a rear DATA/ACC jack with 1:1 audio-isolation transformers. Ferrites "
            "are documented insufficient. Re-run this gate until it PASSES before any modem run."
        )
        return 1

    print("PASS: idle floor is clean in the modem passband; the receive path can hear.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
