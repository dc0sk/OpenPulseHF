---
title: On-Air Test Status
status: in-progress
last_updated: 2026-06-10
---

# On-Air Test Status

**Status as of 2026-06-10.** This document is the current state of the on-air validation effort. It consolidates the on-air test handoff and the on-air debugging learnings into one place: current blockers, confirmed facts, and the root causes found and fixed during loopback and on-air debugging (2026-06-06 to 2026-06-09).

For the reproducible, gate-based preflight procedure to run before each test run, see [`onair-signal-chain-verification.md`](onair-signal-chain-verification.md).

For the full sequenced campaign (the phases that turn this status into the 1.0 group-A evidence), see [`onair-execution-plan.md`](onair-execution-plan.md). **The headline from that plan:** the modem, every waveform, the decoder and two transmitters are already SDR-proven on real RF; the rig→rig blocker is conducted computer RFI into the receive USB audio, whose recorded fix is galvanic USB isolation — not a modem change. Kill that first (Phase G0) before any further matrix run.

---

## Current status / blockers

All loopback tests pass (PR #383). The on-air test still fails.

> **2026-06-10 update:** After rebooting rpi51 and the FT-991A, the IC-9700 USB CODEC on Side A now shows the received level nearly doubling when Side B (FT-991A) transmits. The blocking mystery from the original handoff (flat USB audio during TX) is resolved. The signal chain plan is in [`onair-signal-chain-verification.md`](onair-signal-chain-verification.md). Run all gates in order before the next test run.

As of 2026-06-09, the quick-tier on-air test (IC-9700 → 144.650 MHz → FT-991A, BPSK250) was still failing 0/3. The frequency has since been moved to 144.640 MHz (from 144.650 MHz to avoid the 1286 Hz interferer in the FT-991A passband).

### The (now-resolved) core open question

**Why does the IC-9700 USB audio capture show no signal burst during FT-991A TX?**

In a 20-second stereo raw capture (`plughw:CARD=CODEC,DEV=0`, 2-channel, 8 kHz) taken during a test run where the FT-991A was transmitting (`str_max=18`), both channels stayed completely flat:

```
t(s)   ch0_msq    ch1_msq
   4   0.000860   0.000027   ← FT-991A TX period (T≈5–7.5s from IRS start)
   5   0.000895   0.000025
   6   0.000831   0.000028
```

The IC-9700 showed S9+18 dB on the S-meter and the user heard the signal on the speaker. The USB audio level (ch0 idle ≈ 0.001 mean_sq) did not increase when the FT-991A transmitted.

This was resolved by the 2026-06-10 reboot of rpi51 and the FT-991A (see the update note above). The explanations investigated before the reboot are retained below for reference.

**Possible explanations (investigated):**

1. **IC-9700 AGC / AF gain setting:** The IC-9700 may apply heavy AGC to the DATA/PKT mode audio, compressing the received signal to the same level as background noise. A 0 dB SNR at the USB audio output is possible even with a strong S-meter reading.

2. **Timing mismatch in the capture:** The stereo capture was run in parallel with the test script. The FT-991A only transmits for ~2.5 seconds (frame duration). If the capture window was not aligned with the TX window, the signal burst would be missed. **To verify:** do a synchronized capture during a known TX burst using the test script's PTT confirmation.

3. **Wrong channel:** The IC-9700 USB CODEC may route RX audio to ch1 (right) instead of ch0 (left) in PKTUSB mode. Our capture showed ch1 very quiet (mean_sq ≈ 0.000025), but this was also flat before and after the TX window. If the signal IS on ch1, the AGC might still compress it to the same level as idle noise.

4. **IC-9700 RX audio routing to USB:** Even if USB AF output is set to AF, the specific routing in PKTUSB mode may differ from SSB voice. There may be a separate menu setting for the PKT mode digital audio path.

5. **IC-9700 Ethernet interface:** The IC-9700 can be interfaced via Ethernet (LAN), which exposes the audio at a known sample rate independently of the USB CODEC configuration. Software that supports this interface (e.g., wfview as a reference implementation using the IC-9700 Ethernet API) could be used to provide audio to openpulse without the USB channel.

### Next steps

#### Step 1 — Synchronized audio capture during confirmed FT-991A TX

Run the stereo IC-9700 capture in the same test window as the full test script (which confirms PTT assertion), and analyze the per-second energy precisely in the FT-991A TX window:

```bash
# On rpi51: start 20-second capture in background
ssh dc0sk@dc0sk-rpi51 "arecord -D plughw:CARD=CODEC,DEV=0 -f S16_LE -r 8000 -c 2 -d 20 /tmp/ic9700-sync.raw &"

# Immediately run the test (FT-991A TX starts at ~T=5s):
source docs/config/onair-ic9700-ft991a.example.sh
export A_AUDIO_DEVICE="plughw:CARD=CODEC,DEV=0"
./scripts/run-onair-ic9700-ft991a.sh supervise --single-case 'BPSK250|none|64' --reverse

# Analyze capture:
ssh dc0sk@dc0sk-rpi51 "python3 -c \"
import struct, math
d = open('/tmp/ic9700-sync.raw','rb').read()
s = struct.unpack('<'+'h'*(len(d)//2), d)
L, R = s[0::2], s[1::2]
for i in range(0, min(len(L),len(R))-8000, 8000):
    q0 = sum(x*x for x in L[i:i+8000])/8000/32768**2
    q1 = sum(x*x for x in R[i:i+8000])/8000/32768**2
    print(f't={i//8000:3d}s  L={q0:.6f}  R={q1:.6f}')
\""
```

This will reveal whether the IC-9700 USB audio shows ANY level change during the FT-991A TX window, and which channel carries the signal.

#### Step 2 — Check IC-9700 low-level audio settings

If the USB audio level during TX is below the AFC settling threshold (`ENERGY_GATE_THRESHOLD = 0.0001`), the receive engine will not detect the signal. Check:
- IC-9700 menu → Data → Data MOD (should be USB)
- IC-9700 menu → Connectors → USB Serial Function (should be DATA for PKT)
- IC-9700 AF gain (physical knob or menu) — affects USB output level

#### Step 3 — Lower energy gate if signal level is near noise

If Step 1 shows the IC-9700 USB audio has a slight signal increase (but below 0.0001 mean_sq), lower `ENERGY_GATE_THRESHOLD` in `engine.rs` and the retry gate from 0.01 to something matching the observed level.

#### Step 4 — Consider IC-9700 Ethernet audio

The IC-9700 can serve audio directly over Ethernet (LAN connection, CI-V over TCP). This bypasses the USB CODEC entirely and provides audio at a configurable sample rate. wfview is one example of software that implements this interface; the IC-9700 Ethernet audio API is documented in the IC-9700 Advanced Manual. This path would:
- Eliminate any USB AF output routing question
- Provide audio at a well-known rate (8000/16000/24000/48000 Hz selectable)
- Potentially have better gain/level properties

### Current state of code changes

| Component | Change | Status |
|-----------|--------|--------|
| `engine.rs` | AFC one-shot anchor, ±450 Hz correction range, wall-clock retry trigger | Committed (ff6a1b3) |
| `cli.rs` + `main.rs` | `--center-frequency <HZ>` flag for transmit and receive | Implemented, not yet committed |
| `onair-ic9700-ft991a.example.sh` | `A_AUDIO_DEVICE=plughw:CARD=CODEC,DEV=0` for IRS mode | Committed (ff6a1b3) |

---

## Confirmed facts

| Fact | Source |
|------|--------|
| FT-991A PTT via CAT works | `ptt_on=2`, `rfm>0=2` in test telemetry |
| FT-991A transmits at correct RF frequency | On-air spectrum observation |
| FT-991A audio chain works | FT-8 and other digital modes work on same hardware |
| IC-9700 receives FT-991A signal | User confirmed: visible on IC-9700 display, audible on speaker, `str_max=18` (≈ S9+18 dB) |
| IC-9700 USB CODEC can capture at 8000 Hz | `plughw:CARD=CODEC,DEV=0` confirmed working, idle mean_sq ≈ 0.001 |
| FT-991A USB CODEC minimum rate = 32000 Hz | `/proc/asound/card3/stream0`; capture only at 48000 Hz |
| rpi51 PulseAudio default source = TX output monitor | `pactl info` shows monitor source as default |

### What is known about the audio pipeline

#### rpi51 (IC-9700 side)

| Path | Details |
|------|---------|
| IC-9700 USB CODEC | PCM2901, card 2 (`plughw:CARD=CODEC,DEV=0`). Supports 8000 Hz natively. |
| CPAL capture device | Must be `plughw:CARD=CODEC,DEV=0`. PulseAudio default source = output monitor (captures TX audio, not RX). |
| IC-9700 USB CODEC capture channels | ch0 (L): mean_sq ≈ 0.001 at idle. ch1 (R): mean_sq ≈ 0.000025 at idle (near silence). Neither shows signal burst. |
| IC-9700 audio during FT-991A TX | Flat — no measurable increase in USB audio level on either channel. (Resolved after 2026-06-10 reboot.) |

#### dd2zm-landline (FT-991A side)

| Path | Details |
|------|---------|
| FT-991A USB CODEC | PCM2901 (Burr-Brown). Playback: 32000/44100/48000 Hz only. Capture: 48000 Hz only. |
| CPAL device used | `pulse` (PipeWire-pulse). PipeWire resamples 8000→32000 Hz for TX, 48000→8000 Hz for RX. |
| ALC during TX | ALC=1 at peak during openpulse burst (barely deflects). Very low modulation level. |
| FT-8 / other modes | Work correctly — audio chain is functional. |

### Hardware notes

| Item | Note |
|------|------|
| FT-991A PTT | CAT only (`B_PTT_TYPE="CAT"`). CAT port: `if00-port0`. |
| FT-991A RF power | Fixed at 5W remotely. ALC barely deflects during openpulse TX — modulation level is low. Investigate FT-991A DATA/PKT audio input gain in menu. |
| FT-991A USB CODEC | Min playback rate 32000 Hz; capture only 48000 Hz. PipeWire handles resampling. |
| IC-9700 USB capture | Must use `plughw:CARD=CODEC,DEV=0`, not `pulse` (PulseAudio default source is output monitor). |
| IC-9700 Ethernet | Alternative audio path: IC-9700 LAN port → CI-V/audio over TCP. See IC-9700 Advanced Manual. |
| IC-9700 preamp | Can enable (+20–30 dB). Raises noise floor equally — does not improve SNR, but may push audio above any fixed-level threshold if the current issue is low absolute level. |
| Frequency | 144.640 MHz (moved from 144.650 MHz to avoid 1286 Hz interferer in FT-991A passband). |

---

## Debugging findings & root causes

This section captures the root causes found and fixed during loopback and on-air debugging (2026-06-06 to 2026-06-09), plus the on-air-specific issues identified during validation.

### Part 1 — Loopback test: root causes and fixes

The `scripts/run-loopback-rpi51-rpi52.sh` test (IC-9700 USB soundcard cable between rpi51 and rpi52) was failing 100% of the time. Five interlocking bugs were identified and fixed in PRs #382 and #383.

#### Bug 1 — CPAL hardware output buffer truncation

**Symptom:** CRC mismatch on every frame. Magic bytes correct, last 2 bytes wrong.

**Root cause:** `CpalOutputStream::flush()` returned as soon as the software ring-buffer queue drained. The USB soundcard has a ~2688-sample hardware output buffer (≈ 6 BPSK symbols at 8 kHz) that had not yet been played out to the cable. The stream was closed while the last 6 symbols were still queued in hardware. This destroyed the CRC.

**Fix:** Added a 200 ms sleep after the software queue empties, giving the soundcard hardware time to drain before the stream closes. (`crates/openpulse-audio/src/cpal_backend.rs`)

**Lesson:** CPAL's software queue depth ≠ hardware playback completion. Any signal that depends on the last samples being received must account for hardware buffer latency.

#### Bug 2 — AFC settling on noise before signal arrives

**Symptom:** AFC correction locked to −8 Hz or −33 Hz on noise, then all decode attempts at the correct signal position used a wrong carrier frequency.

**Root cause:** `receive_with_timeout` runs a 6-pass Goertzel AFC settling on the first position where the energy gate fires. For the loopback path, the CPAL backend delivers a tiny initial batch of silence-plus-noise that trips the energy gate before the actual BPSK carrier arrives. The 6 passes then ran on noise and converged to a non-zero correction.

**Fix:** Two guards in the AFC settling block:
1. **Convergence check:** if the last two of six passes differ by ≥ 5 Hz, reject the estimate, reset `afc_correction_hz` to 0, and continue scanning.
2. **Minimum window guard:** defer settling until the window has at least `PREAMBLE_SYMS × step = 1024` samples so the Goertzel has enough data to work with.

(`crates/openpulse-modem/src/engine.rs`)

#### Bug 3 — `find_timing_offset` phase ambiguity

**Symptom:** Even with correct AFC, the preamble correlation produced a wrong sub-symbol timing offset when the received signal had a 180° phase inversion (which DBPSK paths can introduce).

**Root cause:** `find_timing_offset` selected the offset with the highest `energy = Σ s×e` where `e` is the expected preamble pattern. A 180° phase flip makes all `s` values negative, so the sum is maximally negative. The algorithm was picking whichever offset had the least-negative sum (i.e. the wrong one).

**Fix:** Changed the comparison to `energy.abs()`, making the correlator phase-agnostic. (`plugins/bpsk/src/demodulate.rs`)

#### Bug 4 — One-shot retry window too narrow

**Symptom:** After the full-frame timeout, the retry scanned only `fep ± step` (64 samples). The actual preamble was up to 1024 samples past `fep` due to CPAL startup latency on the transmit side.

**Root cause:** The retry was designed for the case where `fep` fires exactly at the preamble. With PulseAudio or CPAL startup jitter, `fep` can fire on the first audio batch while the preamble arrives up to one full preamble length later.

**Fix:** Extended the retry range to `fep−step .. fep + PREAMBLE_SYMS×step` (1024 samples forward). `find_timing_offset` handles the remaining sub-symbol alignment within each candidate start. (`crates/openpulse-modem/src/engine.rs`)

#### Bug 5 — O(N²) AFC settling scan

**Symptom:** (Discovered during on-air testing, also latent for high-noise loopback.) When the noise floor is above `ENERGY_GATE_THRESHOLD`, every scan position fires the gate. The AFC settling was running 6 Goertzel passes over `max_frame_samples = 72960` samples per position — about 170 ms/position. The scan fell irreversibly behind: at step=32, it advanced 32 samples every 170 ms, meaning 5 seconds of audio takes 21 800 seconds to scan.

**Root cause:** The settling window was `start..start+max_frame_samples`, intended to give the Goertzel enough samples for high resolution. But when every position fires the gate, this becomes O(N²).

**Fix:** Use a short window — `start..start+step×32 = 1024 samples` — for AFC settling. This is 70× faster (< 3 ms/position) and still provides enough SNR to detect the BPSK carrier. (`crates/openpulse-modem/src/engine.rs`)

### Part 2 — On-air test: identified issues

The following on-air issues were identified during validation (status as of 2026-06-09).

#### Issue A — Interferer at ≈1286 Hz in FT-991A receive audio

**Observation:** FT-991A receive audio shows a strong signal (`strength = −54 dBm`) even when the IC-9700 is NOT transmitting. During test cases the IQ-squaring AFC estimator consistently reports a raw estimate of ≈ −214 Hz, placing a dominant signal at ≈ 1286 Hz (= 1500 − 214). This is an RF signal at 144.650 + 0.001286 = 144.651286 MHz within the FT-991A's USB passband.

**Impact:** At both 5 W and 25 W IC-9700 output the interferer is strong enough to dominate the Goertzel scan. The AFC settles on the interferer (or gets a plausible-but-wrong correction) rather than the BPSK carrier at 1500 Hz.

**Current mitigation:** `AFC_MAX_CORRECTION_HZ = 100 Hz` rejects corrections with magnitude > 100 Hz. When the IC-9700 is transmitting at 25 W, the AFC occasionally settles to 0 Hz (BPSK carrier), but the decode still fails for the reason below. The test frequency has since been moved to 144.640 MHz to reduce this interferer.

#### Issue B — Retry window misaligned with signal arrival

**Observation:** With PulseAudio on the FT-991A (dd2zm-landline), CPAL delivers audio in chunks of ≈ 28 000 samples (≈ 3.5 s). The energy gate fires on the first chunk at sample ≈ 5000 (from interferer noise), setting `first_energy_pos = 5000`. The BPSK signal from the IC-9700 (delayed by IRS startup + ISS CPAL startup) does not arrive until sample ≈ 52 000 — 47 000 samples after `fep`.

The current retry range is `fep − step .. fep + PREAMBLE_SYMS × step = 5000 .. 6024`. The signal at 52 000 is completely outside this range.

**Status:** This fix is pending. The retry must scan from `fep` to `accumulated.len() − min_frame_samples`, with an energy gate to skip silence efficiently.

#### Issue C — PulseAudio chunk size / latency

**Observation:** FT-991A receive audio is delivered to the IRS binary in ≈ 3.5 s chunks via PulseAudio, instead of the small (200–400 sample) chunks typical of `plughw` direct access. This has two effects:
1. The IRS does not start processing until the first chunk arrives (≈ 3.5 s after start), compressing the effective listen window.
2. `fep` is set on early-chunk interference rather than the actual BPSK signal, misaligning the retry window (Issue B above).

This is the same `plughw` vs `pulse` distinction that caused the loopback to use `plughw:CARD=Device_1` directly. The on-air config uses `B_AUDIO_DEVICE=pulse` because the IC-9700 USB CODEC on rpi51 is held exclusively by PulseAudio. The FT-991A USB codec on dd2zm-landline might be accessible via `plughw` directly.

### Part 3 — On-air investigation strategy

The following steps describe the broader RF-path investigation. The gate-based preflight procedure in [`onair-signal-chain-verification.md`](onair-signal-chain-verification.md) operationalizes most of these checks; this strategy is retained for the underlying rationale.

#### Step 1 — Capture raw audio to verify IC-9700 is transmitting

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

#### Step 2 — Characterise CPAL chunk sizes and latency

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

#### Step 3 — Verify sample-rate chain integrity

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

#### Step 4 — Widen the retry scan

**Pending code fix** (Issue B above): change the one-shot retry to scan `fep .. accumulated.len() − min_frame_samples` with an energy gate, using AFC = 0. The energy gate (mean_sq > `ENERGY_GATE_THRESHOLD`) efficiently skips silence so only a few hundred positions near the actual signal get full decode attempts. Estimated cost: < 4 s for a 16 s buffer.

#### Step 5 — Verify center-frequency alignment

The IC-9700 and FT-991A should both treat PKTUSB as USB with the signal at 1500 Hz above the dial frequency. Verify:

1. In the IC-9700: confirm DATA mode = USB (not LSB), DATA MOD input = USB, AF input level set to produce ≈ 50% ALC deflection.
2. In the FT-991A: confirm PKTUSB = USB, no clarifier offset, no RIT.
3. Check carrier offset with a second receiver (SDR or second rig on the same frequency in USB mode): tune to 144.650 MHz, look for BPSK250 carrier at 1500 Hz audio during IC-9700 TX.

#### Step 0 — FT-991A PTT must be CAT, not RTS

The example config previously had `B_PTT_TYPE="RTS"`. This is wrong: RTS PTT does not key the FT-991A. **CAT PTT is required.** Confirmed by the operator via js8call and flrig. `docs/config/onair-ic9700-ft991a.example.sh` has been corrected to `B_PTT_TYPE="CAT"`. This was the reason side-B never transmitted in the `--reverse` run.

#### Step 6 — Identify and eliminate the interferer

The persistent signal at ≈ 1286 Hz audio (= 144.651286 MHz RF) is a significant obstacle. It dominates the Goertzel AFC scan even at 25 W IC-9700 output.

Candidates:
- A local CW beacon on 144.651 MHz (check bandplan and DX cluster)
- IC-9700 local oscillator leakage heard by FT-991A over the air (unlikely at 10 km)
- FT-991A audio system artefact (check with a dummy load connected to the antenna port)
- SMPS or USB cable interference modulating the FT-991A audio

**Quick test:** connect a 50 Ω dummy load to the FT-991A antenna port and record audio. If the 1286 Hz tone disappears, it is an RF signal. If it remains, it is an audio system artefact.

### Summary of open action items

| # | Action | Owner | Status |
|---|--------|-------|--------|
| 1 | Capture raw WAV during IC-9700 TX, confirm carrier at 1500 Hz | On-air operator | Pending |
| 2 | Check PulseAudio chunk sizes on dd2zm-landline; try `plughw` direct access | On-air operator | Pending |
| 3 | Verify sample-rate chain (PulseAudio → USB codec) on both sides | On-air operator | Pending |
| 4 | Widen retry scan to cover full accumulated buffer | Code | Pending |
| 5 | Identify and eliminate 1286 Hz interferer | On-air operator | Pending |
| 6 | Re-run on-air test after all above resolved | On-air operator | Pending |
