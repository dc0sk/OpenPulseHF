---
project: openpulsehf
doc: scripts/onair-sdr/README.md
status: living
last_updated: 2026-06-24
---

# On-air SDR spectral-measurement toolset

Apparatus for measuring a transmitter's **occupied bandwidth and ACPR on real
RF** with an SDRplay RSP2 (or compatible SoapySDR receiver) — used to validate
the [CE-SSB](../../docs/features.md#ce-ssb-transmit-envelope-conditioning) TX
conditioner on-air (the average-power gain *and* the spectral mask). The
conditioner's average-power benefit is a PA-domain effect that no audio loopback
can show, so an off-air SDR capture is the only way to confirm it without a lab
spectrum analyser.

There are **two hosts**:

- **SDR host** — the machine with the RSP2. Runs `sdr_capture.py`,
  `cap_during_tx.sh`, `analyze_ab.py`, `digital_oob.py`.
- **Station** — the machine with the radio (e.g. a Pi with an FT-991A). Runs
  `keyplay.py` / `keyplay_meter.py` (copy them there). Direct Yaesu CAT is used,
  so **no rigctld is required** on the station.

> ⚠️ This keys a real transmitter. Transmit into a dummy load / attenuator, stay
> in-band and low-power, and observe your licence conditions.

## RF path and the front-end-overload trap

The single biggest measurement error is **SDR front-end overload**: if the input
is too hot, the *SDR's own* intermodulation looks exactly like transmitter
splatter and the result is worthless. Fingerprints of overload: the capture peak
is **clamped to the same value regardless of TX power** (e.g. 5 W and 20 W give
the same peak), the 99% OBW balloons far past the real signal width, and ACPR
reads implausibly bad (e.g. −8 dBc).

Two defences:

1. **Attenuate before the SDR.** Off-air with a short whip/bare connector at a
   sensible distance, or a properly-padded coupled tap (a 20 W source needs
   ~60–70 dB of total attenuation to reach the RSP2's ~−30 dBm linear range —
   **never** connect the SDR directly to the line). Do NOT feed +20 dBm into an
   RSP2; it damages above ~+10 dBm.
2. **Verify linearity.** After picking a gain, re-capture at a ~15–30 dB lower
   level (more RFGR/IFGR) and confirm the ACPR (in dBc) is **unchanged**.
   Shoulders that hold across a big level change are real TX; shoulders that
   improve when you attenuate were SDR IMD.

## SDR host setup (Arch/Manjaro)

The RSP2 has no open driver — it needs SDRplay's proprietary API:

```bash
sudo pacman -S --needed soapysdr python-numpy python-scipy
# AUR (proprietary API + Soapy module):
git clone https://aur.archlinux.org/sdrplay.git       && (cd sdrplay       && makepkg -si)
git clone https://aur.archlinux.org/soapysdrplay3.git && (cd soapysdrplay3 && makepkg -si)
sudo systemctl enable --now sdrplay      # the apiService must be running
SoapySDRUtil --find                      # should list the RSP2
```

Gotchas baked into the scripts:

- **`make()` fails with "no sdrplay device matches" but `--find` sees it** → a
  stale single-client lock in the apiService. Fix: `sudo systemctl restart
  sdrplay` (in a real terminal — sudo can't prompt over a pipe), or replug USB.
- **Python 3.14 SWIG**: `SoapySDR.Device(dict(...))`/string filters fail;
  `SoapySDR.Device()` with no args works (one device assumed).
- **Inverted gain model**: control the gain *elements* `RFGR` (RF/LNA gain
  reduction, 0..8 — higher protects the front end) and `IFGR` (IF reduction,
  20..59 — 20 = most IF gain), not the confusing overall knob.
- Tune the LO **~50 kHz off** the signal so it's clear of the DC spike / IQ image.

## Generating the test WAVs

The OFF/ON WAVs come from the real engine TX path (so CE-SSB is applied exactly
as on air). Drop a throwaway example into the workspace and run it:

```rust
// crates/openpulse-modem/examples/onair_wavgen.rs
// run: cargo run -p openpulse-modem --example onair_wavgen -- <out.wav> <on|off> [mode]
use std::io::Write;
use ofdm_plugin::OfdmPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_modem::ModemEngine;
fn main() {
    let mut a = std::env::args().skip(1);
    let out = a.next().unwrap();
    let cessb = a.next().map(|s| s != "off").unwrap_or(true);
    let mode = a.next().unwrap_or_else(|| "OFDM52".into());
    let be = LoopbackBackend::new_split();      // split: drain TX, do NOT receive()
    let tap = be.clone_shared();
    let mut e = ModemEngine::new(Box::new(be));
    e.register_plugin(Box::new(OfdmPlugin::new())).unwrap();
    e.set_cessb_enabled(cessb);
    let payload: Vec<u8> = (0..255u16).map(|i| (i.wrapping_mul(37).wrapping_add(11)) as u8).collect();
    let mut s = Vec::new();
    while s.len() < 8000 * 3 { e.transmit(&payload, &mode, None).unwrap(); s.extend(tap.drain_samples()); }
    let peak = s.iter().fold(0f32, |m, &x| m.max(x.abs())).max(1e-9);
    let (n, sc) = (s.len() as u32, 0.9 / peak);
    let mut f = std::io::BufWriter::new(std::fs::File::create(&out).unwrap());
    for b in [b"RIFF".as_ref(), &(36 + n * 2).to_le_bytes(), b"WAVEfmt ", &16u32.to_le_bytes(),
              &1u16.to_le_bytes(), &1u16.to_le_bytes(), &8000u32.to_le_bytes(), &16000u32.to_le_bytes(),
              &2u16.to_le_bytes(), &16u16.to_le_bytes(), b"data", &(n * 2).to_le_bytes()] { f.write_all(b).unwrap(); }
    for &x in &s { f.write_all(&((x * sc * 32767.0).clamp(-32768.0, 32767.0) as i16).to_le_bytes()).unwrap(); }
}
```

`new_split()` + drain (no `receive()`) is essential — in a self-loop backend
`receive()` consumes the TX samples and you silently get a silent WAV. Then
upsample to the codec's native rate on the station: `ffmpeg -i in.wav -ar 48000
out_48k.wav`, and verify it's non-silent: `ffmpeg -i out_48k.wav -af volumedetect
-f null /dev/null 2>&1 | grep mean_volume`.

## Procedure

```bash
# 0. Copy the station scripts over and stage WAVs (48 kHz) in /tmp on the station.
scp keyplay.py keyplay_meter.py <station>:/tmp/

# 1. Baseline (no TX): confirm a clean, flat noise floor and no spurs.
python sdr_capture.py 144.650e6 1e6 2.0 8 40 base.cf32

# 2. Gain-stage + linearity: capture a 20 W burst, then a much lower-gain one;
#    ACPR (dBc) must match (see analyze_ab.py). LO 50 kHz above a 144.600 signal.
STATION=<station> ./cap_during_tx.sh 144.650e6 1e6 6.0 0 20 g0.cf32 ofdm_off_48k.wav 20

# 3. OFF/ON A/B — interleave several bursts in ONE untouched run (coupling is
#    stable within a run but drifts between runs). 0 = max gain element here.
for i in 1 2 3; do
  ./cap_during_tx.sh 144.650e6 1e6 6.0 0 20 off_$i.cf32 ofdm_off_48k.wav 20
  ./cap_during_tx.sh 144.650e6 1e6 6.0 0 20 on_$i.cf32  ofdm_on_48k.wav  20
done

# 4. Analyse (per-file PSD normalised before averaging → coupling drift cancels).
python analyze_ab.py off_1.cf32,off_2.cf32,off_3.cf32 on_1.cf32,on_2.cf32,on_3.cf32

# 5. Attribute conditioner vs PA: digital OOB of the WAVs themselves.
python digital_oob.py ofdm_off_8k.wav ofdm_on_8k.wav OFDM52

# Drive-backoff sweep (find the ALC where the PA stops splattering): scale the WAV
# (ffmpeg volume=...) and key with keyplay_meter.py to read ALC alongside the ACPR.
```

## What we found (OFDM, FT-991A, 2 m, 20 W via 20 dB attenuator)

- **QPSK OFDM52**: CE-SSB ON does not worsen the mask (ACPR/OBW Δ negligible);
  +1.18 dB average power confirmed.
- **Dense OFDM-HOM** (8PSK/16QAM/32QAM/64QAM): CE-SSB raises average power a lot
  (+2.4…+7 dB), so at *full* drive the PA over-drives and splatters (+1–4 dB
  ACPR). `digital_oob.py` showed the conditioner itself is clean (±0.7 dB), so
  it's PA compression. A drive sweep on 32QAM ON: ACPR −21.7 dBc at slammed ALC
  → −29.1 dBc at moderate ALC, with the same output power. **Set audio drive for
  moderate ALC** and CE-SSB on HOM stays clean while keeping its power.

See [`docs/dev/roadmap.md`](../../docs/dev/roadmap.md) §10.8 for the full record.

## Files

| File | Host | Role |
|---|---|---|
| `sdr_capture.py` | SDR | RSP2 IQ capture → CF32 (element gains, peak/clip report) |
| `cap_during_tx.sh` | SDR | capture while triggering a station keyed burst over SSH |
| `analyze_ab.py` | SDR | Welch-PSD ACPR / 99% OBW, OFF/ON averaged (drift-robust) |
| `digital_oob.py` | SDR | conditioner-vs-PA attribution from the WAVs (no radio) |
| `keyplay.py` | station | gated CAT keyer + `aplay`, bulletproof unkey |
| `keyplay_meter.py` | station | same, plus PO/ALC meter polling for drive sweeps |
