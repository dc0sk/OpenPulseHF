---
project: openpulsehf
doc: docs/dev/archive/onair-ic9700-ft991a-session-learnings-2026-06-04.md
status: resolved
last_updated: 2026-06-05
---

# IC-9700 / FT-991A On-Air Session Learnings

This note captures the concrete findings from the June 4 on-air session on the IC-9700 (side A) and FT-991A (side B).

## What we verified

- Side-A and side-B CAT/PTT control both work when the radios are in a clean idle state.
- The IC-9700 can key in CAT PTT and return to `PTT=0` reliably after explicit `T 0`.
- The FT-991A can also key and release cleanly through rigctld.
- FM carrier tests prove RF path continuity on both stations when the radio itself generates carrier.

## What failed

- OpenPulse transmit could assert PTT, but RF/ALC telemetry stayed at zero on several runs.
- On the FT-991A side, the receive/decode path did not recover payloads in reverse tests.
- On the IC-9700 side, a keyed transmit could modulate locally in software, but the RF/ALC readback still stayed at zero in the probing runs.

## Audio-path findings

- The USB Audio CODEC exposes only a playback control in ALSA mixer tools.
- There is no useful capture-gain control to monitor on this device.
- `alsamixer` is therefore not a good VU meter for this setup.
- `pavucontrol` is disruptive for these tests because it interacts with PipeWire; avoid it during on-air sessions.
- When PipeWire/WirePlumber is stopped on side B, OpenPulse can see the CODEC device again through CPAL.

## Rigctl settings probe

- The IC-9700 exposes at least `MICGAIN`, `COMP`, and `RFPOWER` through rigctl level queries.
- In the June 4 probe, `MICGAIN` read about 0.75 and `COMP` read about 0.50.
- `COMP` being nonzero matches the suspicion that compression can distort or mask a clean digital-data path.
- These settings are queryable and should be correctable through rigctl before data-mode tests.
- For data-mode transmit tests, verify the radio has compression disabled or minimized, and confirm a sane mic gain level before starting.
- `RFPOWER` was also readable and should be checked as part of the same preflight.

## Operational conclusions

- PTT alone is not enough to prove a usable TX path.
- For USB/data modes, the critical check is whether modulation actually reaches the radio and appears as RF/ALC movement.
- A clean FM carrier test can prove the CAT/PTT chain and RF path, but it does not validate audio modulation.
- The current failure mode is consistent with an audio-path or radio-input configuration gap, not just a software framing problem.

## Recommended improvements

- Add RF/ALC readback into OpenPulse so operators can see transmit-chain health during TX.
- Expose a short telemetry mode in the runner or CLI that prints PTT, RF power, ALC, and receive strength together.
- Preserve receiver logs before cleanup so decode failures can be inspected after a run.
- Add a preflight check for audio device visibility before transmit begins.
- Add a preflight check for IC-9700 transmit settings, especially mic gain and compression, before opening the TX window.
- Add automatic transmit-setting tuning in OpenPulse, with restore of the original radio settings after the test window closes.
- Treat the initial radio state as a saved baseline so tests can change mic gain, compression, power, and mode safely and then restore them.
- Prefer explicit device checks over ALSA mixer observation when validating the CODEC.
- Document the required operator flow: stop PipeWire/WirePlumber on side B before soundcard TX tests, then restore it afterward.

## Operator setup shortcuts

- Use the profile defaults for side A RTS PTT and side B CAT PTT only where supported by the radio.
- Keep side A on the expected operating mode for the chosen test.
- Keep side B in the digital/data mode used by the soundcard path.
- Verify the CODEC with a direct `amixer -c CODEC get PCM` readout instead of expecting a VU meter.
- If the goal is audio-path verification, use a tone injection test or a future RF/ALC readback in the software, not `alsamixer`.

## Root-cause resolution (2026-06-05)

**Root cause**: PulseAudio/PipeWire on dc0sk-rpi51 holds the IC-9700 USB CODEC
(`card 2: CODEC [USB Audio CODEC]`) exclusively. Direct ALSA `hw:CARD=CODEC,DEV=0`
access appeared to succeed (aplay reported no error) but produced zero RF/ALC because
PulseAudio silently discarded or blocked the write at the OS level.

**Fix**: `A_AUDIO_DEVICE=pulse` in the profile routes through PulseAudio to the
default sink (`alsa_output.usb-Burr-Brown_from_TI_USB_Audio_CODEC-00.analog-stereo`),
which is the IC-9700 USB CODEC. ALC confirmed at full deflection with this path.

**Diagnostic tool**: `scripts/audio-device-sweep-a.sh` sweeps all ALSA playback devices
with CAT PTT asserted, and now warns about the PulseAudio exclusive-hold pitfall.

## Next step

Run `./scripts/run-onair-ic9700-ft991a.sh sidea` with the updated profile to confirm
a BPSK250 frame transmits successfully end-to-end before bringing side B back into the loop.
