---
title: On-Air Test Handoff
status: in-progress
branch: fix/afc-settling-receive-with-timeout
last_updated: 2026-06-10
---

# On-Air Test Handoff

All loopback tests pass (PR #383). The on-air test still fails. This document is the current state of the investigation.

---

## Summary of confirmed facts

| Fact | Source |
|------|--------|
| FT-991A PTT via CAT works | `ptt_on=2`, `rfm>0=2` in test telemetry |
| FT-991A transmits at correct RF frequency | On-air spectrum observation |
| FT-991A audio chain works | FT-8 and other digital modes work on same hardware |
| IC-9700 receives FT-991A signal | User confirmed: visible on IC-9700 display, audible on speaker, `str_max=18` (≈ S9+18 dB) |
| IC-9700 USB CODEC can capture at 8000 Hz | `plughw:CARD=CODEC,DEV=0` confirmed working, idle mean_sq ≈ 0.001 |
| FT-991A USB CODEC minimum rate = 32000 Hz | `/proc/asound/card3/stream0`; capture only at 48000 Hz |
| rpi51 PulseAudio default source = TX output monitor | `pactl info` shows monitor source as default |

---

## The core open question

**Why does the IC-9700 USB audio capture show no signal burst during FT-991A TX?**

In a 20-second stereo raw capture (`plughw:CARD=CODEC,DEV=0`, 2-channel, 8 kHz) taken during a test run where the FT-991A was transmitting (`str_max=18`), both channels stayed completely flat:

```
t(s)   ch0_msq    ch1_msq
   4   0.000860   0.000027   ← FT-991A TX period (T≈5–7.5s from IRS start)
   5   0.000895   0.000025
   6   0.000831   0.000028
```

The IC-9700 shows S9+18 dB on the S-meter and the user hears the signal on the speaker. The USB audio level (ch0 idle ≈ 0.001 mean_sq) does not increase when the FT-991A transmits.

**Possible explanations (to investigate):**

1. **IC-9700 AGC / AF gain setting:** The IC-9700 may apply heavy AGC to the DATA/PKT mode audio, compressing the received signal to the same level as background noise. A 0 dB SNR at the USB audio output is possible even with a strong S-meter reading.

2. **Timing mismatch in the capture:** The stereo capture was run in parallel with the test script. The FT-991A only transmits for ~2.5 seconds (frame duration). If the capture window was not aligned with the TX window, the signal burst would be missed. **To verify:** do a synchronized capture during a known TX burst using the test script's PTT confirmation.

3. **Wrong channel:** The IC-9700 USB CODEC may route RX audio to ch1 (right) instead of ch0 (left) in PKTUSB mode. Our capture showed ch1 very quiet (mean_sq ≈ 0.000025), but this was also flat before and after the TX window. If the signal IS on ch1, the AGC might still compress it to the same level as idle noise.

4. **IC-9700 RX audio routing to USB:** Even if USB AF output is set to AF, the specific routing in PKTUSB mode may differ from SSB voice. There may be a separate menu setting for the PKT mode digital audio path.

5. **IC-9700 Ethernet interface:** The IC-9700 can be interfaced via Ethernet (LAN), which exposes the audio at a known sample rate independently of the USB CODEC configuration. Software that supports this interface (e.g., wfview as a reference implementation using the IC-9700 Ethernet API) could be used to provide audio to openpulse without the USB channel.

---

## What is known about the audio pipeline

### rpi51 (IC-9700 side)

| Path | Details |
|------|---------|
| IC-9700 USB CODEC | PCM2901, card 2 (`plughw:CARD=CODEC,DEV=0`). Supports 8000 Hz natively. |
| CPAL capture device | Must be `plughw:CARD=CODEC,DEV=0`. PulseAudio default source = output monitor (captures TX audio, not RX). |
| IC-9700 USB CODEC capture channels | ch0 (L): mean_sq ≈ 0.001 at idle. ch1 (R): mean_sq ≈ 0.000025 at idle (near silence). Neither shows signal burst. |
| IC-9700 audio during FT-991A TX | Flat — no measurable increase in USB audio level on either channel. |

### dd2zm-landline (FT-991A side)

| Path | Details |
|------|---------|
| FT-991A USB CODEC | PCM2901 (Burr-Brown). Playback: 32000/44100/48000 Hz only. Capture: 48000 Hz only. |
| CPAL device used | `pulse` (PipeWire-pulse). PipeWire resamples 8000→32000 Hz for TX, 48000→8000 Hz for RX. |
| ALC during TX | ALC=1 at peak during openpulse burst (barely deflects). Very low modulation level. |
| FT-8 / other modes | Work correctly — audio chain is functional. |

---

## Current state of code changes

| Component | Change | Status |
|-----------|--------|--------|
| `engine.rs` | AFC one-shot anchor, ±450 Hz correction range, wall-clock retry trigger | Committed (ff6a1b3) |
| `cli.rs` + `main.rs` | `--center-frequency <HZ>` flag for transmit and receive | Implemented, not yet committed |
| `onair-ic9700-ft991a.example.sh` | `A_AUDIO_DEVICE=plughw:CARD=CODEC,DEV=0` for IRS mode | Committed (ff6a1b3) |

---

## Next steps

### Step 1 — Synchronized audio capture during confirmed FT-991A TX

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

### Step 2 — Check IC-9700 low-level audio settings

If the USB audio level during TX is below the AFC settling threshold (`ENERGY_GATE_THRESHOLD = 0.0001`), the receive engine will not detect the signal. Check:
- IC-9700 menu → Data → Data MOD (should be USB)
- IC-9700 menu → Connectors → USB Serial Function (should be DATA for PKT)
- IC-9700 AF gain (physical knob or menu) — affects USB output level

### Step 3 — Lower energy gate if signal level is near noise

If Step 1 shows the IC-9700 USB audio has a slight signal increase (but below 0.0001 mean_sq), lower `ENERGY_GATE_THRESHOLD` in `engine.rs` and the retry gate from 0.01 to something matching the observed level.

### Step 4 — Consider IC-9700 Ethernet audio

The IC-9700 can serve audio directly over Ethernet (LAN connection, CI-V over TCP). This bypasses the USB CODEC entirely and provides audio at a configurable sample rate. wfview is one example of software that implements this interface; the IC-9700 Ethernet audio API is documented in the IC-9700 Advanced Manual. This path would:
- Eliminate any USB AF output routing question
- Provide audio at a well-known rate (8000/16000/24000/48000 Hz selectable)
- Potentially have better gain/level properties

---

## Hardware notes

| Item | Note |
|------|------|
| FT-991A PTT | CAT only (`B_PTT_TYPE="CAT"`). CAT port: `if00-port0`. |
| FT-991A RF power | Fixed at 5W remotely. ALC barely deflects during openpulse TX — modulation level is low. Investigate FT-991A DATA/PKT audio input gain in menu. |
| FT-991A USB CODEC | Min playback rate 32000 Hz; capture only 48000 Hz. PipeWire handles resampling. |
| IC-9700 USB capture | Must use `plughw:CARD=CODEC,DEV=0`, not `pulse` (PulseAudio default source is output monitor). |
| IC-9700 Ethernet | Alternative audio path: IC-9700 LAN port → CI-V/audio over TCP. See IC-9700 Advanced Manual. |
| IC-9700 preamp | Can enable (+20–30 dB). Raises noise floor equally — does not improve SNR, but may push audio above any fixed-level threshold if the current issue is low absolute level. |
| Frequency | 144.640 MHz (moved from 144.650 MHz to avoid 1286 Hz interferer in FT-991A passband). |
