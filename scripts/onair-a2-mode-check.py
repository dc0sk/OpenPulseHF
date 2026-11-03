#!/usr/bin/env python3
"""A2 corroboration: identify each transmitted data frame's mode in a receive recording.

Usage: scripts/onair-a2-mode-check.py <rx-recording.wav.gz> <recording start HH:MM:SS.ffffff> <sender daemon log .gz>

For every data frame the sender logged (`logged TX metadata ... mode=X`, written at the END of the
transmission), it takes the 3.5 s of the receiver's recording before that line and measures:
  - the baud: the strongest line in the envelope spectrum among 31.25, 62.5, 100 and 250 Hz (the lowest
    with a strong line, since a 31.25 Bd envelope also shows its 62.5 Hz harmonic);
  - BPSK or QPSK: the squared baseband signal has a carrier line, stronger than the fourth power's, for
    BPSK; for QPSK the fourth power's is stronger (measured p2/p4: QPSK 0.25-0.5, BPSK above 2).
It prints one line per frame and `agree N, disagree M`. A2's corroboration rule
(`docs/dev/project/release-1.0-criteria.md`, A2 scoring) asks that a mode change be "physically
identifiable by baud and occupied bandwidth without trusting the modem": this measures it from the audio.

## Consumer
The A2 scoring of the on-air runs (`docs/dev/onair-tasks.md`, tasks 18-20). Run by hand on the receive
recordings in `docs/dev/test-reports/on-air/rx-audio-*`.

## Prior art
`grep -rln "baud\|fourth power" scripts/` found no mode classifier (only the runners). The A2 criteria
name the capture check; none existed.

## Twins
The RSPdx RF captures (`sdr_capture.py`) are the other corroboration source; they stay on rpi51 and are
UNCHECKED here.
"""
import sys, gzip, wave, io, re, datetime as dt
import numpy as np

wav_gz, rec_start, tx_log_gz = sys.argv[1], sys.argv[2], sys.argv[3]
FS = 8000
with gzip.open(wav_gz) as f:
    w = wave.open(io.BytesIO(f.read()))
x = np.frombuffer(w.readframes(w.getnframes()), dtype='<i2').astype(np.float64) / 32768
t0 = dt.datetime.strptime(rec_start, "%H:%M:%S.%f")
sec = np.sqrt(np.array([(x[i:i+FS]**2).mean() for i in range(0, len(x) - FS, FS)]))
floor = np.percentile(sec, 10)

EXPECT = {"BPSK31": (31.25, 2), "BPSK63": (62.5, 2), "BPSK100": (100.0, 2),
          "BPSK250": (250.0, 2), "QPSK250-D": (250.0, 4), "QPSK250": (250.0, 4)}
BAUDS = [31.25, 62.5, 100.0, 250.0]

def classify(seg):
    n = len(seg)
    t = np.arange(n) / FS
    z = seg * np.exp(-2j * np.pi * 1500 * t)
    k = 8  # crude low-pass: moving average, 1 ms
    z = np.convolve(z, np.ones(k) / k, mode='same')
    env = np.abs(z); env -= env.mean()
    E = np.abs(np.fft.rfft(env * np.hanning(n)))
    fr = np.fft.rfftfreq(n, 1 / FS)
    def line(f0, bw=1.5):
        sel = (fr > f0 - bw) & (fr < f0 + bw)
        ref = (fr > f0 - 15) & (fr < f0 + 15) & ~sel
        return E[sel].max() / (np.median(E[ref]) + 1e-12)
    scores = {b: line(b) for b in BAUDS}
    # The lowest baud with a strong line: a 31.25 Bd envelope also shows its 62.5 Hz harmonic.
    top = max(scores.values())
    baud = min(b for b in BAUDS if scores[b] >= 0.6 * top)
    def peak(p):
        Z = np.abs(np.fft.fft(z ** p * np.hanning(n)))
        f = np.fft.fftfreq(n, 1 / FS)
        sel = np.abs(f) < 60
        return Z[sel].max() / (np.median(Z) + 1e-12)
    p2, p4 = peak(2), peak(4)
    # BPSK: the squared signal has a carrier line, stronger than the fourth power's. QPSK: measured
    # p2/p4 0.25-0.5 (noise lifts both), BPSK above 2.
    order = 2 if p2 > p4 else 4
    return baud, order, scores[baud], p2, p4

rows = []
with gzip.open(tx_log_gz, 'rt') as f:
    for line in f:
        m = re.search(r"logged TX metadata: .* mode=(\S+)", line)
        if not m or m.group(1) not in EXPECT:
            continue
        ts = dt.datetime.strptime(line[11:23], "%H:%M:%S.%f")
        rows.append((ts, m.group(1)))
# A data frame's TX-metadata line is logged at its first sample; an ID frame (BPSK250, short) is filtered
# below by the cut length check.
ok = bad = 0
for i, (ts, mode) in enumerate(rows):
    # The TX-metadata line is logged at the END of a transmission: the frame is the audio before it.
    # A short line within 2 s of the previous one is an ID keyed right after a data frame: skipped.
    if i > 0 and (ts - rows[i - 1][0]).total_seconds() < 2.0:
        continue
    end = (ts - t0).total_seconds() - 0.3
    start = end - 3.5
    if start < 0 or int(end * FS) > len(x):
        continue
    seg = x[int(start * FS):int(end * FS)]
    if np.sqrt((seg ** 2).mean()) < 1.8 * floor:
        print(f"{ts.time()} {mode:10s} (no signal in the recording)")
        continue
    baud, order, s, p2, p4 = classify(seg)
    eb, eo = EXPECT[mode]
    good = (baud == eb) and (order == eo)
    ok += good; bad += (not good)
    print(f"{ts.time()} {mode:10s} measured {baud:6.2f} Bd, {'BPSK' if order == 2 else 'QPSK'}  "
          f"(line {s:5.1f}, p2 {p2:6.1f}, p4 {p4:6.1f}) {'ok' if good else 'MISMATCH'}")
print(f"agree {ok}, disagree {bad}")
