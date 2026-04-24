---
project: openpulsehf
doc: docs/troubleshooting-alsa-cpal.md
status: living
last_updated: 2026-04-24
---

# Troubleshooting ALSA and CPAL Audio Setup

This guide covers common audio backend setup and runtime issues for OpenPulse on ALSA (Linux), PipeWire (Linux), CoreAudio (macOS), and WASAPI (Windows) platforms.

## Platform Overview

OpenPulse uses the **`cpal`** (Cross-Platform Audio Library) crate to abstract audio hardware. On each platform:

| Platform | Backend | Common Issues |
|---|---|---|
| **Linux (PulseAudio/ALSA)** | ALSA device enumeration, PipeWire via ALSA emulation | Device not found, permission issues, ALSA config corruption |
| **Linux (modern)** | PipeWire (via ALSA emulation) | PipeWire daemon not running, ALSA config mismatch |
| **macOS** | CoreAudio (coreaudio-sys) | Device not found, permission restrictions, USB device handling |
| **Windows** | WASAPI (Windows Audio Session API) | Device enumeration delays, exclusive mode conflicts, driver issues |

---

## General Diagnostics

### 1. Verify OpenPulse can detect backends and devices

```bash
# List available plugins (should show BPSK at minimum)
openpulse-cli plugins list

# List detected audio backends (output varies by platform)
# This requires a diagnostic command (planned for future release)
# For now, check manually:
```

### 2. Check if cpal was compiled with audio support

```bash
cargo build --manifest-path pki-tooling/Cargo.toml --features cpal-backend --verbose
# Look for log output confirming ALSA/WASAPI/CoreAudio crate compilation
```

### 3. Check OpenPulse was compiled with cpal backend

```bash
cargo build --features cpal-backend --verbose 2>&1 | grep -i audio
# Expected: successful compilation of openpulse-audio with cpal enabled
```

---

## Linux (ALSA / PipeWire)

### Issue: "No audio devices found" or "DeviceNotFound"

**Symptoms**: 
- `openpulse-cli transmit ...` fails with `DeviceNotFound` or similar
- `list_devices()` returns empty vector

**Root causes**:
- ALSA is not installed or misconfigured
- PipeWire daemon has crashed or is not running (modern Linux)
- ALSA config is corrupted
- udev rules are missing (permission issues)

**Solutions**:

1. **Check ALSA is installed**:
   ```bash
   # Debian/Ubuntu
   sudo apt-get install alsa-utils libasound2 libasound2-dev

   # Fedora/RHEL
   sudo dnf install alsa-utils alsa-lib-devel

   # Arch
   sudo pacman -S alsa-utils alsa-lib
   ```

2. **Verify ALSA can enumerate devices**:
   ```bash
   aplay -L          # List output devices
   arecord -L        # List input devices
   ```
   If no devices appear, proceed to step 3.

3. **Reset ALSA state**:
   ```bash
   # Reset ALSA mixer and configuration
   sudo alsamixer    # Interactive: press 'u' to unmute, check levels
   sudo alsactl init # Reset to defaults
   ```

4. **Check PipeWire (modern Linux)**:
   ```bash
   # Check if PipeWire is running
   systemctl --user status pipewire

   # If not running, start it
   systemctl --user start pipewire

   # Enable on boot
   systemctl --user enable pipewire
   ```

5. **Verify user is in audio group**:
   ```bash
   groups | grep audio
   # If 'audio' is missing:
   sudo usermod -aG audio $USER
   # Then log out and back in (or: su - $USER)
   ```

6. **Check for device permission issues**:
   ```bash
   ls -la /dev/snd/
   # Expected: your user should have read+write on /dev/snd/pcmC*D0p and /dev/snd/pcmC*D0c
   # If not, check udev rules:
   cat /etc/udev/rules.d/60-alsa.rules | grep snd
   ```

7. **Rebuild OpenPulse with ALSA backend**:
   ```bash
   cargo clean
   cargo build --features cpal-backend
   cargo test -p openpulse-modem
   ```

### Issue: ALSA lib warnings (e.g., "Unknown PCM cards" or "Disabling PulseAudio")

**Symptoms**:
- Stderr is flooded with ALSA lib warnings during startup
- Warnings like `confmisc.c: Cannot connect to /tmp/speech-dispatcher-<UID>`
- Operation works but is noisy

**Root cause**: ALSA libraries have stale config or are trying to load unavailable modules.

**Solutions**:

1. **Suppress warnings (non-breaking)**:
   ```bash
   # Set ALSA_CARD to the desired card (or check with `aplay -L`):
   ALSA_CARD=0 openpulse-cli receive BPSK31

   # Or silence all ALSA lib output:
   LIBASOUND_CONFDIR=/dev/null openpulse-cli receive BPSK31
   ```

2. **Clean up ALSA config**:
   ```bash
   # Backup your current config
   cp ~/.asoundrc ~/.asoundrc.bak

   # Remove or simplify ~/.asoundrc to remove stale entries
   rm ~/.asoundrc
   # Or edit it to only include devices you actively use
   ```

3. **Check ALSA config in system directories**:
   ```bash
   grep -r "speech-dispatcher" /etc/alsa/ ~/.asoundrc 2>/dev/null
   # Remove any references to unavailable services
   ```

### Issue: Device always selected is "default" or "hw:0,0" (mono or low quality)

**Symptoms**:
- OpenPulse works but audio quality is poor
- Sample rate is stuck at 8000 Hz when higher rates are available
- Mono output when stereo is expected

**Root cause**: ALSA default device is configured to a low-fidelity profile (e.g., modem or USB fallback).

**Solutions**:

1. **List available devices and their capabilities**:
   ```bash
   aplay -L | head -20
   arecord -L | head -20
   # Look for entries like "default", "front", "surround51", etc.
   ```

2. **Set a better default device**:
   ```bash
   # Edit ~/.asoundrc and add:
   defaults.pcm.card 0    # Replace 0 with your desired card number
   defaults.ctl.card 0
   ```

3. **For OpenPulse, explicitly specify device** (future enhancement):
   ```bash
   # Current: no option, uses first device
   # Planned: --audio-device hw:0,0 flag
   openpulse-cli receive --audio-device plughw:0,0 BPSK31
   ```

---

## macOS (CoreAudio)

### Issue: USB audio device not detected

**Symptoms**:
- USB microphone or speaker is plugged in but does not appear in device list
- `aplay -L` equivalent not available on macOS

**Root causes**:
- CoreAudio cache is stale
- USB device permissions not granted
- Device is in exclusive mode (claimed by another app)

**Solutions**:

1. **Check System Settings audio devices**:
   ```bash
   # Open System Settings > Sound
   # Verify your device appears in both Input and Output tabs
   ```

2. **Restart CoreAudio daemon**:
   ```bash
   sudo launchctl stop com.apple.audio.coreaudiod
   sudo launchctl start com.apple.audio.coreaudiod
   ```

3. **Clear CoreAudio cache**:
   ```bash
   rm -rf ~/Library/Preferences/com.apple.audio.coreaudiod.plist
   rm -rf ~/Library/Caches/com.apple.audio.*
   # Then restart CoreAudio
   sudo launchctl stop com.apple.audio.coreaudiod
   sleep 2
   sudo launchctl start com.apple.audio.coreaudiod
   ```

4. **Check device permissions** (App Sandbox / Gatekeeper):
   ```bash
   # If OpenPulse is blocked by security restrictions:
   # System Settings > Security & Privacy > Microphone: Allow OpenPulse
   # System Settings > Security & Privacy > Privacy tab
   ```

5. **Close other audio apps**:
   - GarageBand, Logic Pro, Audacity, or other audio software may claim exclusive access
   - Close them before running OpenPulse

### Issue: Sample rate mismatch or "cannot open device"

**Symptoms**:
- Error: "The specified device is not open"
- Audio distortion or crackles
- Sample rate conversion errors

**Root cause**: CoreAudio default sample rate doesn't match OpenPulse request.

**Solutions**:

1. **Check current system sample rate**:
   ```bash
   # Use Audio Midi Setup (built-in app)
   open /Applications/Utilities/Audio\ MIDI\ Setup.app
   # Right-click your device, check sample rate (should be 44100 or 48000)
   ```

2. **Set a compatible sample rate**:
   ```bash
   # In Audio MIDI Setup, set all devices to 48000 Hz
   ```

3. **Build OpenPulse with 48kHz as default**:
   ```bash
   # Edit crates/openpulse-core/src/audio.rs
   # Change: sample_rate: 8000, to sample_rate: 48000,
   # Rebuild: cargo build --features cpal-backend
   ```

---

## Windows (WASAPI)

### Issue: "No audio devices found" or long enumeration delay

**Symptoms**:
- Startup hangs for 5–30 seconds before failing
- No devices detected even though devices appear in Windows Settings
- `openpulse-cli` command-line tools are slow to start

**Root causes**:
- Audio driver is outdated or corrupt
- WASAPI device enumeration is slow (known issue on some systems)
- Realtek or other vendor audio drivers have incompatibilities with cpal

**Solutions**:

1. **Update audio drivers**:
   ```powershell
   # Windows 11/10: Settings > Update & Security > Optional Updates
   # Look for "Audio" or soundcard driver updates
   # Or: Device Manager > Audio inputs and outputs > Right-click > Update driver
   ```

2. **Disable problematic audio enhancements**:
   ```powershell
   # Right-click volume icon > Open Volume mixer
   # Advanced > App volume and device preferences
   # Disable "audio enhancements" for OpenPulse
   ```

3. **Clear WASAPI cache**:
   ```powershell
   # Remove cached device list (warning: will be regenerated)
   $AppData = $env:APPDATA
   Remove-Item "$AppData\..\Local\Microsoft\Windows\Caches\*Audio*" -Force -ErrorAction SilentlyContinue
   ```

4. **Rebuild with WASAPI debugging**:
   ```bash
   # Rebuild with verbose logging
   RUST_LOG=debug cargo build --features cpal-backend --release
   ./target/release/openpulse-cli receive BPSK31 2>&1 | grep -i wasapi
   ```

### Issue: Device in use / "already open" error

**Symptoms**:
- Error: "The audio device is in exclusive use"
- Cannot open both input and output simultaneously
- Other apps can use the device but OpenPulse cannot

**Root cause**: Device is in exclusive mode, or another app is using it.

**Solutions**:

1. **Close conflicting applications**:
   - Discord, Skype, streaming apps (OBS, Elgato)
   - Browser tabs with audio
   - Background recording/streaming services

2. **Disable exclusive mode in audio settings**:
   ```powershell
   # Right-click volume icon > Open Volume mixer
   # Device properties > Advanced > Exclusive mode
   # Uncheck "Allow applications to take exclusive control"
   ```

3. **Check for stuck/zombie processes**:
   ```powershell
   tasklist | findstr /i audio  # May show "WaveOutMix" or similar
   # Use Task Manager to close if stuck
   ```

---

## Cross-Platform Runtime Issues

### Issue: Crackling, stuttering, or glitches during transmission

**Symptoms**:
- Audio output has clicks, pops, or brief silences
- Regular pattern of glitches (e.g., every 2–5 seconds)
- CPU usage is high (>50% for a single-thread modem operation)

**Root causes**:
- Buffer underruns (audio consumed faster than produced)
- System CPU overload
- Driver latency issues
- Sample rate mismatch

**Solutions**:

1. **Increase buffer size** (if configurable in future releases):
   ```bash
   # Current: uses cpal defaults (~512 samples)
   # Planned: --buffer-size 2048 or similar flag
   ```

2. **Reduce CPU load**:
   ```bash
   # Close other applications
   # Disable unneeded services
   # Use task affinity to pin OpenPulse to one core
   ```

3. **Check system CPU throttling**:
   ```bash
   # Linux
   cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_driver
   # If "powersave", try:
   echo performance | sudo tee /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor

   # Windows
   # Settings > System > Power & sleep > Power plan > High performance
   ```

4. **Verify sample rate consistency**:
   ```bash
   # Ensure all devices use the same sample rate (8000, 16000, 48000)
   # Mismatches force real-time resampling, causing artifacts
   ```

### Issue: No audio output or intermittent reception

**Symptoms**:
- Transmit works but receive fails silently
- Audio appears on device but OpenPulse doesn't "hear" it
- Intermittent reception (works sometimes, fails others)

**Root causes**:
- Input device is muted or level is too low
- Device is not set as default
- Buffer is not flushed before read
- Timing/synchronization issue

**Solutions**:

1. **Check input device levels**:
   ```bash
   # Linux
   alsamixer  # Press F4 to view capture levels, set to ~80%

   # macOS
   # Open Audio Midi Setup > Select device > Check input level

   # Windows
   # Right-click volume > Open Volume mixer > Confirm input level
   ```

2. **Test input device directly**:
   ```bash
   # Linux
   arecord -d 5 test.wav && aplay test.wav  # Record 5 sec, play back

   # macOS
   afrecord -d 5 -f WAVE -b 16 test.wav && afplay test.wav

   # Windows (PowerShell)
   # Use built-in voice recorder or:
   # https://github.com/PowerShell/PowerShell/wiki/Audio-recording-on-Windows
   ```

3. **Verify input device is selected**:
   ```bash
   # Set as system default (all platforms)
   # Then re-run OpenPulse
   ```

4. **Check for loopback misconfiguration**:
   ```bash
   # If using loopback backend for testing:
   # Loopback is only for transmit→receive within same process
   # For real HF work, use hardware audio device
   ```

---

## Audio Backend Selection and Fallback

### Preference order (cpal auto-selection)

If OpenPulse detects multiple backends:

1. **Linux**:
   - PipeWire (if available and running)
   - ALSA (fallback)
   - PulseAudio (legacy, if enabled in cpal)

2. **macOS**:
   - CoreAudio (only option)

3. **Windows**:
   - WASAPI (only option for modern Windows)

### Force a specific backend

Current releases do not expose backend selection flags. Future enhancement:

```bash
# Planned (not yet available):
openpulse-cli --audio-backend alsa receive BPSK31
openpulse-cli --audio-backend pipewire receive BPSK31
```

For now, to force ALSA on Linux:

```bash
# Set environment variable (cpal may respect this):
ALSA_CARD=0 openpulse-cli receive BPSK31
```

---

## Testing Audio Configuration

### Loopback test (software-only)

```bash
# Create a loopback connection to verify modulation/demodulation
openpulse-cli transmit "Hello World" BPSK100 --backend loopback
openpulse-cli receive BPSK100 --backend loopback
# (Note: --backend flag is planned; current versions use default)
```

### Real audio device test

```bash
# Connect a speaker to the output device
# Connect a microphone to the input device
# Transmit a known signal:
openpulse-cli transmit "TEST" BPSK31

# Then receive in another terminal:
openpulse-cli receive BPSK31
# Should decode "TEST"
```

---

## Reporting Audio Issues

If you encounter audio issues not covered above, please report:

1. **OS and version**: `uname -a` (Linux), `sw_vers` (macOS), Windows version
2. **Audio device**: `aplay -L` (Linux), Audio MIDI Setup (macOS), Windows Settings
3. **OpenPulse and cpal versions**: `cargo tree | grep -E "openpulse|cpal"`
4. **Full error output**:
   ```bash
   RUST_LOG=debug openpulse-cli receive BPSK31 2>&1 > openpulse-debug.log
   ```
5. **Steps to reproduce**: Clear sequence of commands that trigger the issue

Report to: [OpenPulseHF GitHub Issues](https://github.com/dc0sk/OpenPulseHF/issues)

