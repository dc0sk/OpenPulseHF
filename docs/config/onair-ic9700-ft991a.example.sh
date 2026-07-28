#!/usr/bin/env bash
# Profile for dual-SSH setup:
#   Station A (ISS): IC-9700 on dc0sk-rpi51
#   Station B (IRS): FT-991A on dd2zm-landline (about 10 km away)
#
# Usage:
#   source docs/config/onair-ic9700-ft991a.example.sh
#   ./scripts/run-onair-ic9700-ft991a.sh supervise --quick

# SSH targets (must be reachable via ssh-agent keys)
export A_SSH="dc0sk@dc0sk-rpi51"
export B_SSH="dd2zm@dd2zm-landline"
export SSH_OPTS='-o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=30'

# Callsigns
export CALLSIGN_A="DC0SK"   # IC-9700 (ISS)
export CALLSIGN_B="DD2ZM"   # FT-991A (IRS)

# Friendly labels for report output
export A_LABEL="IC-9700"
export B_LABEL="FT-991A"

# Hamlib models (verify with: rigctl -l | grep -i 'ic-9700\|ft-991')
export A_HAMLIB_MODEL=3081
export B_HAMLIB_MODEL=1035

# Station A rig/CAT/PTT settings
export A_CAT_PORT="/dev/serial/by-id/usb-Silicon_Labs_CP2102N_USB_to_UART_Bridge_Controller_IC-9700_13012889_A-if00-port0"
export A_CAT_BAUD=115200
export A_PTT_PORT="/dev/serial/by-id/usb-Silicon_Labs_CP2102N_USB_to_UART_Bridge_Controller_IC-9700_13012889_B-if00-port0"
# Prefer CAT PTT on the IC-9700; RTS is kept only as an explicit fallback.
export A_PTT_TYPE="CAT"
export A_RIGCTLD_ADDR="127.0.0.1"
export A_RIGCTLD_PORT=4532

# Station B rig/CAT/PTT settings
export B_CAT_PORT="/dev/serial/by-id/usb-Silicon_Labs_CP2105_Dual_USB_to_UART_Bridge_Controller_008924A1-if00-port0"
export B_CAT_BAUD=38400
export B_PTT_PORT="/dev/serial/by-id/usb-Silicon_Labs_CP2105_Dual_USB_to_UART_Bridge_Controller_008924A1-if01-port0"
# FT-991A PTT is via CAT (confirmed via js8call/flrig — RTS does not work).
export B_PTT_TYPE="CAT"
export B_RIGCTLD_ADDR="127.0.0.1"
export B_RIGCTLD_PORT=4532

# 2m safety guard for this test window (script enforces this range)
export BAND2M_MIN_HZ=144500000
export BAND2M_MAX_HZ=144750000
export TEST_FREQ_HZ=144600000
export TEST_MODE_RIG="PKTUSB"

# Per-side dial trim to null the measured rig-to-rig crystal offset (Gate 5).
# On 2026-07-28 the FT-991A ran ~64 Hz LOW of the IC-9700 at 144.6 MHz: with both
# commanded to 144.600000 the received audio carrier sat at 1436 Hz instead of 1500,
# i.e. at the edge of BPSK250's +-62.5 Hz AFC. Trimming Station B (FT-991A) up 64 Hz
# corrects the actual crystal difference and aligns BOTH directions (received carrier
# measured back at 1501.5 Hz). Re-measure with Gate 5 if a rig or its TCXO changes.
export A_FREQ_OFFSET_HZ=0
export B_FREQ_OFFSET_HZ=64

# IC-9700 audio prerequisites for digital USB TX (set on the radio UI):
# - DATA MOD = USB
# - USB MOD Level > 0 (start around mid-scale)
# - Correct DATA mode (USB-D/PKTUSB) selected

# Optional audio device pinning per station (leave empty for default)
# Use the PulseAudio sink rather than direct hw: access.
# PulseAudio holds the IC-9700 USB CODEC exclusively; hw:/plughw: access is
# blocked at the OS level and produces no RF even though aplay reports success.
# The PulseAudio default sink is:
#   alsa_output.usb-Burr-Brown_from_TI_USB_Audio_CODEC-00.analog-stereo
export A_AUDIO_DEVICE="pulse"
export A_AUDIO_DEVICE_LABEL="IC-9700 USB Audio CODEC (PulseAudio)"
export B_AUDIO_DEVICE="pulse"

# --- Audio routing + TX level (consumed by scripts/onair-setup-audio-routing.sh) ---
# Because A_AUDIO_DEVICE/B_AUDIO_DEVICE = "pulse" resolve to each host's DEFAULT sink
# (TX) and DEFAULT source (RX), those defaults must point at the rig CODEC, and the TX
# sink volume must be low enough that openpulse's near-full-scale audio does not pin the
# rig ALC. Run `scripts/onair-setup-audio-routing.sh` (set) once per session, then the
# runner's `--device pulse` reaches the radios. Gate-6-validated values (2026-07-28):
#   TX sink volume 0.15 gave a clean non-clipping BPSK250 that decoded first try.
# A_CODEC_MATCH/B_CODEC_MATCH are case-insensitive substrings of the CODEC's description
# in `wpctl status` (IC-9700: "PCM2901 Audio Codec"; FT-991A: "PCM2903B Audio CODEC").
export A_CODEC_MATCH="Codec"
export B_CODEC_MATCH="Codec"
export A_TX_SINK_VOLUME=0.15
export B_TX_SINK_VOLUME=0.15

# RX capture level — a DECODE GATE, not cosmetics. The modem's energy gate sets its
# threshold to clamp(idle_floor*3, 0.0001, 0.0032); if the idle floor is above 0.00107 the
# threshold clamps BELOW the noise, the gate never shuts, the receiver settles AFC on noise,
# and a perfectly aligned frame decodes to "invalid magic". Measured on air 2026-07-28:
#   IC-9700 at 1.00 -> idle mean_sq 0.0154 (CLAMPED) -> BPSK250 64B FAILED;
#   IC-9700 at 0.55 -> idle 0.00042, signal 0.0024   -> the same case PASSED (A1 1/1).
#   FT-991A needs no reduction: its USB AF output is quieter (idle 0.000125 at 1.00).
# These are PER-RIG measurements, not values to copy. Verify with:
#   scripts/onair-rx-level-check.sh
export A_RX_SOURCE_VOLUME=0.55
export B_RX_SOURCE_VOLUME=1.0

# Paths:
# A is a normal repo checkout and used as build source.
export A_REPO_DIR='${HOME}/git/OpenPulseHF'

# B can be non-git with limited disk; runner keeps repo-like layout and only
# transfers needed binaries to target/release.
export B_REPO_DIR='${HOME}/openpulse/OpenPulseHF'
export B_LOG_DIR='${HOME}/var/log/openpulse/on-air'

# Timing
export IRS_STARTUP_WAIT=5
export TX_TIMEOUT=120

# RF power (Hamlib scale 0.0–1.0; 0.05 = 5% of max).
# Pre-flight check aborts the run if this reads back as < 1% — set explicitly.
export A_RFPOWER=0.05   # IC-9700: ~5 W of 100 W (max 5 W for this test; verified PC/RFPOWER)
export B_RFPOWER=0.05   # FT-991A: PC005 = exactly 5 W (max 5 W for this test)

# Telemetry: capture PTT/ALC/RFM on ISS and STRENGTH on IRS during each case.
export TELEMETRY_ENABLE=1
export TELEMETRY_SAMPLES=40
export TELEMETRY_INTERVAL=0.2

# Safety note for report metadata
export ON_AIR_FIRST_PASS_NOTE="2m only, low power, agreed test window"

# Power cycle transceivers via Hamlib CAT before setup.
# Both radios enter CAT standby on power-off so the same serial port handles
# power-on. IC-9700 and FT-991A both support Hamlib P 0/1 commands.
# Set to 1 to enable; useful when resuming after a reboot or config change.
export POWER_CYCLE_ENABLE=0
export POWER_OFF_WAIT=10   # seconds between power-off and power-on commands
export POWER_ON_WAIT=15    # seconds after power-on before rigctld starts

# Hardware audio loopback regression (rpi51 ↔ rpi52 USB cable).
# run_loopback_regression() deploys the freshly-built binary to rpi52, then
# runs run-loopback-rpi51-rpi52.sh to verify the full audio+modem stack.
# LOOPBACK_TIER: 'quick' (4 modes, ~100s) or 'full' (8 modes, ~200s).
#   supervise/sidea always use 'full' at session start.
#   run action uses 'quick' (binary already deployed).
#   post-failure check uses the default tier below.
export LOOPBACK_IRS_SSH="dc0sk@dc0sk-rpi52"
export LOOPBACK_IRS_BIN_DIR="/home/dc0sk/openpulse/bin"
export LOOPBACK_TIER="quick"

# Periodic loopback interval: run a quick-tier loopback every N on-air test cases.
# Each quick loopback takes ~100s; set 0 to disable periodic checks.
# Session-start and post-failure checks always run regardless of this setting.
export LOOPBACK_REGRESSION_INTERVAL=0

# Side-A-only transmit smoke test defaults. The `sidea` action uses these when
# reducing the test loop to a single transmit path on the IC-9700.
export SIDE_A_SINGLE_CASE="BPSK250|none|64"
# Optional high-SWR tuner policy (all values are explicit opt-in defaults).
# When enabled, the runner checks SWR and attempts integrated tuner operation
# on rigs that support the Hamlib TUNER function.
export ALLOW_TUNER_ON_HIGH_SWR=0
export HIGH_SWR_THRESHOLD=2.0

# If QSY mode is enabled for the run, trigger the same SWR+tuner policy after
# tune/QSY transitions.
export QSY_MODE_ENABLED=0
export TUNER_TRIGGER_ON_QSY=1
