---
title: On-Air Debugging Learnings
last_updated: 2026-06-09
status: in-progress
---

# On-Air Debugging Learnings

This document captures the root causes found and fixed during loopback and on-air debugging (2026-06-06 to 2026-06-09), plus the investigation strategy for completing the on-air validation.

---

## Part 1 — Loopback test: root causes and fixes

The `scripts/run-loopback-rpi51-rpi52.sh` test (IC-9700 USB soundcard cable between rpi51 and rpi52) was failing 100% of the time. Five interlocking bugs were identified and fixed in PRs #382 and #383.

### Bug 1 — CPAL hardware output buffer truncation

**Symptom:** CRC mismatch on every frame. Magic bytes correct, last 2 bytes wrong.

**Root cause:** `CpalOutputStream::flush()` returned as soon as the software ring-buffer queue drained. The USB soundcard has a ~2688-sample hardware output buffer (≈ 6 BPSK symbols at 8 kHz) that had not yet been played out to the cable. The stream was closed while the last 6 symbols were still queued in hardware. This destroyed the CRC.

**Fix:** Added a 200 ms sleep after the software queue empties, giving the soundcard hardware time to drain before the stream closes. (`crates/openpulse-audio/src/cpal_backend.rs`)

**Lesson:** CPAL's software queue depth ≠ hardware playback completion. Any signal that depends on the last samples being received must account for hardware buffer latency.

---

### Bug 2 — AFC settling on noise before signal arrives

**Symptom:** AFC correction locked to −8 Hz or −33 Hz on noise, then all decode attempts at the correct signal position used a wrong carrier frequency.

**Root cause:** `receive_with_timeout` runs a 6-pass Goertzel AFC settling on the first position where the energy gate fires. For the loopback path, the CPAL backend delivers a tiny initial batch of silence-plus-noise that trips the energy gate before the actual BPSK carrier arrives. The 6 passes then ran on noise and converged to a non-zero correction.

**Fix:** Two guards in the AFC settling block:
1. **Convergence check:** if the last two of six passes differ by ≥ 5 Hz, reject the estimate, reset `afc_correction_hz` to 0, and continue scanning.
2. **Minimum window guard:** defer settling until the window has at least `PREAMBLE_SYMS × step = 1024` samples so the Goertzel has enough data to work with.

(`crates/openpulse-modem/src/engine.rs`)

---

### Bug 3 — `find_timing_offset` phase ambiguity

**Symptom:** Even with correct AFC, the preamble correlation produced a wrong sub-symbol timing offset when the received signal had a 180° phase inversion (which DBPSK paths can introduce).

**Root cause:** `find_timing_offset` selected the offset with the highest `energy = Σ s×e` where `e` is the expected preamble pattern. A 180° phase flip makes all `s` values negative, so the sum is maximally negative. The algorithm was picking whichever offset had the least-negative sum (i.e. the wrong one).

**Fix:** Changed the comparison to `energy.abs()`, making the correlator phase-agnostic. (`plugins/bpsk/src/demodulate.rs`)

---

### Bug 4 — One-shot retry window too narrow

**Symptom:** After the full-frame timeout, the retry scanned only `fep ± step` (64 samples). The actual preamble was up to 1024 samples past `fep` due to CPAL startup latency on the transmit side.

**Root cause:** The retry was designed for the case where `fep` fires exactly at the preamble. With PulseAudio or CPAL startup jitter, `fep` can fire on the first audio batch while the preamble arrives up to one full preamble length later.

**Fix:** Extended the retry range to `fep−step .. fep + PREAMBLE_SYMS×step` (1024 samples forward). `find_timing_offset` handles the remaining sub-symbol alignment within each candidate start. (`crates/openpulse-modem/src/engine.rs`)

---

### Bug 5 — O(N²) AFC settling scan

**Symptom:** (Discovered during on-air testing, also latent for high-noise loopback.) When the noise floor is above `ENERGY_GATE_THRESHOLD`, every scan position fires the gate. The AFC settling was running 6 Goertzel passes over `max_frame_samples = 72960` samples per position — about 170 ms/position. The scan fell irreversibly behind: at step=32, it advanced 32 samples every 170 ms, meaning 5 seconds of audio takes 21 800 seconds to scan.

**Root cause:** The settling window was `start..start+max_frame_samples`, intended to give the Goertzel enough samples for high resolution. But when every position fires the gate, this becomes O(N²).

**Fix:** Use a short window — `start..start+step×32 = 1024 samples` — for AFC settling. This is 70× faster (< 3 ms/position) and still provides enough SNR to detect the BPSK carrier. (`crates/openpulse-modem/src/engine.rs`)

---

## Part 2 — On-air test: current status and known issues

As of 2026-06-09, the quick-tier on-air test (IC-9700 → 144.650 MHz → FT-991A, BPSK250) is still failing 0/3. The following issues have been identified.

### Issue A — Interferer at ≈1286 Hz in FT-991A receive audio

**Observation:** FT-991A receive audio shows a strong signal (`strength = −54 dBm`) even when the IC-9700 is NOT transmitting. During test cases the IQ-squaring AFC estimator consistently reports a raw estimate of ≈ −214 Hz, placing a dominant signal at ≈ 1286 Hz (= 1500 − 214). This is an RF signal at 144.650 + 0.001286 = 144.651286 MHz within the FT-991A's USB passband.

**Impact:** At both 5 W and 25 W IC-9700 output the interferer is strong enough to dominate the Goertzel scan. The AFC settles on the interferer (or gets a plausible-but-wrong correction) rather than the BPSK carrier at 1500 Hz.

**Current mitigation:** `AFC_MAX_CORRECTION_HZ = 100 Hz` rejects corrections with magnitude > 100 Hz. When the IC-9700 is transmitting at 25 W, the AFC occasionally settles to 0 Hz (BPSK carrier), but the decode still fails for the reason below.

---

### Issue B — Retry window misaligned with signal arrival

**Observation:** With PulseAudio on the FT-991A (dd2zm-landline), CPAL delivers audio in chunks of ≈ 28 000 samples (≈ 3.5 s). The energy gate fires on the first chunk at sample ≈ 5000 (from interferer noise), setting `first_energy_pos = 5000`. The BPSK signal from the IC-9700 (delayed by IRS startup + ISS CPAL startup) does not arrive until sample ≈ 52 000 — 47 000 samples after `fep`.

The current retry range is `fep − step .. fep + PREAMBLE_SYMS × step = 5000 .. 6024`. The signal at 52 000 is completely outside this range.

**Status:** This fix is pending. The retry must scan from `fep` to `accumulated.len() − min_frame_samples`, with an energy gate to skip silence efficiently.

---

### Issue C — PulseAudio chunk size / latency

**Observation:** FT-991A receive audio is delivered to the IRS binary in ≈ 3.5 s chunks via PulseAudio, instead of the small (200–400 sample) chunks typical of `plughw` direct access. This has two effects:
1. The IRS does not start processing until the first chunk arrives (≈ 3.5 s after start), compressing the effective listen window.
2. `fep` is set on early-chunk interference rather than the actual BPSK signal, misaligning the retry window (Issue B above).

This is the same `plughw` vs `pulse` distinction that caused the loopback to use `plughw:CARD=Device_1` directly. The on-air config uses `B_AUDIO_DEVICE=pulse` because the IC-9700 USB CODEC on rpi51 is held exclusively by PulseAudio. The FT-991A USB codec on dd2zm-landline might be accessible via `plughw` directly.

---

## Part 3 — On-air investigation strategy

### Step 1 — Capture raw audio to verify IC-9700 is transmitting

Before any further code changes, verify the RF path end-to-end:

```bash
# On dd2zm-landline: record 30 s of FT-991A audio during IC-9700 TX
arecord -D pulse -f S16_LE -r 8000 -c 1 -d 30 /tmp/ftir-capture.wav

# Then on a workstation: measure energy per second and identify tone frequencies
python3 - <<'EOF'
import wave, numpy as np
with wave.open('/tmp/ftir-capture.wav') as f:
    data = np.frombuffer(f.readframes(f.getnframes()), dtype=np.int16) / 32768.0
for i in range(0, len(data)-8000, 8000):
    chunk = data[i:i+8000]
    rms = np.sqrt(np.mean(chunk**2))
    fft = np.abs(np.fft.rfft(chunk))
    peak_hz = np.argmax(fft) * 8000 / len(chunk)
    print(f"t={i//8000:3d}s  rms={rms:.4f}  peak_hz={peak_hz:.0f}")
EOF
```

This tells you:
- Is there a signal burst matching ISS transmit timing? (rms jump when IC-9700 TXes)
- Is the carrier at 1500 Hz or elsewhere?
- How strong is the interferer vs the BPSK burst?

---

### Step 2 — Characterise CPAL chunk sizes and latency

Add a diagnostic to log CPAL callback timing on the IRS side:

```bash
# On dd2zm-landline: run openpulse receive for 20 s, check chunk sizes in log
openpulse --backend cpal --log debug receive --mode BPSK250 --listen-ms 20000 \
    2>&1 | grep "received.*audio samples"
```

Look for:
- Chunk size variation (should ideally be < 1024 samples at 8 kHz)
- Total accumulated samples vs wall-clock time (confirms no sample-rate mismatch)

If chunks are ≫ 1024, the PulseAudio buffer size should be reduced:
```bash
# In ~/.config/pulse/daemon.conf on dd2zm-landline:
default-fragments = 4
default-fragment-size-msec = 25
```

Or switch to direct ALSA access. Check if the FT-991A USB codec is free:
```bash
aplay -l   # should show FT-991A USB codec
arecord -D plughw:CARD=<FT-991A-card>,DEV=0 -f S16_LE -r 8000 -c 1 -d 5 /tmp/test.wav
```

If direct access works, set `B_AUDIO_DEVICE=plughw:CARD=<name>,DEV=0`.

---

### Step 3 — Verify sample-rate chain integrity

The on-air path includes two SRC stages:

```
openpulse (8 kHz) → PulseAudio SRC → USB codec (48 kHz)
       ↓ RF
FT-991A USB audio (48 kHz) → PulseAudio SRC → openpulse (8 kHz)
```

A mismatch at any stage causes the BPSK carrier to appear at the wrong audio frequency. Check:

```bash
# On rpi51 (TX side): confirm PulseAudio output sample rate to IC-9700 codec
pactl list sinks | grep -A5 "USB Audio CODEC"

# On dd2zm-landline (RX side): confirm PulseAudio input sample rate from FT-991A
pactl list sources | grep -A5 "FT-991\|USB Audio"
```

If PulseAudio resamples at a rate other than 48 kHz, the carrier shift is:
```
carrier_shift = center_freq × (actual_rate / expected_rate − 1)
```

For a 0.5% rate error at 1500 Hz: 7.5 Hz shift (benign). For a 6.25% error (e.g. 8000 vs 8500 Hz): 93 Hz shift (would push AFC to edge).

---

### Step 4 — Widen the retry scan

**Pending code fix** (Issue B above): change the one-shot retry to scan `fep .. accumulated.len() − min_frame_samples` with an energy gate, using AFC = 0. The energy gate (mean_sq > `ENERGY_GATE_THRESHOLD`) efficiently skips silence so only a few hundred positions near the actual signal get full decode attempts. Estimated cost: < 4 s for a 16 s buffer.

---

### Step 5 — Verify center-frequency alignment

The IC-9700 and FT-991A should both treat PKTUSB as USB with the signal at 1500 Hz above the dial frequency. Verify:

1. In the IC-9700: confirm DATA mode = USB (not LSB), DATA MOD input = USB, AF input level set to produce ≈ 50% ALC deflection.
2. In the FT-991A: confirm PKTUSB = USB, no clarifier offset, no RIT.
3. Check carrier offset with a second receiver (SDR or second rig on the same frequency in USB mode): tune to 144.650 MHz, look for BPSK250 carrier at 1500 Hz audio during IC-9700 TX.

---

### Step 0 — FT-991A PTT must be CAT, not RTS

The example config previously had `B_PTT_TYPE="RTS"`. This is wrong: RTS PTT does not key the FT-991A. **CAT PTT is required.** Confirmed by the operator via js8call and flrig. `docs/config/onair-ic9700-ft991a.example.sh` has been corrected to `B_PTT_TYPE="CAT"`. This was the reason side-B never transmitted in the `--reverse` run.

---

### Step 6 — Identify and eliminate the interferer

The persistent signal at ≈ 1286 Hz audio (= 144.651286 MHz RF) is a significant obstacle. It dominates the Goertzel AFC scan even at 25 W IC-9700 output.

Candidates:
- A local CW beacon on 144.651 MHz (check bandplan and DX cluster)
- IC-9700 local oscillator leakage heard by FT-991A over the air (unlikely at 10 km)
- FT-991A audio system artefact (check with a dummy load connected to the antenna port)
- SMPS or USB cable interference modulating the FT-991A audio

**Quick test:** connect a 50 Ω dummy load to the FT-991A antenna port and record audio. If the 1286 Hz tone disappears, it is an RF signal. If it remains, it is an audio system artefact.

---

### Summary of open action items

| # | Action | Owner | Status |
|---|--------|-------|--------|
| 1 | Capture raw WAV during IC-9700 TX, confirm carrier at 1500 Hz | On-air operator | Pending |
| 2 | Check PulseAudio chunk sizes on dd2zm-landline; try `plughw` direct access | On-air operator | Pending |
| 3 | Verify sample-rate chain (PulseAudio → USB codec) on both sides | On-air operator | Pending |
| 4 | Widen retry scan to cover full accumulated buffer | Code | Pending |
| 5 | Identify and eliminate 1286 Hz interferer | On-air operator | Pending |
| 6 | Re-run on-air test after all above resolved | On-air operator | Pending |
