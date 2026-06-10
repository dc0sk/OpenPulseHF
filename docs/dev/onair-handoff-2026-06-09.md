---
title: On-Air Test Handoff
status: in-progress
branch: fix/afc-settling-receive-with-timeout
last_updated: 2026-06-10
---

# On-Air Test Handoff

All loopback tests pass (PR #383). The on-air test still fails. This document captures the exact state of the investigation.

---

## What is working

- **Loopback test (rpi51 → cable → rpi52)**: 3/3 PASS, merged in PR #383.
- **FT-991A PTT via CAT**: confirmed working (`ptt_on=2`, `rfm=2` in telemetry). `B_PTT_TYPE="CAT"`.
- **IC-9700 RF path (reverse)**: IC-9700 S-meter shows `str_max=18` (≈ S9+18dB = −55 dBm) during FT-991A TX, confirming a real RF signal arrives at the IC-9700 antenna.
- **FT-991A USB audio**: FT-991A properly outputs RX audio via USB CODEC (CPAL can capture it; the 1286 Hz interferer at 144.650 MHz was audible via USB).

---

## Root cause of current on-air failure

### IC-9700 USB AF output not enabled

**Observation (2026-06-10):**  
Stereo raw capture from the IC-9700 USB CODEC (`plughw:CARD=CODEC,DEV=0`, 2-channel, 8 kHz) during a real FT-991A transmit burst shows NO energy jump on either channel:

| Second | ch0 mean_sq | ch1 mean_sq |
|--------|------------|------------|
| 0 | 0.001276 | 0.000192 |
| 1–19 | 0.000831–0.001276 | 0.000023–0.000030 |
| (FT-991A TX at ~T=5–7.5s) | **no jump** | **no jump** |

The IC-9700 S-meter simultaneously shows `str_max=18` (S9+18 dB, confirmed real signal). The user confirms they can **see and hear** the FT-991A signal on the IC-9700 display and speaker. But the USB CODEC output carries no RX audio.

**Root cause:**  
The IC-9700 is not routing its received audio to the USB CODEC output. In PKTUSB mode, the IC-9700 plays received audio through its internal speaker/headphone but does NOT send it to the USB audio device unless explicitly configured in the menu.

**Required fix (hardware, must be done on the rig):**  
IC-9700 menu → Set → Connectors → **USB AF/IF Output** → set to **AF** (not IF or Off).  
Also verify: Menu → Set → Connectors → **MOD Level** (DATA MOD source for TX audio) and that PKT AF input is set to **USB**.

This is the single gating issue for the reverse (FT-991A ISS → IC-9700 IRS) test direction.

---

### AFC locking onto noise carrier before signal arrives (forward direction)

**Observation:**  
In the forward direction (IC-9700 ISS → FT-991A IRS), the AFC settles at T=0.15s (before the IC-9700 signal arrives at T=5s) on a noise/QRM carrier in the IC-9700 audio. The settled correction is +175Hz (1675Hz tone in the audio), not the IC-9700's carrier offset.

The main scan then runs the full session at +175Hz and never decodes the real signal.

**Root cause:**  
The AFC settling fires on the first stable carrier above the noise floor (`ENERGY_GATE_THRESHOLD = 0.0001`). On the IC-9700 channel there is a persistent carrier at ≈1675Hz (probably an audio artifact of the IC-9700 USB CODEC or residual QRM) that is stable enough to pass the stability guard. This carrier is weaker than the actual IC-9700 BPSK signal but arrives earlier.

**Mitigation added (this branch):**  
- One-shot anchor + 5-pass fine-track in both main-scan settling and retry mini-settle.
- Stability guard: |after_fine − after_anchor| ≤ 20 Hz (noise-oscillating estimates are rejected).
- AFC_MAX_CORRECTION_HZ raised from 100 Hz to 450 Hz to accommodate the ~300–400 Hz rig crystal offsets.
- Retry trigger: wall-clock T ≥ 12s + re-arm every 2s (rate-independent).
- Retry energy gate: 0.01 (tries to skip noise-only positions; see limitation below).

**Limitation:**  
The retry energy gate at 0.01 was set assuming FT-991A signal mean_sq ≫ noise (0.001). If the IC-9700 USB AF output is also not configured for the non-reverse IRS side (which is the FT-991A IRS — a separate rig), and if the FT-991A analog level is only slightly above the noise floor, the gate would block the signal. The FT-991A IRS USB audio level has NOT yet been verified during a real IC-9700 TX burst.

---

## Audio device configuration

| Station | Role | Correct CPAL device | Notes |
|---------|------|---------------------|-------|
| rpi51 (IC-9700 ISS) | TX | `pulse` (PulseAudio default sink) | PulseAudio default sink = IC-9700 USB output. Confirmed working. |
| rpi51 (IC-9700 IRS) | RX | `plughw:CARD=CODEC,DEV=0` | **NOT `pulse`**. PulseAudio default source on rpi51 is the monitor of the USB output (captures TX feedback, not RX audio). Direct ALSA bypasses this. BUT: IC-9700 USB AF output must be enabled in the rig menu first. |
| dd2zm-landline (FT-991A ISS) | TX | `pulse` | PulseAudio default sink = FT-991A USB output. Confirmed working. |
| dd2zm-landline (FT-991A IRS) | RX | `pulse` | Need to verify whether PulseAudio default source on dd2zm-landline is the FT-991A input or a monitor. |

**Action required in config:**  
```bash
# In docs/config/onair-ic9700-ft991a.example.sh:
# IC-9700 RX (IRS role) must use direct ALSA, not PulseAudio default:
export A_AUDIO_DEVICE="plughw:CARD=CODEC,DEV=0"  # for IRS role
# IC-9700 TX (ISS role) must keep PulseAudio:
export A_AUDIO_DEVICE="pulse"  # for ISS role
```

The script does not currently support per-role audio device selection. Needs a code change or a second variable (e.g. `A_AUDIO_CAPTURE_DEVICE`).

---

## Open action items

| # | Action | Blocker? | Status |
|---|--------|----------|--------|
| 1 | Enable USB AF output on IC-9700 (Menu → Set → Connectors → USB AF/IF Output = AF) | **YES — gating for reverse test** | Requires physical rig access |
| 2 | Verify FT-991A USB AF output for non-reverse test (PulseAudio default source on dd2zm-landline) | YES — gating for forward test | Run `pactl info` on dd2zm-landline, check default source |
| 3 | Commit engine.rs changes on `fix/afc-settling-receive-with-timeout` | No | Pending — see below |
| 4 | Add per-role audio device config (capture vs. playback separate) | No | Low priority |
| 5 | Measure FT-991A IRS audio level during IC-9700 TX | No | After items 1/2 |

---

## Uncommitted changes (engine.rs)

All changes are on branch `fix/afc-settling-receive-with-timeout`. The engine.rs changes improve the receive pipeline but do not fix the hardware configuration issue above.

| Change | Purpose |
|--------|---------|
| `AFC_MAX_CORRECTION_HZ = 450.0` (was 100.0) | Accept rig crystal offsets up to ±400 Hz (IC-9700/FT-991A offset measured at ≈300–400 Hz) |
| One-shot anchor + 5-pass fine-track (main scan settling AND retry) | Avoid iterative divergence for signals at the Goertzel boundary (±400 Hz offset saturates iterative update) |
| Stability guard: \|fine − anchor\| ≤ 20 Hz | Distinguish stable signal (both passes agree) from noise (both passes random-walk) |
| Flat-noise guard: both anchor and fine < 5 Hz → skip | Avoid full decodes on silent sections where Goertzel returns ~0 Hz |
| Retry energy gate: 0.01 mean_sq | Skip noise-only positions in retry scan (relies on signal being > 10× noise power) |
| Retry trigger: T ≥ 12s, re-arm every 2s | Rate-independent trigger replacing sample-count threshold |

**To commit:**
```bash
git add crates/openpulse-modem/src/engine.rs docs/config/onair-ic9700-ft991a.example.sh
git commit -m "fix: AFC one-shot anchor, wider correction range, wall-clock retry trigger"
git push
```

---

## Test commands

```bash
# Single case, normal direction (IC-9700 ISS → FT-991A IRS):
source docs/config/onair-ic9700-ft991a.example.sh
export SSH_AUTH_SOCK=/run/user/1000/ssh-agent.socket
./scripts/run-onair-ic9700-ft991a.sh supervise --single-case 'BPSK250|none|64'

# Single case, reverse (FT-991A ISS → IC-9700 IRS):
export A_AUDIO_DEVICE="plughw:CARD=CODEC,DEV=0"   # use direct ALSA for IRS capture
./scripts/run-onair-ic9700-ft991a.sh supervise --single-case 'BPSK250|none|64' --reverse

# Verify IC-9700 USB AF output is working (run during FT-991A TX burst):
ssh dc0sk@dc0sk-rpi51 "arecord -D plughw:CARD=CODEC,DEV=0 -f S16_LE -r 8000 -c 2 -d 15 /tmp/ic9700-rx.raw"
# Analyze: look for energy jump on ch0 or ch1 at ~T=5s when FT-991A starts TX
```

---

## Hardware notes

| Item | Note |
|------|------|
| FT-991A PTT | CAT only, not RTS. `B_PTT_TYPE="CAT"`. |
| FT-991A RF power | Fixed at 5W remotely (Hamlib set_level RFPOWER unsupported for model 1035). |
| FT-991A PipeWire audio | ~3600 effective samples/s due to PipeWire buffering. IRS accumulates correctly but slowly. |
| IC-9700 audio (capture) | Must use `plughw:CARD=CODEC,DEV=0` not `pulse`. PulseAudio default source = monitor of TX output. |
| IC-9700 audio (USB AF out) | **NOT yet confirmed enabled.** Stereo capture shows no signal burst during FT-991A TX. Must enable in rig menu. |
| IC-9700 carrier offset | ≈ −300 to −400 Hz offset relative to FT-991A at same dial frequency (1200–1100 Hz audio instead of 1500 Hz). |
| IC-9700 S-meter (Hamlib) | STRENGTH returns dB relative to S9 reference; `str_max=18` ≈ S9+18dB = −55 dBm. |
| Freq | 144.640 MHz (moved from 144.650 MHz to avoid 1286 Hz interferer in FT-991A passband). |
