---
title: On-Air Signal Chain Verification Plan
status: active
last_updated: 2026-06-10
---

# On-Air Signal Chain Verification Plan

> For the current investigation status, confirmed facts, and debugging root causes, see [`onair-status.md`](onair-status.md). This document is the preflight checklist to run before each test run.

This document defines a reproducible, gate-based procedure for verifying the full on-air signal chain between the IC-9700 (Side A, DC0SK, rpi51) and the FT-991A (Side B, DD2ZM, dd2zm-landline) before any openpulse on-air test run.

**Every gate must pass in order.** If a gate fails, stop, apply the corrective action, and re-run that gate from the top. Do not proceed to a later gate while an earlier one is failing.

---

## Signal chain and measurement points

```
SIDE B — TRANSMIT PATH

  [SW] openpulse transmit (8 kHz BPSK samples)
       │
       MP-B1: audio amplitude in software (loopback self-test)
       │
  [CPAL] PipeWire/PulseAudio → resample 8→48 kHz → FT-991A USB CODEC write
       │
       MP-B2: PipeWire sink volume & mute state
       │
  [HW] FT-991A USB CODEC (PCM2901, card 3) → AF MOD input
       │
       MP-B3: ALC meter (< 50% deflection = not overdriven; > 0 = modulating)
       MP-B4: RF power meter (RFM > 0 = RF output present)
       │
  [RF] FT-991A antenna → 144.640 MHz PKTUSB, ~2.5 W


RF PATH (10 km line-of-sight, 2 m)

       MP-C1: IC-9700 S-meter during FT-991A TX (> S5 expected; S9+18 confirmed)


SIDE A — RECEIVE PATH

  [HW] IC-9700 USB CODEC (PCM2901) → plughw capture
       │
       MP-A1: USB CODEC capture level idle vs. signal
               idle mean_sq ≈ 0.001; signal mean_sq should be ≥ 5× idle
       │
  [CPAL] plughw:CARD=CODEC,DEV=0 → 8 kHz mono
       │
       MP-A2: CPAL chunk sizes and sample rate integrity
       │
  [SW] openpulse receive → AFC settling → decode → payload match
       │
       MP-A3: AFC correction Hz (should be < 50 Hz if chain is aligned)
       MP-A4: payload match (PASS / FAIL)
```

---

## Required settings — IC-9700 (Side A)

These must be verified physically on the radio before any test run. They are not readable via CAT/rigctld.

| Setting | Location | Required value | Why |
|---------|----------|---------------|-----|
| DATA MOD | MENU → SET → Connectors → DATA MOD | USB | Routes USB audio to DATA mode TX input |
| DATA OFF MOD | MENU → SET → Connectors → DATA OFF MOD | USB | Ensures USB audio routes even with microphone connected |
| USB AF Output | MENU → SET → Connectors → USB AF Output | AF | Routes demodulated audio (not IF) to USB CODEC |
| USB MOD Level | MENU → SET → Connectors → USB MOD Level | ~50% (start at 50, adjust) | Input sensitivity from USB CODEC; affects ALC |
| USB AF Level | MENU → SET → Connectors → USB AF Level | ~50% (start at 50, adjust) | Output level to USB CODEC; affects capture mean_sq |
| DATA mode | VFO main mode button | USB-D or PKTUSB (not LSB) | Correct sideband for 1500 Hz carrier |
| AF Squelch | Physical squelch knob or MENU → SQL | Minimum / off | Squelch gates USB audio output |
| NR | MENU → SET → DSP | Off (NR = 0) | DSP noise reduction distorts BPSK signal |
| NB | MENU → SET → DSP | Off | Noise blanker distorts BPSK signal |
| RIT | RIT button | Off (LED unlit) | Offsets receive frequency |
| Clarifier | CLAR button | Off / 0 Hz | Offsets receive frequency |
| Preamp | PRE button | Off (or P.AMP 1 if signal is weak) | P.AMP 1 = +10 dB; use only if mean_sq at MP-A1 is < 0.005 |

---

## Required settings — FT-991A (Side B)

| Setting | Location | Required value | Why |
|---------|----------|---------------|-----|
| Data mode | Mode select | PKT-U (PKTUSB) | Digital USB mode; routes USB audio |
| PKT PTT | MENU 18 (PKT PTT SELECT) | CAT | RTS does not work on FT-991A; confirmed via js8call |
| Data Input Gain | MENU 040 (PKT MIC GAIN) | 50 (start) | USB input sensitivity into modulator |
| Data BW | MENU → DATA bandwidth | 3000 Hz (or Wide) | Passband must cover 1500 Hz ± 125 Hz carrier |
| RIT | RIT button | Off | Offsets receive audio |
| Clarifier | CLAR | Off / 0 Hz | Offsets receive audio |
| NR | DSP NR button | Off | DSP noise reduction distorts BPSK |
| NB | DSP NB button | Off | Noise blanker distorts BPSK |
| Compressor | PROC button | Off | Compressor should never be on for data modes |
| VOX | VOX button | Off | VOX must be off; PTT is via CAT |
| AF gain (physical knob) | Physical knob | ~12 o'clock | Sets speaker and USB output level simultaneously |
| SQL (squelch) | Physical squelch knob | Minimum (fully CCW) | Squelch gates USB capture audio |
| RF Power | Physical knob or MENU | 5 W (verified via rigctld RFPOWER readback) | Low power for test; must read ≥ 0.01 on Hamlib scale |

---

## Required settings — audio system

### Side A — rpi51 (IC-9700 receive)

| Parameter | Required value | Verification command |
|-----------|---------------|---------------------|
| CPAL capture device | `plughw:CARD=CODEC,DEV=0` | `arecord -l \| grep CODEC` |
| PulseAudio default source | Must NOT be the CODEC source (use plughw directly) | `pactl info \| grep "Default Source"` |
| ALSA PCM capture volume | ≥ 70% | `amixer -c CODEC get PCM` |
| ALSA Mic/Line capture volume | ≥ 70% (if separate) | `amixer -c CODEC get Mic` |
| Sample rate | 8000 Hz (native, no resampling) | Verified by `/proc/asound/card2/pcm0c/info` |

### Side B — dd2zm-landline (FT-991A transmit)

| Parameter | Required value | Verification command |
|-----------|---------------|---------------------|
| CPAL playback device | `pulse` (PipeWire) | `pactl list sinks short` |
| PipeWire sink volume | 80–100% | `pactl get-sink-volume @DEFAULT_SINK@` |
| PipeWire sink muted | No | `pactl get-sink-mute @DEFAULT_SINK@` |
| FT-991A USB CODEC sample rate | 48000 Hz (hardware) | `/proc/asound/card3/pcm0p/info` |
| PipeWire → USB resampling | 8000 → 48000 Hz | Verified by aplay rate readback |

---

## Gate 0 — Reboot, reset, and optional power cycle (run after any hardware or config change)

This gate resets both sides to a known clean state and should be run at the start of every session after any reboot, cable change, or config edit.

```bash
# On rpi51 (Side A):
# 1. Confirm IC-9700 USB CODEC is enumerated
ssh dc0sk@dc0sk-rpi51 "arecord -l | grep -i codec"
# Expected: card 2: CODEC [USB Audio CODEC], device 0: USB Audio [...]

# 2. Kill any stale openpulse or rigctld processes
ssh dc0sk@dc0sk-rpi51 "pkill -f openpulse; pkill -x rigctld; sleep 0.5"

# On dd2zm-landline (Side B):
# 1. Confirm FT-991A USB CODEC is enumerated
ssh dd2zm@dd2zm-landline "arecord -l | grep -i 'USB Audio CODEC\|Burr-Brown'"
# Expected: card 3: CODEC [USB Audio CODEC], device 0: USB Audio [...]

# 2. Kill any stale processes
ssh dd2zm@dd2zm-landline "pkill -f openpulse; pkill -x rigctld; sleep 0.5"

# 3. Confirm PipeWire sink for FT-991A is present
ssh dd2zm@dd2zm-landline "pactl list sinks short | grep -i 'usb\|codec\|burr'"
# Expected: at least one sink named something like 'alsa_output.usb-Burr-Brown...'
```

**Pass criterion:** both USB CODECs enumerated, no stale processes.

### Optional: CAT power cycle (add to Gate 0 when resuming after reboot or config change)

A CAT power cycle ensures both radios start from their factory-default state rather than inheriting settings from the previous session. Both radios enter a low-power CAT standby when powered off via CAT, so the same serial port handles power-on.

Enable by setting `POWER_CYCLE_ENABLE=1` in the profile. The script handles the full sequence automatically:

```
power off → wait POWER_OFF_WAIT (10 s) → power on → wait POWER_ON_WAIT (15 s) → verify CAT response
```

Manual equivalent if running outside the script:

```bash
# IC-9700 (Side A) — direct CAT, no rigctld daemon needed
rigctl -m 3081 -r /dev/serial/by-id/usb-Silicon_Labs_CP2102N_..._A-if00-port0 -s 115200 P 0
sleep 10
rigctl -m 3081 -r /dev/serial/by-id/usb-Silicon_Labs_CP2102N_..._A-if00-port0 -s 115200 P 1
sleep 15
rigctl -m 3081 -r /dev/serial/by-id/usb-Silicon_Labs_CP2102N_..._A-if00-port0 -s 115200 f

# FT-991A (Side B) — direct CAT
rigctl -m 1035 -r /dev/serial/by-id/usb-Silicon_Labs_CP2105_...-if00-port0 -s 38400 P 0
sleep 10
rigctl -m 1035 -r /dev/serial/by-id/usb-Silicon_Labs_CP2105_...-if00-port0 -s 38400 P 1
sleep 15
rigctl -m 1035 -r /dev/serial/by-id/usb-Silicon_Labs_CP2105_...-if00-port0 -s 38400 f
```

After power cycle, re-verify IC-9700 and FT-991A menu settings (the tables above) because some settings revert to defaults on power-on.

**Important:** the power cycle uses direct `rigctl` (not the rigctld daemon) so there is no port conflict. rigctld is started afterwards.

---

## Gate 1 — Radio settings and CAT connectivity

Checks that both radios are on the correct frequency, mode, and power via rigctld.

```bash
# Load profile
source docs/config/onair-ic9700-ft991a.example.sh

# Start rigctld on both stations
./scripts/run-onair-ic9700-ft991a.sh setup

# Verify via preflight_check (embedded in setup → run)
# Or manually:
rc_a="rigctl -m 2 -r 127.0.0.1:4532"
rc_b="rigctl -m 2 -r 127.0.0.1:4532"

ssh dc0sk@dc0sk-rpi51 "$rc_a f; $rc_a m; $rc_a l RFPOWER; $rc_a l STRENGTH"
ssh dd2zm@dd2zm-landline "$rc_b f; $rc_b m; $rc_b l RFPOWER"
```

**Pass criteria:**

| Check | Side A (IC-9700) | Side B (FT-991A) |
|-------|-----------------|-----------------|
| Frequency | 144640000 Hz | 144640000 Hz |
| Mode | PKTUSB or USB | PKTUSB |
| Passband | ≥ 2400 Hz | ≥ 2400 Hz |
| RF power | ≥ 0.01 (Hamlib) | ≥ 0.01 (Hamlib) |
| COMP | 0 | 0 |
| NB | 0 | 0 |
| NR | 0 | 0 |
| SQL | 0 | 0 |
| VOX | 0 | 0 |

**Corrective actions:**

- Frequency wrong → `rigctl -m 2 -r 127.0.0.1:4532 F 144640000`
- Mode wrong → set manually on radio front panel (CAT mode set is unreliable on IC-9700 for PKT modes)
- RF power reads 0 → set on radio front panel, verify with `rigctl l RFPOWER` after
- NB/NR/SQL nonzero → `rigctl -m 2 -r 127.0.0.1:4532 L NB 0` etc. (some may need front panel)

---

## Gate 2 — Side-B audio output (FT-991A TX path, MP-B2/MP-B3/MP-B4)

Verifies that Side B can transmit a carrier and it is visible on Side A's S-meter, while ALC is in the acceptable range.

### Step 2a — PipeWire sink volume check

```bash
ssh dd2zm@dd2zm-landline "
  pactl get-sink-volume @DEFAULT_SINK@
  pactl get-sink-mute @DEFAULT_SINK@
"
```

**Pass criterion:** Volume ≥ 80%, muted = no. If muted: `pactl set-sink-mute @DEFAULT_SINK@ 0`.

### Step 2b — Transmit a continuous tone and measure ALC + S-meter

```bash
# On dd2zm-landline: generate a 1500 Hz sine for 5 seconds via aplay → PipeWire → FT-991A
# This is the ALC calibration tone.

# First, assert PTT via rigctld
ssh dd2zm@dd2zm-landline "rigctl -m 2 -r 127.0.0.1:4532 T 1"

# On a second terminal: play 5s of 1500 Hz sine to default PipeWire sink
ssh dd2zm@dd2zm-landline "
  python3 -c \"
import math, struct, sys, wave, io
sr=8000; dur=5; f=1500; amp=0.4
samples=[int(amp*32767*math.sin(2*math.pi*f*i/sr)) for i in range(sr*dur)]
buf=struct.pack('<' + 'h'*len(samples), *samples)
sys.stdout.buffer.write(buf)
\" | aplay -f S16_LE -r 8000 -c 1 -D pulse"

# Immediately read ALC and RF power
ssh dd2zm@dd2zm-landline "
  for i in \$(seq 1 10); do
    alc=\$(rigctl -m 2 -r 127.0.0.1:4532 l ALC_METER 2>/dev/null || echo na)
    rfm=\$(rigctl -m 2 -r 127.0.0.1:4532 l RFPOWER_METER 2>/dev/null || echo na)
    printf 'ALC=%s  RFM=%s\n' \"\$alc\" \"\$rfm\"
    sleep 0.5
  done"

# While transmitting: read IC-9700 S-meter on Side A
ssh dc0sk@dc0sk-rpi51 "
  for i in \$(seq 1 10); do
    sm=\$(rigctl -m 2 -r 127.0.0.1:4532 l STRENGTH 2>/dev/null || echo na)
    printf 'STRENGTH=%s dBm\n' \"\$sm\"
    sleep 0.5
  done"

# Release PTT
ssh dd2zm@dd2zm-landline "rigctl -m 2 -r 127.0.0.1:4532 T 0"
```

**Pass criteria:**

| Metric | Acceptable range | Meaning |
|--------|-----------------|---------|
| ALC (Hamlib) | 0.05 – 0.40 | Signal modulating, not clipping. ALC = 0 means no audio input. ALC > 0.50 means overdriven. |
| RFM (Hamlib) | > 0.01 | RF output present |
| IC-9700 STRENGTH | > −73 dBm (S5) | RF path working. Expected S9+ at 10 km. |

**Corrective actions:**

- ALC = 0: check FT-991A DATA mode is PKTUSB, DATA IN GAIN (MENU 040) not zero, PipeWire not muted, correct sink selected. Try `pactl list sinks` to confirm which sink the audio is going to.
- ALC > 0.50: reduce MENU 040 (PKT MIC GAIN) or reduce PipeWire sink volume.
- RFM = 0 with ALC > 0: PTT not asserting. Check `rigctl T 1` actually shows PTT=1 on readback (`rigctl t`).
- S-meter on IC-9700 = na or < S3: check antenna cable, frequency, and IC-9700 mode.

### Step 2c — Verify ALC target with openpulse waveform

BPSK is multi-frequency; a sine tone calibration sets the peak level. openpulse at 8 kHz BPSK250 will typically peak ALC at 60–80% of the sine tone ALC. After gate 2b establishes a sine ALC of 0.15–0.35, run a real transmit and confirm ALC is still > 0.

```bash
source docs/config/onair-ic9700-ft991a.example.sh
./scripts/run-onair-ic9700-ft991a.sh sidea
# Observe: telemetry must show alc>0 > 0 and rfm>0 > 0
```

---

## Gate 3 — Side-A audio capture (IC-9700 RX path, MP-A1)

Verifies that the IC-9700 USB CODEC captures audio and that its level increases visibly when the FT-991A transmits.

### Step 3a — Idle capture baseline

```bash
# On rpi51: record 5 seconds of idle audio and measure mean_sq
ssh dc0sk@dc0sk-rpi51 "
  arecord -D plughw:CARD=CODEC,DEV=0 -f S16_LE -r 8000 -c 2 -d 5 /tmp/ic9700-idle.raw 2>/dev/null
  python3 -c \"
import struct, math
d = open('/tmp/ic9700-idle.raw','rb').read()
s = struct.unpack('<' + 'h'*(len(d)//2), d)
L, R = s[0::2], s[1::2]
msq_L = sum(x*x for x in L)/len(L)/32768**2
msq_R = sum(x*x for x in R)/len(R)/32768**2
print(f'idle: ch0(L) mean_sq={msq_L:.6f}  ch1(R) mean_sq={msq_R:.6f}')
\"
"
```

**Expected idle baseline:** L ≈ 0.0005–0.003, R ≈ 0.00002–0.0001. Note the actual values.

### Step 3b — Synchronized capture during FT-991A TX

```bash
# On rpi51: start a 20-second capture in the background
ssh dc0sk@dc0sk-rpi51 "
  arecord -D plughw:CARD=CODEC,DEV=0 -f S16_LE -r 8000 -c 2 -d 20 /tmp/ic9700-sync.raw &
  echo 'capture_pid='$!
" &

# On dd2zm-landline: transmit a 5-second BPSK250 test frame (≈ T+2s after capture start)
sleep 2
source docs/config/onair-ic9700-ft991a.example.sh
AR="$(ssh dc0sk@dc0sk-rpi51 "echo \$HOME/git/OpenPulseHF")"
ssh dd2zm@dd2zm-landline "
  BR=\$HOME/openpulse/OpenPulseHF
  nohup \$BR/target/release/openpulse \
    --backend cpal --log info --ptt rigctld \
    --rig 127.0.0.1:4532 \
    transmit --mode BPSK250 \
    'GATECHECK' >/tmp/tx-gatecheck.log 2>&1 &
  echo tx_pid=\$!
"

# Wait for capture to finish (20 s)
sleep 21

# Analyze per-second energy on both channels
ssh dc0sk@dc0sk-rpi51 "
python3 -c \"
import struct, math
d = open('/tmp/ic9700-sync.raw','rb').read()
s = struct.unpack('<' + 'h'*(len(d)//2), d)
L, R = s[0::2], s[1::2]
n = min(len(L), len(R))
print('t(s)   ch0_msq     ch1_msq')
for i in range(0, n-8000, 8000):
    q0 = sum(x*x for x in L[i:i+8000]) / 8000 / 32768**2
    q1 = sum(x*x for x in R[i:i+8000]) / 8000 / 32768**2
    print(f'{i//8000:4d}   {q0:.6f}   {q1:.6f}')
\"
"
```

**Pass criteria:**

| Metric | Condition |
|--------|-----------|
| Signal burst visible | At least one 1-second window shows ≥ 5× the idle mean_sq on ch0 (L) or ch1 (R) |
| Signal on expected channel | ch0 (L) shows the increase (IC-9700 USB CODEC routes RX audio to L in PKTUSB) |
| Signal level | Peak mean_sq ≥ 0.005 (this is needed for openpulse ENERGY_GATE_THRESHOLD detection) |

**Corrective actions if no signal burst:**
1. Confirm IC-9700 is receiving (S-meter shows S9+ during FT-991A TX — Gate 2 must pass first).
2. Check IC-9700 MENU → SET → Connectors → USB AF Output = AF (not IF).
3. Check IC-9700 AF gain knob is not at minimum.
4. Check squelch is fully off.
5. If signal burst is on ch1 (R) instead of ch0 (L): set `A_AUDIO_DEVICE` to use the correct channel, or verify DATA mode vs SSB mode routing in IC-9700 menus.

**Corrective actions if signal burst is too low (0.001–0.005):**
1. Increase IC-9700 USB AF Level (MENU → Connectors → USB AF Level).
2. Enable IC-9700 Preamp 1 (+10 dB RF, raises noise and signal equally — helps only if the issue is too-quiet USB output, not RF SNR).
3. Note: peak mean_sq of 0.002 is borderline — the openpulse energy gate threshold is configurable; proceed but note the level.

---

## Gate 4 — CPAL chunk size and sample rate integrity (MP-A2)

Verifies that the CPAL backend on rpi51 delivers audio in small, real-time chunks (not 3-second batches) and that the sample rate is correct.

```bash
source docs/config/onair-ic9700-ft991a.example.sh
AR="$(ssh dc0sk@dc0sk-rpi51 "echo \$HOME/git/OpenPulseHF")"

ssh dc0sk@dc0sk-rpi51 "
  '$AR/target/release/openpulse' \
    --backend cpal --log debug \
    --ptt none \
    receive --mode BPSK250 --listen-ms 10000 \
    --device 'plughw:CARD=CODEC,DEV=0' \
    2>&1 | grep -E 'audio.*samples|chunk|callback|received' | head -30
"
```

**Pass criteria:**

| Metric | Acceptable | Problem |
|--------|-----------|---------|
| Chunk size | ≤ 2048 samples (≤ 256 ms at 8 kHz) | > 8000 means PulseAudio batch delivery |
| Chunks per second | ≥ 4 | < 2 means audio is not streaming in real time |
| Total samples / wall-clock | ≈ 8000 per second (within ±2%) | Large deviation = sample rate mismatch |

**Corrective action if chunks are too large (> 8000 samples):**
- plughw is not being used — PulseAudio is intercepting. Run `fuser /dev/snd/pcmC2D0c` on rpi51 to see what holds the device.
- If PulseAudio holds it: `pactl suspend-source alsa_input.usb-Burr-Brown... 1` to suspend the PA source, then retry with plughw.
- Alternatively: in `/etc/pulse/default.pa` on rpi51, add `load-module module-alsa-source device=hw:CODEC tsched=0 fragment_size=1024` — but this is complex; prefer suspending the PA source.

---

## Gate 5 — Frequency alignment and bandwidth check

Verifies that the BPSK carrier is at 1500 Hz in the receive audio (not shifted by frequency offset, wrong mode, or sample rate error).

```bash
# On dd2zm-landline: transmit a known carrier for 10 seconds
# On rpi51: capture and compute peak FFT bin

ssh dc0sk@dc0sk-rpi51 "
  arecord -D plughw:CARD=CODEC,DEV=0 -f S16_LE -r 8000 -c 1 -d 10 /tmp/ic9700-fft.raw 2>/dev/null &"

sleep 1
ssh dd2zm@dd2zm-landline "
  # Generate 1500 Hz sine for 8 seconds
  python3 -c \"
import math, struct, sys
sr=8000; dur=8; f=1500; amp=0.35
s=[int(amp*32767*math.sin(2*math.pi*f*i/sr)) for i in range(sr*dur)]
sys.stdout.buffer.write(struct.pack('<'+'h'*len(s), *s))
\" | aplay -f S16_LE -r 8000 -c 1 -D pulse"

sleep 10

ssh dc0sk@dc0sk-rpi51 "
python3 -c \"
import struct, math
d = open('/tmp/ic9700-fft.raw','rb').read()
s = [x for (x,) in struct.iter_unpack('<h', d)]
n = len(s)
# FFT of middle 4 seconds
mid = s[n//4: n//4 + 4*8000]
mag = [0.0] * (len(mid)//2)
# Goertzel for each 50 Hz bin around 1500 Hz
for bin_f in range(1200, 1801, 25):
    w = 2*math.pi*bin_f/8000
    s1, s2 = 0.0, 0.0
    coef = 2*math.cos(w)
    for x in mid:
        s0 = x/32768 + coef*s1 - s2
        s2 = s1; s1 = s0
    power = s1**2 + s2**2 - coef*s1*s2
    print(f'{bin_f:5d} Hz  power={power:.6f}')
\"
"
```

**Pass criterion:** The highest power bin is at 1500 Hz ± 50 Hz.

**Corrective actions:**

| Observed peak | Cause | Fix |
|--------------|-------|-----|
| 1200–1400 Hz | IC-9700 or FT-991A in LSB mode | Set both to USB/PKTUSB |
| 1550–1700 Hz | Clarifier or RIT offset | Disable RIT and clarifier on both radios |
| 1300–1450 Hz | FT-991A dial frequency slightly off | No fix needed if < 100 Hz; update `TEST_FREQ_HZ` if systematic |
| > 100 Hz shift | Sample rate mismatch (e.g. 44100 vs 48000) | Check `/proc/asound/card3/pcm0p/info` on dd2zm-landline; ensure USB codec runs at 48000 |
| No identifiable peak | No RF received | Back to Gate 2/3 |

---

## Gate 6 — End-to-end decode with single test frame

The final gate. Transmits one BPSK250 frame and verifies the payload is decoded correctly.

```bash
source docs/config/onair-ic9700-ft991a.example.sh

# Run with reverse flag (FT-991A transmits, IC-9700 receives)
./scripts/run-onair-ic9700-ft991a.sh supervise \
  --single-case 'BPSK250|none|64' \
  --reverse

# Check the result JSON
ls -t docs/dev/test-reports/on-air/ | head -3
```

**Pass criterion:** `"result": "pass"` in the JSON report for the BPSK250 case.

**Diagnostic if this fails:**
1. Check IRS log tail (printed on failure) for:
   - `afc_correction` — if > 100 Hz, Gate 5 failed (carrier off frequency)
   - `mean_sq below threshold` — if present, Gate 3 failed (audio level too low)
   - `no preamble found` — timing issue; see Step 6a below
2. Inspect telemetry: `tel_iss_ptt_on`, `tel_iss_alc_nonzero`, `tel_iss_rfm_nonzero`

### Step 6a — AFC diagnostic (if decode fails with no preamble)

```bash
# Run receive side with --log debug for 30 seconds while FT-991A transmits manually
source docs/config/onair-ic9700-ft991a.example.sh
AR="$(ssh dc0sk@dc0sk-rpi51 "echo \$HOME/git/OpenPulseHF")"

ssh dc0sk@dc0sk-rpi51 "
  '$AR/target/release/openpulse' \
    --backend cpal --log debug \
    --ptt none \
    receive --mode BPSK250 --listen-ms 30000 \
    --device 'plughw:CARD=CODEC,DEV=0' \
    2>&1
" &
RECV_PID=$!

sleep 3  # let receiver start
# Manually trigger a transmit from Side B (or use run_side_a_transmit in reverse)
ssh dd2zm@dd2zm-landline "
  BR=\$HOME/openpulse/OpenPulseHF
  \$BR/target/release/openpulse \
    --backend cpal --log info --ptt rigctld \
    --rig 127.0.0.1:4532 \
    transmit --mode BPSK250 \
    'DIAGNOSTIC_PAYLOAD_ABC123' >/tmp/tx-diag.log 2>&1"

wait $RECV_PID
```

Look for these log lines in the receive output:

| Log line | Meaning |
|----------|---------|
| `afc_correction_hz=X` | AFC settled to X Hz; should be < 50 Hz |
| `energy_gate fired at pos=N` | First gate position — if N < 5000, likely idle noise trigger |
| `preamble found at pos=N` | Timing offset found — N should be within 2000 of energy_gate |
| `crc ok` | Frame decoded successfully |
| `afc settling rejected` | AFC did not converge (still noisy) |
| `preamble not found after retry` | Retry scan did not find preamble |

---

## Gate summary checklist

Run this checklist at the start of every session. Mark each gate Pass/Fail. Do not start the on-air test matrix until all are Pass.

```
Date/time: ___________
Operator A (rpi51): ___________
Operator B (dd2zm-landline): ___________
Git SHA: ___________
Test frequency: 144.640 MHz
Mode A: PKTUSB   Mode B: PKTUSB

[ ] Gate 0 — Reboot/reset: USB CODECs enumerated, no stale processes
[ ] Gate 0 (optional) — CAT power cycle: both radios responded with freq after power-on
[ ] IC-9700 menu settings verified (list above, 12 items)
[ ] FT-991A menu settings verified (list above, 11 items)
[ ] Gate 1 — CAT connectivity: freq match, RF power ≥ 0.01, all DSP off
[ ] Gate 2a — PipeWire sink: volume ≥ 80%, not muted
[ ] Gate 2b — Sine tone TX: ALC 0.05–0.40, RFM > 0, IC-9700 STRENGTH > −73 dBm
[ ] Gate 2c — openpulse TX: sidea smoke test passes (alc>0, rfm>0)
[ ] Gate 3a — Idle capture baseline: ch0 and ch1 mean_sq noted
[ ] Gate 3b — Sync capture during TX: peak mean_sq ≥ 5× idle, on ch0 (L)
[ ] Gate 4 — CPAL chunk sizes: ≤ 2048 samples, ≥ 4 chunks/s
[ ] Gate 5 — Carrier frequency: peak FFT bin at 1500 Hz ± 50 Hz
[ ] Gate 6 — Single frame decode: BPSK250 64B PASS

Measured values:
  Idle ch0 mean_sq: ___________
  Signal ch0 peak mean_sq: ___________
  ALC (tone): ___________   ALC (BPSK): ___________
  RFM (tone): ___________
  IC-9700 STRENGTH during TX: ___________
  BPSK carrier peak bin: ___________ Hz
  AFC correction at decode: ___________ Hz
```

---

## Bandwidth and level targets for test matrix

| Mode | Baud | 3 dB BW | Audio carrier | Minimum passband needed | Target ALC range |
|------|------|---------|---------------|------------------------|-----------------|
| BPSK31 | 31.25 | 62.5 Hz | 1500 Hz | 1437–1563 Hz | 0.05–0.25 |
| BPSK100 | 100 | 200 Hz | 1500 Hz | 1400–1600 Hz | 0.05–0.30 |
| BPSK250 | 250 | 500 Hz | 1500 Hz | 1250–1750 Hz | 0.10–0.35 |
| QPSK250 | 125 | 250 Hz | 1500 Hz | 1375–1625 Hz | 0.05–0.25 |
| QPSK500 | 250 | 500 Hz | 1500 Hz | 1250–1750 Hz | 0.10–0.35 |

All modes fit comfortably within the FT-991A PKTUSB 3 kHz passband. No passband adjustments needed between modes.

ALC targets: the audio playback level (PipeWire sink volume + FT-991A DATA IN GAIN) should be calibrated so BPSK250 produces ALC in the 0.10–0.35 range. Set using the Gate 2b sine tone calibration and confirm with Gate 2c.

---

## Hardware audio loopback regression checks

`run_loopback_regression()` runs `scripts/run-loopback-rpi51-rpi52.sh` — a real audio test over the USB cable connecting rpi51 (ISS, TX) to rpi52 (IRS, RX). It exercises the complete modem stack: signal generation → CPAL → USB soundcard → cable → USB soundcard → CPAL → AFC settling → decode. It does **not** use radios or PTT.

Before each run, the function streams the freshly-built rpi51 binary to `/home/dc0sk/openpulse/bin/openpulse` on rpi52, so both ends run identical code.

### Mode matrix

| Tier | Modes | Approximate duration |
|------|-------|---------------------|
| `quick` (4 cases) | BPSK31·32B, BPSK250·64B, QPSK250·64B, QPSK500·128B | ~100 s |
| `full` (8 cases) | above + BPSK63·32B, BPSK100·64B, QPSK125·64B, QPSK1000·128B | ~200 s |

### When it runs automatically (via `run-onair-ic9700-ft991a.sh`)

| Trigger | Tier | Rationale |
|---------|------|-----------|
| After build + transfer in `supervise`/`sidea` | **full** | Session start; thorough gate before any RF is keyed |
| At the start of a `run` action (no rebuild) | **quick** | Pre-existing binary; fast sanity check |
| After every on-air test failure | **quick** (default tier) | Distinguish signal-path vs. code regression |
| Every `LOOPBACK_REGRESSION_INTERVAL` test cases | **quick** | Periodic check; default 0 (disabled) — each run takes ~100 s |

### Manual invocation

```bash
source docs/config/onair-ic9700-ft991a.example.sh
./scripts/run-loopback-rpi51-rpi52.sh --full --output docs/dev/test-reports
# or for a single mode:
./scripts/run-loopback-rpi51-rpi52.sh --single-case 'BPSK250|64'
```

### Interpreting results

| Loopback result after on-air failure | Diagnosis | Next action |
|--------------------------------------|-----------|-------------|
| All cases PASS | Signal-path or RF problem | Work through Gates 2–6 |
| Some cases FAIL | Mode-specific regression | Check that mode in the affected plugin; rebuild |
| All cases FAIL | Broad modem regression or binary crash | Check git log; rebuild and re-run loopback before continuing |
| FAIL (no output / "binary deploy failed") | rpi52 unreachable or SSH key missing | Check SSH agent, rpi52 power, network |

### Radio settings check

Radio settings (frequency, mode, passband, RF power, COMP=0, NB=0, NR=0, SQL=0, VOX=0, audio levels) are verified by `preflight_check()`, which runs automatically:

- At the start of every `run_matrix()` call (all `supervise` and `run` actions)
- At the start of every `run_side_a_transmit()` call (`sidea` action)

`preflight_check()` applies corrections first (sets COMP/NB/NR/SQL/VOX to 0 via CAT), then reads back all values and fails hard on frequency mismatch, RF power = 0, or muted audio. It also warns on low audio levels, active squelch, or DSP filters that could not be cleared.

The verification plan checklist above (Gates 1–2) covers the same settings with manual commands for cases where rigctld is not yet running.

---

## Known issues and workarounds

### 1286 Hz interferer

A signal at approximately 144.641286 MHz has previously been observed in the FT-991A passband on 144.650 MHz. The test frequency has been moved to 144.640 MHz to reduce this. Before any test session, run Gate 3a (idle capture) and check for tones in the 800–1400 Hz range of the receive audio. If a persistent tone > idle + 10 dB is present:

1. Measure with a dummy load: connect a 50 Ω dummy load to the FT-991A antenna port and re-run Gate 3a. If the tone vanishes it is an RF signal; if it remains it is an audio system artefact.
2. If RF: move to 144.660 MHz and rerun Gate 5.
3. If audio artefact: check USB cable shielding, ferrite bead on FT-991A USB cable, and laptop power supply noise.

### PulseAudio monitor source on rpi51

If `pactl info | grep "Default Source"` shows a monitor source (contains `.monitor`), PulseAudio will capture the TX audio output, not the IC-9700 USB CODEC input. This produces a loopback echo, not received RF. Always use `plughw:CARD=CODEC,DEV=0` directly and avoid the PulseAudio source for Side A.

### FT-991A 48 kHz minimum rate

The FT-991A USB CODEC only supports 32000/44100/48000 Hz playback. openpulse generates 8 kHz audio. PipeWire or PulseAudio resamples 8→48 kHz. A 0.04% sample rate error at 48 kHz causes a 0.6 Hz tone shift — negligible. Verify the resampling chain with `pactl list sinks | grep -A3 "alsa_output.*USB"` and look for `rate = 48000 Hz`.

### IC-9700 plughw device exclusion by PulseAudio

If `arecord -D plughw:CARD=CODEC,DEV=0` fails with "device busy", PulseAudio holds the device. Run:

```bash
# Suspend the PulseAudio source for the IC-9700 CODEC
pactl suspend-source alsa_input.usb-Burr-Brown_from_TI_USB_Audio_CODEC-00.analog-stereo 1
# Then retry plughw access
```

Or stop PulseAudio entirely on rpi51: `systemctl --user stop pulseaudio` (requires re-enabling after testing).
