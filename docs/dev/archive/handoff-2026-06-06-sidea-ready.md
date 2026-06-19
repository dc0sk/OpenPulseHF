# Handoff 2026-06-06 — side-A ready for smoke test

## State

Audio path is fixed. The IC-9700 on `dc0sk-rpi51` transmits RF when OpenPulse
routes audio through PulseAudio (`A_AUDIO_DEVICE=pulse`). CAT PTT is working.
ALC confirmed at full deflection with a 1 kHz tone via `aplay -D pulse`.

The profile `docs/config/onair-ic9700-ft991a.example.sh` is correct and ready.

## First thing to do

Run the side-A smoke test:
```bash
source docs/config/onair-ic9700-ft991a.example.sh
./scripts/run-onair-ic9700-ft991a.sh sidea
```

Expected: PASS with `alc>0` and `rfm>0` in telemetry. If it passes, the IC-9700
audio-to-RF path is fully validated for OpenPulse BPSK250 frames.

If it fails, check:
1. `TELEMETRY_ENABLE=1` in the profile to get per-sample ALC/RFM logs.
2. That rigctld is not stale — the runner restarts it as part of setup.
3. That PulseAudio default sink is still the IC-9700:
   `ssh dc0sk@dc0sk-rpi51 "pactl info | grep 'Default Sink'"`

## After sidea passes

Bring side B (FT-991A on `dd2zm-landline`) back into the loop:
```bash
source docs/config/onair-ic9700-ft991a.example.sh
./scripts/run-onair-ic9700-ft991a.sh supervise --quick
```

Side B uses `B_AUDIO_DEVICE=default:CARD=CODEC` — PipeWire is stopped on side B
before tests and restored after (see `stop_audio_services_b` in the runner).

## Key facts to remember

- `dc0sk-rpi51` runs PulseAudio. Always use `A_AUDIO_DEVICE=pulse`, never `hw:`.
- `pactl info | grep 'Default Sink'` must return the Burr-Brown/CODEC sink.
- The `scripts/audio-device-sweep-a.sh` tool is useful for future audio-path
  debugging — it tests all ALSA devices with PTT keyed and now warns about the
  PulseAudio exclusive-hold pitfall.
- CAT PTT model: `A_HAMLIB_MODEL=3081` (IC-9700), port via CP2102N serial bridge.
