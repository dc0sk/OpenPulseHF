---
project: openpulsehf
doc: docs/troubleshooting-alsa-cpal.md
status: living
last_updated: 2026-10-15
---

# Troubleshooting ALSA and CPAL Audio Setup

This guide covers audio backend setup and runtime issues for OpenPulse. Linux (ALSA, with
PulseAudio or PipeWire on top) is the platform the project builds, tests and operates on
(Raspberry Pi 4/5 stations and x86 hosts). macOS is only compile-checked in CI, without the audio
backend; Windows is not built in CI. The macOS and Windows sections below are generic cpal advice,
not project-verified procedures.

Binary names used below: `openpulse` (CLI), `openpulse-server` (daemon), `openpulse-tnc` (ARDOP TNC).

## Platform Overview

OpenPulse uses the **`cpal`** crate for audio hardware. The CLI has it on by default (feature
`cpal-backend`); the daemon and the ARDOP TNC need `--features cpal`. A build without it has only the
in-memory loopback backend.

| Platform | cpal host | Common issues |
|---|---|---|
| **Linux** | ALSA (PulseAudio/PipeWire reached through their ALSA plugins, e.g. the `pulse` device) | Device not found, device held by another client, wrong card picked, `hw:` device rejects 8 kHz |
| **macOS** | CoreAudio | Not exercised by the project (no audio CI) |
| **Windows** | WASAPI | Not exercised by the project |

The modem runs at 8 kHz and opens the device at that rate. On Linux use a `plughw:` device or the
`pulse` device so ALSA or the sound server converts the rate; a raw `hw:` device that does not
support 8 kHz fails to open.

---

## General Diagnostics

### 1. Verify OpenPulse can see the backend and devices

```bash
# Is the backend flag there, and which audio devices does cpal see?
openpulse --help | grep backend
openpulse-tnc --help | grep backend
openpulse --backend cpal devices

# Registered modulation modes (the CLI has no plugin-listing command; modes is the equivalent)
openpulse modes
```

`openpulse devices` names every device the same way the transmit and receive paths resolve them,
so a name it prints is a name `--device` / `[audio] device` can open.

### 2. Check the binary was built with cpal

```bash
cargo build --release -p openpulse-cli                       # cpal-backend is on by default
cargo build --release -p openpulse-daemon --features cpal    # daemon
cargo build --release -p openpulse-ardop --features cpal     # ARDOP TNC
```

A CLI built `--no-default-features` prints `note: cpal backend not compiled in; falling back to
loopback` when run with the default backend. The ARDOP TNC logs a warning and uses loopback if
`--backend cpal` is requested without the feature.

---

## Linux (ALSA / PulseAudio / PipeWire)

### Issue: "No audio devices found" or the device does not open

**Symptoms**:
- `openpulse devices` prints `No audio devices found.`
- `openpulse transmit ...` / `receive ...` fails to open the configured device

**Root causes**:
- ALSA userspace missing or misconfigured
- The card is held exclusively by another client (often PulseAudio/PipeWire holding a USB CODEC)
- Permission on `/dev/snd/*`
- The selector names a device that is not present

**Solutions**:

1. **Install ALSA userspace and headers**:
   ```bash
   # Debian/Ubuntu/Raspberry Pi OS
   sudo apt-get install alsa-utils libasound2-dev

   # Fedora/RHEL
   sudo dnf install alsa-utils alsa-lib-devel

   # Arch
   sudo pacman -S alsa-utils alsa-lib
   ```

2. **Verify ALSA can enumerate devices**:
   ```bash
   aplay -L          # output devices
   arecord -L        # input devices
   arecord -l        # cards and their CARD= names
   ```

3. **Check for a sound server holding the card.** If PulseAudio or PipeWire owns a USB CODEC, a
   `hw:`/`plughw:` open fails while the server runs. Either select the server's device (`pulse`)
   or stop the server for that card. Example: the IC-9700's CODEC on rpi51 is held by PulseAudio,
   so that station uses `pulse`.
   ```bash
   systemctl --user status pulseaudio pipewire
   fuser -v /dev/snd/*
   ```

4. **Verify the user is in the audio group**:
   ```bash
   groups | grep audio
   sudo usermod -aG audio $USER   # then log out and back in
   ```

5. **Check device permissions**:
   ```bash
   ls -la /dev/snd/
   # your user needs read+write on /dev/snd/pcmC*D0p and /dev/snd/pcmC*D0c
   ```

### Issue: the wrong card is used

**Symptoms**:
- Audio goes to onboard audio or HDMI instead of the radio interface
- Works until a USB device is replugged or the card order changes

**Root cause**: no device selected, so the OS default is used. On a multi-card host that is
whichever card the OS prefers.

**Solutions**:

1. Select the device explicitly. The CLI takes `--device <NAME>` on `transmit`, `receive` and
   `arq`; the daemon and the ARDOP TNC read `[audio] device` in `config.toml`:
   ```toml
   [audio]
   backend = "cpal"
   device = "plughw:CARD=CODEC,DEV=0"
   ```
   ```bash
   openpulse receive --mode BPSK250 --device plughw:CARD=CODEC,DEV=0 --listen-ms 60000
   ```

2. Prefer the stable `CARD=` form over an index (`hw:1,0`). The selector is resolved in three
   steps: an exact device name, then the ALSA `CARD=` token, then a case-insensitive substring. So
   a card whose index shifts, or that the OS renames (e.g. gains a `(2)` suffix), still resolves.
   An ambiguous match is refused rather than guessed.

3. The first open of a named device enumerates every device to find it (about 0.5 s on rpi51);
   the resolved name is then cached, so later opens in the same process skip the enumeration. This
   matters for transmit: the daemon and TNC key PTT before opening the output, and the cache keeps
   that gap short. If a repeat open is slow, the name no longer matches and is being re-resolved.

### Issue: ALSA lib warnings on stderr

**Symptoms**:
- Stderr shows ALSA lib warnings at startup (e.g. `Unknown PCM cards`, `confmisc.c: ...`)
- Operation works but is noisy

**Root cause**: ALSA config references cards or plugins that are not present. cpal enumerates
every PCM, and ALSA prints a warning for each one it cannot open.

**Solutions**:

1. **Clean up `~/.asoundrc`**: back it up, then remove stale entries.
   ```bash
   cp ~/.asoundrc ~/.asoundrc.bak
   ```
   Keep entries written by the project's setup scripts (`scripts/setup-virtual-loopback.sh` adds
   `aloop_tx`/`aloop_rx`) if you use those rigs.

2. **Check system ALSA config** for references to unavailable services:
   ```bash
   grep -r "speech-dispatcher" /etc/alsa/ ~/.asoundrc 2>/dev/null
   ```

### Issue: capture level wrong, or level moves during a frame

**Symptoms**:
- Weak or no decodes although the signal is audible
- Phase-only modes decode but amplitude-carrying modes fail

**Root cause**: capture gain too low, clipped, or a USB adapter's capture AGC is on. An AGC moves the
level during a frame, which hurts every mode that carries bits in amplitude.

**Solutions**:
1. Set capture gain with `alsamixer` (F4 for capture) and check the level in a short `arecord`
   capture. (`openpulse calibrate audio` runs on the in-memory loopback, not on your device.)
2. Turn off the adapter's capture AGC. USB mixer state is not persistent: re-check after every
   replug. The dual-card rig's `scripts/setup-dualcard-loopback.sh` does this and verifies it.

---

## macOS (CoreAudio)

Not exercised by the project; CI only compile-checks macOS without the audio backend. Generic
cpal/CoreAudio advice:

- Confirm the device appears in System Settings > Sound (input and output) and in Audio MIDI Setup.
- Grant microphone access to the terminal you run `openpulse` from (System Settings > Privacy &
  Security > Microphone).
- Close other applications that may hold the device.

---

## Windows (WASAPI)

Not built or tested by the project. Generic cpal/WASAPI advice:

- Update the audio driver; disable "audio enhancements" on the radio interface.
- Disable "Allow applications to take exclusive control" on the device if another application
  holds it.
- Close applications that use the device (conferencing, streaming, other modem software).

---

## Cross-Platform Runtime Issues

### Issue: transmit stalls or fails with "output buffer did not drain"

**Root cause**: the output device stopped consuming samples. The drain deadline is computed from
the queued audio (its duration plus 25 % plus 5 s, at least 5 s, at most 600 s), so it always
exceeds the audio it waits on; when it fires, the driver is stuck, not slow.

**Solutions**: check that nothing else holds the device, that the USB interface is still present
(`arecord -l`), and that the device is not suspended by a sound server.

### Issue: crackling, stuttering, or glitches during transmission

**Root causes**: buffer underruns from CPU load or CPU throttling.

**Solutions**:
1. Reduce CPU load; close other applications.
2. On Linux, check the CPU governor:
   ```bash
   cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor
   echo performance | sudo tee /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor
   ```
   On a Raspberry Pi see [raspberry-pi-4-5-tuning-and-benchmarks.md](raspberry-pi-4-5-tuning-and-benchmarks.md).
3. The cpal buffer size is not configurable.

### Issue: transmit works but receive hears nothing

**Root causes**:
- Input muted or level too low
- The input is a different device from the one the radio feeds
- Loopback backend used across two processes

**Solutions**:

1. **Check input levels** (`alsamixer`, F4).
2. **Test the input directly**:
   ```bash
   arecord -D plughw:CARD=CODEC,DEV=0 -d 5 -f S16_LE -r 8000 test.wav && aplay test.wav
   ```
3. **Select the input explicitly** with `--device` or `[audio] device` (see above).
4. **Loopback is in-process only.** `--backend loopback` is an in-memory buffer; a loopback
   `transmit` in one process and a loopback `receive` in another never meet. Use real audio, an
   `snd-aloop` virtual loopback ([§13.2 C in the manual](openpulse-manual.md#c-virtual-single-clock-loopback-snd-aloop-one-host-no-radio)),
   or the hardware-free commands (`openpulse adaptive`, `openpulse --backend loopback benchmark run`).

---

## Audio Backend Selection and Fallback

`--backend` (CLI) and `[audio] backend` (daemon, TNC) take `default`, `cpal` or `loopback`.
`default` means cpal when compiled in, loopback otherwise. Within cpal, OpenPulse uses the
platform's default cpal host: ALSA on Linux, CoreAudio on macOS, WASAPI on Windows. There is no
option to pick a different cpal host; on Linux, reach PulseAudio or PipeWire through their ALSA
device (`pulse`, or `pipewire` where installed).

---

## Testing Audio Configuration

### Software-only check

```bash
# In-process: modulate, demodulate and rate-adapt over a simulated channel
openpulse adaptive --profile fast --channel awgn --snr 12 --frames 8
openpulse --backend loopback --log error benchmark run
```

### Real audio device test

Cable the output of one interface to the input of another (or use two stations), then:

```bash
# Receiver first
openpulse receive --mode BPSK250 --fec rs --device <RX_DEVICE> --listen-ms 60000

# Then, in another terminal or on the other host
openpulse transmit "TEST" --mode BPSK250 --fec rs --device <TX_DEVICE>
# The receiver should print "TEST"
```

For a no-cable virtual path use `scripts/setup-virtual-loopback.sh` and
`scripts/run-loopback-virtual.sh` (manual §13.2 C).

---

## Reporting Audio Issues

If you encounter audio issues not covered above, report:

1. **OS and version**: `uname -a`, distribution, sound server (`pactl info` / `pw-cli info`)
2. **Audio devices**: `aplay -L`, `arecord -l`, and `openpulse --backend cpal devices`
3. **OpenPulse and cpal versions**: `openpulse --version` and `cargo tree -i cpal`
4. **Full output at debug level**:
   ```bash
   openpulse --log debug receive --mode BPSK250 --listen-ms 30000 > openpulse-debug.log 2>&1
   ```
   The daemon and the TNC take their level from `RUST_LOG` if set, else `[logging] level`.
5. **Steps to reproduce**: the exact commands and config excerpt

Report to: [OpenPulseHF GitHub Issues](https://github.com/dc0sk/OpenPulseHF/issues)
