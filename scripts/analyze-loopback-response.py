#!/usr/bin/env python3
"""Magnitude frequency-response of a loopback path from a recorded chirp.

A linear chirp has an approximately flat source spectrum over its sweep band, so
the received PSD shape is |H(f)|^2 up to a constant. Pass one or more captured
WAV files (mono, 16-bit); prints a 250 Hz-bin magnitude table (dB relative to the
300-2000 Hz median) and, if matplotlib is available, writes a comparison PNG.

Usage: analyze-loopback-response.py [--png OUT.png] LABEL=path.wav [LABEL=path.wav ...]
"""
import sys, wave
import numpy as np


def load(path):
    w = wave.open(path, "r")
    sr, n = w.getframerate(), w.getnframes()
    x = np.frombuffer(w.readframes(n), dtype="<i2").astype(np.float64) / 32768.0
    w.close()
    return sr, x


def find_chirp(x, sr):
    win = max(int(0.05 * sr), 1)
    e = np.sqrt(np.convolve(x * x, np.ones(win) / win, mode="same"))
    idx = np.where(e > 0.15 * e.max())[0]
    if len(idx) == 0:
        return 0, len(x)
    pad = int(0.1 * sr)
    return max(idx[0] + pad, 0), min(idx[-1] - pad, len(x))


def welch(x, sr, nfft=8192):
    if len(x) < nfft:
        nfft = 1 << int(np.floor(np.log2(max(len(x), 2))))
    win = np.hanning(nfft)
    acc = np.zeros(nfft // 2 + 1)
    k = 0
    for s in range(0, len(x) - nfft + 1, nfft // 2):
        acc += np.abs(np.fft.rfft(x[s:s + nfft] * win)) ** 2
        k += 1
    k = max(k, 1)
    return np.fft.rfftfreq(nfft, 1 / sr), acc / k


def report(label, path):
    sr, x = load(path)
    a, b = find_chirp(x, sr)
    f, psd = welch(x[a:b], sr)
    db = 10 * np.log10(psd + 1e-20)
    db -= np.median(db[(f >= 300) & (f <= 2000)])
    peak = np.abs(x).max()
    print(f"\n=== {label}  ({path}) ===")
    print(f"  sr={sr}  chirp={ (b-a)/sr:.2f}s  peak={peak:.3f}  ({'CLIP!' if peak>0.98 else 'ok'})")
    print("  magnitude (dB rel. 300-2000 Hz median), 250 Hz bins:")
    for lo in range(0, 4000, 250):
        m = (f >= lo) & (f < lo + 250)
        if m.any():
            print(f"    {lo:4d}-{lo+250:4d} Hz : {db[m].mean():+6.1f} dB")
    return f, db


def main():
    args = sys.argv[1:]
    png = None
    if args and args[0] == "--png":
        png = args[1]
        args = args[2:]
    if not args:
        print(__doc__)
        sys.exit(2)
    series = []
    for a in args:
        label, _, path = a.partition("=")
        if not path:
            label, path = path, label or a
        series.append((label or path, report(label or path, path)))
    if png:
        try:
            import matplotlib
            matplotlib.use("Agg")
            import matplotlib.pyplot as plt
            plt.figure(figsize=(9, 5))
            for label, (f, db) in series:
                m = f <= 4000
                plt.plot(f[m], db[m], label=label)
            plt.axvline(2500, color="r", ls="--", lw=0.8, label="SCFDMA52 top SC")
            plt.axvline(625, color="g", ls=":", lw=0.8, label="SCFDMA16 top SC")
            plt.xlabel("Hz"); plt.ylabel("dB rel. 300-2000 Hz")
            plt.ylim(-40, 10); plt.grid(alpha=0.3); plt.legend(fontsize=8)
            plt.title("Loopback path frequency response (chirp probe)")
            plt.savefig(png, dpi=110, bbox_inches="tight")
            print(f"\nplot -> {png}")
        except ImportError:
            print("\n(matplotlib not available; skipped PNG)")


if __name__ == "__main__":
    main()
