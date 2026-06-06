# Handoff 2026-06-05 — IC-9700 audio debug (RESOLVED)

**Status: RESOLVED** — root cause found and fixed on 2026-06-05.

## Root cause

PulseAudio/PipeWire on `dc0sk-rpi51` holds the IC-9700 USB CODEC
(`hw:CARD=CODEC,DEV=0`) exclusively. Any `hw:` or `plughw:` ALSA write
appeared to succeed (no error from aplay) but produced zero RF/ALC because
PulseAudio silently discarded the write.

JS8Call works because it uses the PulseAudio sink by name:
`alsa_output.usb-Burr-Brown_from_TI_USB_Audio_CODEC-00.analog-stereo`

OpenPulse (via CPAL) was using `hw:CARD=CODEC,DEV=0` — wrong for this Pi.

## Fix applied

`docs/config/onair-ic9700-ft991a.example.sh`:
```
A_AUDIO_DEVICE=pulse   # was: hw:CARD=CODEC,DEV=0
```

`pulse` routes through PulseAudio to the default sink, which on this Pi is
the IC-9700 USB CODEC. ALC hit full deflection immediately on first test.

The PulseAudio default sink was confirmed correct:
```
$ pactl info | grep 'Default Sink'
Default Sink: alsa_output.usb-Burr-Brown_from_TI_USB_Audio_CODEC-00.analog-stereo
```

## Files changed in this fix

- `docs/config/onair-ic9700-ft991a.example.sh` — `A_AUDIO_DEVICE=pulse`
- `docs/config/README.md` — updated description
- `docs/dev/archive/onair-ic9700-ft991a-session-learnings-2026-06-04.md` — resolution added
- `scripts/audio-device-sweep-a.sh` — warning added about PulseAudio exclusive hold
